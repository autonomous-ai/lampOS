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
//! is asked; words it holds end the turn unanswered (#1014). With
//! [`Engine::doing_though_held`] (`LAMP_REPORTS=on`), a held line whose
//! words name reports ([`crate::attention::HELD_ASKED_WORDS`]) is still
//! asked once, told only the functions done though a line's answer is held
//! ([`crate::attention::DONE_THOUGH_HELD`]), as the Live route's model
//! hears every line: those calls are done, and nothing of it is said.
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
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{
    sync::mpsc,
    task::{JoinHandle, JoinSet},
};

pub mod link;
pub mod net;
mod own_echo;
pub mod policy;
pub mod probe;
mod services;
pub mod speech_tags;
pub mod worker;

pub use services::{
    CHAT_WARM_ENVIRONMENT, Listener, Net, RELAY_ROUTES_ENVIRONMENT, Routable,
    SPEECH_SOCKET_ENVIRONMENT, SPEECH_SOCKET_MODEL_ENVIRONMENT, SPEECH_WARM_ENVIRONMENT, Services,
};

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
/// sound, and then waits another [`BEGIN_WITHIN_AFTER_CUE`]: a "hmm" followed
/// by silence is the worst of both (the first builder, 15:43). On lamp-4ace
/// on 2026-10-16, 9 of 10 backup turns waited past 1.5 s.
pub const CUE_AFTER: Duration = Duration::from_millis(1_500);
/// Time allowed for the first chat event after the scheduled cue, when a
/// clip is ready. The deadline from the words is the configured cue delay
/// plus this interval: 4.5 s with [`CUE_AFTER`], 7 s with [`CUE_AFTER_MOST`].
pub const BEGIN_WITHIN_AFTER_CUE: Duration = Duration::from_secs(3);
/// With it, the wait sound comes this many milliseconds after the words in
/// place of [`CUE_AFTER`], from [`CUE_AFTER_LEAST`] to [`CUE_AFTER_MOST`]
/// (Fourth's breakdown of the silences on lamp-4ace, #2602, 21 Oct 20:41).
/// The first answer audio comes at about 2 s now (p50 1.99 s, p90 3.47 s
/// over 96 turns), so at 1.5 s the sound came in about 40% of turns, most
/// of them about to answer: "hmm", then a gap, then the answer. Later, it
/// comes only for a slow chat, where a sign that it heard helps.
pub const CUE_AFTER_ENVIRONMENT: &str = "LAMP_SECOND_CUE_AFTER_MS";
/// The earliest the sound may come.
pub const CUE_AFTER_LEAST: Duration = Duration::from_millis(500);
/// The latest scheduled sound. A ready clip still leaves
/// [`BEGIN_WITHIN_AFTER_CUE`] for the chat's first event after this time.
pub const CUE_AFTER_MOST: Duration = Duration::from_millis(4_000);

/// When the wait sound comes for `value` ([`CUE_AFTER_ENVIRONMENT`]): its
/// milliseconds, from [`CUE_AFTER_LEAST`] to [`CUE_AFTER_MOST`]; `None`
/// unset, and an error for anything else.
pub fn cue_after(value: Option<&str>) -> Result<Option<Duration>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let after = value
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_millis)
        .filter(|after| (CUE_AFTER_LEAST..=CUE_AFTER_MOST).contains(after))
        .ok_or_else(|| {
            format!(
                "{CUE_AFTER_ENVIRONMENT} is milliseconds from {} to {}, not {value:?}",
                CUE_AFTER_LEAST.as_millis(),
                CUE_AFTER_MOST.as_millis()
            )
        })?;
    Ok(Some(after))
}
/// A look's first round may take this long: a picture is slower to read
/// than words (lamp-52e6 on 2026-10-16, one of two real looks past 3 s; the
/// first builder, 21:25). Past it, after "Let me look.", the lamp says
/// [`COULD_NOT_LOOK`] rather than nothing.
pub const LOOK_BEGIN_WITHIN: Duration = Duration::from_secs(5);
/// With `LAMP_SECOND_WRITTEN_WAIT=on` ([`WRITTEN_WAIT_ENVIRONMENT`]): a turn
/// the lamp wrote itself (news, a skill's say, a reminder; not a look) may
/// begin this late. Nobody asked and nobody waits, so [`BEGIN_WITHIN`]'s
/// reason doesn't hold, and a turn given up is news lost: on lamp-75c9 on
/// 20 Oct at 18:19 "Devin finished" began past 3 s, ended unanswered, and
/// was never told (`no_audio_answer`). Once its chat has begun, nothing on
/// this route bounds its first audio (the provider file's 8 s no-reply
/// wait, `config::PATIENT_NO_REPLY_MS`, is a Live session's).
pub const WRITTEN_BEGIN_WITHIN: Duration = Duration::from_secs(7);
/// What the lamp says when a look's first round has not begun within
/// [`LOOK_BEGIN_WITHIN`]: after "Let me look.", silence reads as broken.
pub const COULD_NOT_LOOK: &str = "I couldn't get a good look.";
/// A spoken request's words cut short before any of its answer was heard
/// are answered in the next line's place when that line is held, within
/// this ([`Shared::cut`], #1712).
pub const CARRY_WITHIN: Duration = Duration::from_secs(30);
/// The most of the last line's sound kept for the next: its last 1.5 s at
/// 16 kHz, enough for "Lamp," and a pause. Carried whole, 2 to 4 s of it
/// held each line's words back 1 to 2.5 s while Deepgram worked through it
/// (the first builder on lamp-4ace on 19 Oct at 18:56).
pub const KEPT_SAMPLES: usize = 16_000 * 3 / 2;
/// A line beginning this soon after the last one ended, that one not
/// answered, goes on from it: its sound goes first ([`KeptSound`]).
pub const CARRY_AFTER_END: Duration = Duration::from_millis(1_500);
/// The last line's sound is let go this long after it ended, whatever comes.
pub const KEEP_AFTER_END: Duration = Duration::from_secs(2);
/// How long a line that may go on from the one it cut short waits for a
/// voice print still to come ([`Joined::Coming`]): the same-voice child's
/// prints came 181–311 ms after they were asked on lamp-52e6 on 21 Oct
/// (Seventh on Queue #2602, 17:3x). Only such a line pays it.
pub const JOIN_WAIT: Duration = Duration::from_millis(300);
/// Functions one chat request of this route may be told of. Live's bound of
/// [`lamp_gemini::MAX_TOOLS`] (32) is Gemini Live's; the chat models this
/// route asks take far more (DeepSeek's API up to 128). On lamp-4ace on
/// 22 Oct the lamp's own 17 actions, its skills' and the voice's own came to
/// 37, and Live's bound left out the time, the search and the light scenes
/// from 00:35 on, and the web search too after the look became one of the
/// core at 15:42: "what's the weather in Hanoi" was answered from the
/// lamp's own place, with no search (the owner at the lamp, 16:0x).
pub const MAX_TOOLS: usize = 48;
/// How often it asks again meanwhile.
const JOIN_POLL: Duration = Duration::from_millis(25);
/// How soon after the cut line's end a line must begin to be joined with
/// its words ([`Engine::joining_cut_words`]): its own, longer than
/// [`CARRY_AFTER_END`], as [`OPEN_NAME_WITHIN`] is, since a join needs one
/// person's sign as well. On lamp-4ace on 21 Oct at 18:36 a played request
/// with a 1.2 s pause in it began its second half 1635 ms after the first
/// half's end, past 1.5 s. 3 s, not 2.5: in the owner's test on lamp-4ace
/// on 22 Oct (19:04-19:08), request 36 began about 2.6 s after request 35,
/// which it cut short, ended (at least 2.14 s by the journal; 35's wait
/// sound came 1.79 s after its end and was cut 0.81 s in), and the
/// others 1.45 to 2.45 s after; the gap counts the coordinator's hold of a
/// sound said while an answer is awaited (`LAMP_WAIT_KEEPS_ANSWER`, up to
/// 1 s), not only the person's pause.
pub const JOIN_WITHIN: Duration = Duration::from_millis(3_000);
/// The most words a chain of lines cut short is asked with, its newest
/// line's own among them ([`Engine::joining_cut_words`]): the oldest pieces
/// past it are let go. People's requests broke into 3 or 4 pieces at
/// natural pauses on lamp-4ace on 22 Oct ("Lamp. / I've been feeling a bit
/// tired lately. / Like, every afternoon, / Why could that be?", 14 words);
/// the longest played request was under 40 words.
pub const CHAIN_MOST_WORDS: usize = 60;
/// How long a chain of lines cut short keeps its oldest piece: past this
/// since that piece's words were kept, it is let go.
pub const CHAIN_MOST_FOR: Duration = Duration::from_secs(20);
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
/// round has not begun within its bound, rather than nothing (the first
/// builder, 2026-10-17 02:48: the relay's chat stopped answering at 02:29
/// and the backup voice went silent). Said as a slow moment, never a fault:
/// "I can't think right now" read as broken to the tester on a 7071 lamp
/// (#3755, 27 Oct 13:53).
pub const DOWN_LINE: &str = "Sorry, my thoughts are slow just now. Ask me again in a moment.";
/// What it says instead when the last of these was [`DOWN_LINE`], less than
/// [`DOWN_LINE_EVERY`] ago: shorter, and never the same line twice in a row
/// (#3755). After this one, the next is [`DOWN_LINE`] again.
pub const DOWN_LINE_AGAIN: &str = "Still slow, sorry.";
/// What it says when the chat refuses before any word for a reason asking
/// again won't mend (401, 402, 403, any other 4xx but 429: a wrong or
/// expired key, no credit): no promise of a moment (the Reviewer-lead's P3
/// on #3771). At most once in [`DOWN_LINE_EVERY`]; asking again changes
/// nothing, so it isn't repeated.
pub const DOWN_LINE_CANNOT: &str = "Sorry, I can't answer just now.";

/// Whether a refusal before any word is passing (429, a 5xx), which the slow
/// line suits, or one asking again won't mend.
fn passing(status: u16) -> bool {
    status == 429 || status >= 500
}
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
/// A held line's one round ([`Engine::doing_though_held`]), its first event
/// and its calls together, is given up past this: the line is over by then.
pub const HELD_WITHIN: Duration = Duration::from_secs(6);
/// Beside a held line's words, for its one round: what it is asked for.
pub const HELD_NOTE: &str = "These words were not said to you, and nothing you say now is heard. If the person asks to stop sending reports, call reports_stop; otherwise call nothing.";
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
/// The second route's guard showed a request's own ask for everything
/// (#3608): its words now, the same person's ask just before, or their yes
/// to one held. Only this module makes one, so only this route can mark a
/// request as asking for everything, the mark a forget of everything needs
/// before it takes the face too
/// ([`crate::memory_scope::ScopeLease::mark_everything_asked`], #3564).
pub(crate) struct EverythingShown(());

/// [`EverythingShown`] for a test of the face step itself.
#[cfg(test)]
pub(crate) fn everything_shown_for_test() -> EverythingShown {
    EverythingShown(())
}

pub type ScopedToolRunner = Arc<
    dyn Fn(u64, bool, String, Value, crate::memory_scope::ScopeLease) -> BoxFuture<'static, Value>
        + Send
        + Sync,
>;
/// A separate capability for the worker's microphone turns. Messages receives
/// only `ToolRunner`, even when a text message asks to speak its answer aloud.
pub(crate) type RequestedAudioRunner = Arc<
    dyn Fn(
            lamp_msgs::RequestedAudioContext,
            lamp_gemini::ToolCall,
            crate::memory_scope::ScopeLease,
        ) -> BoxFuture<'static, crate::tools::ToolAnswer>
        + Send
        + Sync,
>;

#[derive(Clone, Copy)]
struct SpokenTurn {
    request: u64,
    session: u64,
    privacy_generation: u64,
    captured: bool,
    enrollment_generation: crate::remember_speaker::Generation,
}

// The background room-speaker publish must use the request's existing proof;
// the room reading itself supplies no enrollment evidence.
fn publish_room_line(
    mut seen: crate::speaking_faces::LineSeen,
    remember_speaker: bool,
    lease: Option<&crate::memory_scope::ScopeLease>,
) {
    if remember_speaker {
        if let Some(lease) = lease {
            crate::speaking_faces::saw_line_scoped(seen, lease);
        }
    } else {
        // The off path keeps room-speaker attribution without enrollment.
        seen.enrollment = None;
        crate::speaking_faces::saw_line(seen);
    }
}

fn audio_scope_current(
    scope: &crate::memory_scope::ScopeLease,
    context: &lamp_msgs::RequestedAudioContext,
) -> bool {
    scope.is_current()
        && scope.key().request == context.request
        && scope.key().session == context.provider_session
        && crate::process::boot_text(scope.key().worker) == context.worker_boot
}

/// The system message for a turn, built when the turn's chat begins.
pub type System = Arc<dyn Fn() -> Told + Send + Sync>;
/// The system message built for the admitted request's immutable scope, never
/// for whoever is in view when the chat begins.
pub type ScopedSystem = Arc<dyn Fn(&crate::memory_scope::ScopeLease) -> Told + Send + Sync>;

/// What a turn's chat is told: the system message, and the exchanges just
/// before it as messages (user, then assistant), oldest first. Both come
/// from the thread, so they are the person's at the lamp only and nothing
/// that was forgotten (`crate::thread::Thread::forget`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Told {
    pub text: String,
    /// With `LAMP_SECOND_STEADY_FIRST=on` ([`STEADY_FIRST_ENVIRONMENT`]),
    /// what changes from one turn to the next (the memory note, the time,
    /// the room's facts and the thread), told as a system message after
    /// the exchanges, so `text` keeps only what stays. `None`: all in `text`.
    pub after: Option<String>,
    pub history: Vec<Message>,
}
impl From<String> for Told {
    /// With no exchanges before it.
    fn from(text: String) -> Self {
        Self {
            text,
            after: None,
            history: Vec::new(),
        }
    }
}
impl Told {
    /// The chat's messages before a turn's notes and words: the system
    /// message, the exchanges, then [`Self::after`]. What stays from one
    /// request to the next comes first, as a service's prompt cache holds
    /// only a prefix that is the same as before (#1712, the first builder
    /// at 18:56: DeepSeek's `prompt_cache_hit_tokens`).
    pub fn messages(self) -> Vec<Message> {
        let mut messages = vec![Message::system(self.text)];
        messages.extend(self.history);
        messages.extend(self.after.map(Message::system));
        messages
    }

    /// Without `section` wherever its system text holds it (a dated
    /// follow-up's, reviewer-two's P2 on #3059); the exchanges as they are.
    pub fn without(mut self, section: &str) -> Self {
        if section.is_empty() {
            return self;
        }
        self.text = self.text.replace(section, "");
        self.after = self.after.map(|after| after.replace(section, ""));
        self
    }
}

/// Where the worker reads whether a spoken turn's system message keeps only
/// what stays (the character and the skills) and tells the rest after the
/// exchanges ([`Told::after`]): `on`, and nothing else (#1712, the first
/// builder at 18:56).
pub const STEADY_FIRST_ENVIRONMENT: &str = "LAMP_SECOND_STEADY_FIRST";

/// Where the worker reads whether, in a quiet room, a line let out only as
/// the conversation going on is held when it is surely someone else's than
/// the one the lamp answered: `on`, and nothing else (the first builder at
/// 21:03 and 01:21 on Queue #1722,
/// `Heeding::going_on_quiet_only_for_the_same_one`).
pub const QUIET_GOING_ON_SAME_ONE_ENVIRONMENT: &str = "LAMP_QUIET_GOING_ON_SAME_ONE";

/// Why that rule holds such a line, as its journal says it after "held
/// back: ".
pub(crate) const QUIET_GOING_ON_HELD: &str = "a quiet room: a conversation with the lamp is going on, but the line is someone else's: another voice than the conversation's, or a face speaking it, not turned to the lamp, that is not the one it answered";

/// Why the second route holds a line let out only by a trusted session
/// when its voice print says someone else's than the conversation's, with
/// `LAMP_QUIET_GOING_ON_SAME_ONE` or `LAMP_GOING_ON_NOT_OWN` on (the first
/// builder at 09:40 on #2428), as its journal says it after "held back: ".
pub(crate) const TRUSTED_NOT_CARRIED: &str =
    "the session's trust is the caller's, and the line's voice print says someone else";

/// Where the worker reads whether a spoken turn's chat is given only the
/// functions a spoken turn mostly needs, and the rest behind one call
/// (`worker::few_tools`): `on`, and nothing else (#1712, the first builder
/// at 18:56).
pub const FEW_TOOLS_ENVIRONMENT: &str = "LAMP_SECOND_FEW_TOOLS";

/// Where the worker reads whether a round that said words before its calls
/// tells the next round so ([`Engine::saying_once`]): `on`, and nothing else
/// (the first builder at 06:56 on #1722: on lamp-4ace "Set a timer for one
/// minute." got "A minute, starting now. One minute, counting from now —
/// six forty-four and change.", 2 of the last 4 runs).
pub const SAID_ONCE_ENVIRONMENT: &str = "LAMP_SECOND_SAID_ONCE";

/// Where the worker reads whether the chat is told a few emotion tags its
/// voice may read ([`speech_tags`], [`Engine::speaking_tags`]): `on`, and
/// nothing else (#2901; the lead's proposal on #1711, 22 Oct 12:4x).
pub const SPEECH_TAGS_ENVIRONMENT: &str = "LAMP_SECOND_SPEECH_TAGS";
/// Where the worker reads whether a turn whose every call is a light or
/// volume change lamp-actions answered short is answered with the calls'
/// own line when the round after them adds nothing
/// ([`Engine::answering_short`]):
/// `on`, and nothing else. The same switch as lamp-actions' short answer
/// (`lamp_actions::SHORT_ENVIRONMENT`, #1520): on the second route the
/// model still spoke about ten words after it, a median of 10 off and 10 on
/// (Fifth's row 24 on lamp-52e6, #2877).
pub const SHORT_DONE_ENVIRONMENT: &str = "LAMP_SHORT_DONE";

/// Where the worker reads whether a spoken line that begins with the lamp's
/// name, answered with no words and no call at all, is asked once more, and
/// given [`NO_WORDS_LINE`] if the second answer has none either; never a
/// stop or "never mind" said with the name
/// ([`Engine::asking_again_for_no_words`], #3704): `on`, and nothing else.
/// On lamp-52e6 on 27 Oct "Lamp, What's my name?" got an empty answer and
/// the lamp said nothing.
pub const NO_WORDS_AGAIN_ENVIRONMENT: &str = "LAMP_NO_WORDS_AGAIN";

/// What the model is told when its answer to a line said to the lamp by name
/// had no words and called nothing ([`NO_WORDS_AGAIN_ENVIRONMENT`]).
pub(crate) const NO_WORDS_AGAIN_NOTE: &str = "(For you to know: they called you by name and your answer had no words, so they heard nothing. Answer them now, in a few words. If you can't, say so briefly.)";

/// What the lamp says when the model's second answer to a line said to it
/// by name has no words either ([`NO_WORDS_AGAIN_ENVIRONMENT`]): never
/// silence after its name.
pub(crate) const NO_WORDS_LINE: &str = "Sorry, I lost my words there. Could you ask me again?";

/// What a turn of short answers is said when its calls' own lines differ
/// ("Brighter." with "Quieter."): one line, never two (#2877).
pub(crate) const SHORT_DONE_TOGETHER: &str = "Done.";

/// Where the worker reads whether the lamp's name heard only in the room's
/// sound needs a second sign to count: a face turned to the lamp, said close
/// to it, or its words calling it (`on`, and nothing else; at any score).
/// On lamp-4ace on 2026-10-20 at 18:20 a hearing of 0.9 opened a
/// conversation nobody began (the first builder at 21:03 on #1722).
pub const NAME_SECOND_SIGN_ENVIRONMENT: &str = "LAMP_NAME_SECOND_SIGN";

/// Where the worker reads whether, with [`NAME_SECOND_SIGN_ENVIRONMENT`], a
/// hearing of the name at the line's start is no second sign of its own
/// when the line's first word, set off, is said to a person
/// ([`crate::attention::SAID_TO_A_PERSON`]), and a conversation with the
/// lamp going on is one (`on`, and nothing else). On c0a4 "Ma'am, could you
/// pass me the salt please?", said to a person, was heard at 0.95 to 0.99
/// at its start and answered (#3945).
pub const NAME_SAID_TO_A_PERSON_ENVIRONMENT: &str = "LAMP_NAME_SAID_TO_A_PERSON";

/// Where the worker reads whether a hearing of the lamp's name inside one
/// line's sound stays with that line, never counting for the one begun
/// after it (`on`, and nothing else;
/// [`worker::Heeding::keeping_the_name_in_its_line`]).
pub const NAME_STAYS_ENVIRONMENT: &str = "LAMP_NAME_STAYS_IN_ITS_LINE";

/// Where the worker reads whether the lamp's name at a line's start
/// ([`crate::attention::NAME_AT_THE_START_US`]) is dated from the line's usual
/// start, not its soft start's lead (`on`, and nothing else;
/// [`crate::attention::Attention::date_the_start_from_the_usual_start`],
/// #4226).
pub const NAME_START_USUAL_ENVIRONMENT: &str = "LAMP_NAME_START_USUAL";

/// Where the worker reads whether a line that begins with the lamp's name,
/// said to a face turned to the lamp, is always answered (#2191): `on`, and
/// nothing else. In a busy room the rule does not hold it, and the model's
/// `not_for_me` on it is not taken ([`Engine::answering_named_to_a_face`]).
pub const NAMED_ALWAYS_ENVIRONMENT: &str = "LAMP_NAMED_ALWAYS";

/// Where the worker reads whether a reply to a line told
/// `worker::CONVERSATION_ALONE` that opens with the words "not for me" is
/// taken as `not_for_me` ([`Engine::declining_in_words`]): `on`, and nothing
/// else.
pub const NOT_FOR_ME_WORDS_ENVIRONMENT: &str = "LAMP_NOT_FOR_ME_WORDS";

/// Where the worker reads whether the lamp's own answers to lines let out
/// only as the conversation going on keep that conversation going: `on`, and
/// nothing else, and they don't when another face than the caller's is
/// seen speaking the line (the first builder's 17:2x task and 01:02 on Queue
/// #1722, `Heeding::not_counting_own_answers`).
pub const GOING_ON_NOT_OWN_ENVIRONMENT: &str = "LAMP_GOING_ON_NOT_OWN";

/// Where the worker reads whether, in a busy room with nobody turned to the
/// lamp, the text shows a call only as the name set off as one or with the
/// name heard (#1712): `on`, and nothing else
/// (`crate::attention::Attention::narrow_the_call`).
pub const BUSY_CALLED_STRICT_ENVIRONMENT: &str = "LAMP_BUSY_CALLED_STRICT";

/// Where the worker reads whether a face turned to the lamp is a sign only
/// when it is not seen silent: `on`, and nothing else. The eyes' speaking
/// faces are read for it (`LAMP_EYES_SPEAKING` on lamp-eyes): when the only
/// faces turned to the lamp through a line read still, nobody is turned to
/// it (the first builder on Queue #1722, 21 Oct 14:31: on lamp-4ace someone
/// at the desk, turned to it and quiet, let 33 lines of two other people's
/// talk out; `Heeding::counting_only_turned_speakers`).
pub const TURNED_SPEAKING_ENVIRONMENT: &str = "LAMP_TURNED_SPEAKING";

/// Where the worker reads whether a face turned to the lamp lets a line out
/// only when it is seen speaking it, or the line is in the conversation's
/// voice: `on`, and nothing else. A face read and never seen speaking the
/// line is nobody turned to the lamp, and the line, held, is talk for the
/// room (lamp-52e6 on 24 Oct at 05:31: with a still face turned to it, two
/// other voices' desk talk was answered and the room never read busy;
/// `worker::Heeding::needing_the_turned_face_to_speak`).
pub const TURNED_FACE_SPEAKS_ENVIRONMENT: &str = "LAMP_TURNED_FACE_SPEAKS";

/// Where the worker reads whether two faces make the room busy only when
/// they stay: `on`, and nothing else. With `LAMP_NAME_OR_FOLLOW_UP`, two
/// faces in view make the room busy only when the eyes' newest look and at
/// least 3 of their looks in the last 2.5 s saw two, spanning 1.5 s or
/// more, not on one look (a passer-by, a misread, a photo frame;
/// `worker::FacesHeld`).
pub const TWO_FACES_HELD_ENVIRONMENT: &str = "LAMP_TWO_FACES_HELD";

/// Where the worker reads whether a line that cut the one before it short
/// is asked with that one's words when the same person said both: `on`,
/// and nothing else ([`Engine::joining_cut_words`]). On lamp-4ace on 21 Oct
/// a line's words were let go when the next one cut them short 31 times,
/// most in two-person scenes, where that is right (the first builder on
/// Queue #2602 at 17:22); a person who paused mid-request lost its first
/// half the same way (Eighth's long request).
pub const JOIN_CUT_WORDS_ENVIRONMENT: &str = "LAMP_JOIN_CUT_WORDS";

/// Where the worker reads whether a busy room ends sooner (the first
/// builder on Queue #1722, 04:00): `on`, and nothing else. A minute after
/// the last talk when the lamp has not answered since, and a minute after
/// its first answer since when it has (`room_told::RoomTold::end_busy_sooner`,
/// `room_told::RoomTold::end_busy_once_answered`).
pub const BUSY_ENDS_SOONER_ENVIRONMENT: &str = "LAMP_BUSY_ENDS_SOONER";

/// Where the worker reads whether, in a busy room, the one the lamp answered
/// goes on talking with it by their own voice or face for
/// `worker::SAME_SPEAKER_CARRIES_FOR` after each answer, past a follow-up's
/// 8 s (`LAMP_NAME_OR_FOLLOW_UP`) and talk past the lamp: `on`, and nothing
/// else (the owner's P1 on Queue #3126, `worker::Heeding::carrying_the_same_speaker`).
pub const SAME_SPEAKER_CARRIES_ENVIRONMENT: &str = "LAMP_SAME_SPEAKER_CARRIES";

/// Where the worker reads whether a line is in a known person's scope only
/// when their own face was seen speaking it, else the room's: `on`, and
/// nothing else, with `LAMP_SECOND_SPEAKING_FACE` ([`Engine::scoping_by_sight`],
/// `speaking_faces::not_theirs`). Off, only another face seen speaking it
/// narrows it (reviewer-three's P1 and P2 on #3435).
pub const PERSON_SCOPE_SEEN_ENVIRONMENT: &str = "LAMP_PERSON_SCOPE_SEEN";

/// What a `not_for_me` is answered in a turn named to a face
/// ([`NAMED_ALWAYS_ENVIRONMENT`]): the model is asked once more.
pub(crate) const NAMED_TO_A_FACE_ANSWER: &str =
    "Not taken: they called you by name while facing you, so this is said to you. Answer them.";

/// Where the worker reads whether a skill's answer with `"look": true`
/// (`skills/look-for-things`' `look_for`, #941) asks the coordinator for
/// the turn's full look, as on the Live route ([`Engine::looking_for_skills`]):
/// `on`, and nothing else.
pub const SKILL_LOOK_ENVIRONMENT: &str = "LAMP_SECOND_SKILL_LOOK";

/// The second route waits [`WRITTEN_BEGIN_WITHIN`] for the first round of a
/// turn the lamp wrote itself ([`Engine::waiting_for_written`]): `on`, and
/// nothing else (#1854's follow-up, the first builder at 18:28 on #1722).
pub const WRITTEN_WAIT_ENVIRONMENT: &str = "LAMP_SECOND_WRITTEN_WAIT";

/// Where the worker and the coordinator read whether a look's turn whose
/// answer called nothing is told so (`LookNoCall`), so that a skill's
/// search the model left after one look gets one note to go on
/// ([`Engine::noting_looks`], `coordinator::look_note`): `on`, and nothing
/// else. Only with [`SKILL_LOOK_ENVIRONMENT`].
pub const LOOK_NOTE_ENVIRONMENT: &str = "LAMP_SECOND_LOOK_NOTE";

/// Where the worker and the coordinator read whether a skill's search the
/// model leaves again after its note walks on by itself to the next place
/// (`coordinator::look_note`): the worker then tells a search's note turn
/// answered with nothing called too ([`Engine::walking_looks`]). `on`, and
/// nothing else. Only with [`LOOK_NOTE_ENVIRONMENT`].
pub const LOOK_WALK_ENVIRONMENT: &str = "LAMP_SECOND_LOOK_WALK";

/// The note after a round's call results, when that round said `said`
/// before its calls: the person heard it, so the next round says only what
/// the results add ([`Engine::saying_once`]).
pub(crate) fn said_once_note(said: &str) -> String {
    format!(
        "(For you to know, not to answer: before these calls you already said aloud \"{}\", and the person heard it. Do not say it again in other words or confirm twice. Say only what the results add that you have not said, such as the time it is set for or that it failed, in a few words, or nothing if they add nothing.)",
        said.trim()
    )
}

/// The line a round's calls are answered with when every one of them was
/// answered short (`LAMP_SHORT_DONE`, #2877): `told` holds what each call
/// was told, one for each. lamp-actions marks a light or volume change it
/// made with `"next"` and gives its own words in `"say"` ("Brighter.");
/// a call refused, failed, held, unchanged ("already") or anyone else's
/// carries no `"say"`, and then there is no line and the turn goes on as
/// without the switch. One line for the round: the calls' own when they
/// agree, [`SHORT_DONE_TOGETHER`] when they differ.
pub(crate) fn short_done_line(told: &[Value]) -> Option<&str> {
    let mut says = told.iter().map(|told| {
        told["next"]
            .is_string()
            .then(|| told["say"].as_str())
            .flatten()
            .filter(|say| say.chars().any(char::is_alphanumeric))
    });
    let first = says.next()??;
    let mut line = first;
    for say in says {
        if say? != first {
            line = SHORT_DONE_TOGETHER;
        }
    }
    Some(line)
}

/// The note after a round's calls all answered short (`LAMP_SHORT_DONE`,
/// #2877): the lamp answers the change itself with `line`, so the next
/// round adds only what else was asked, or nothing at all.
pub(crate) fn short_done_note(line: &str) -> String {
    format!(
        "(For you to know, not to answer: the lamp itself says \"{line}\" for this change, so say nothing about it. If they asked for anything else in the same words, answer only that. If they asked for nothing else, reply with nothing at all: no words.)"
    )
}
/// Whether a spoken turn's words were said to the lamp, judged before they
/// are answered: `(request, (began_us, ended_us), heard)` to why it is not
/// answered, or nothing. The window is the microphone's read time of the
/// request's first and last packets, so a line replayed to this route keeps
/// the time it was said. The worker's is the Live route's own rule
/// (`worker::Heeding`, `crate::attention`).
/// The rule's verdict on a spoken turn: its request, when its line was said,
/// what was heard, the request it goes on from (`continues`): the one
/// begun just before it, in the same session; and how long its sound began
/// before its usual start by a soft start's lead, in microseconds, when its
/// window begins with its own first sound, else 0 (#4226).
pub type Heed =
    Arc<dyn Fn(u64, (u64, u64), &str, Option<u64>, u64) -> Option<&'static str> + Send + Sync>;
/// Whether the same person said a line cut short and the line that cut it
/// ([`Engine::joining_cut_words`]).
#[derive(Clone, Debug, PartialEq)]
pub enum Joined {
    /// The same person, and why: the two lines' voices, or one face seen
    /// speaking both.
    Same(&'static str),
    /// No sign of a second person, and what was seen, in counts: at most
    /// one face in view, no other voice heard in the conversation, no two
    /// faces seen speaking. A line let out is asked with the cut words; a
    /// held one is not, since only the voices or one face tell whose its
    /// words are (owner's test on lamp-4ace on 22 Oct, 19:04-19:08: 5 of 28
    /// lines were a request's second half answered alone, with neither a
    /// voice print nor a speaking face to join them).
    NoSecond(String),
    /// A print still to come, and what is known without it, for once
    /// [`JOIN_WAIT`] is over.
    Coming(Box<Joined>),
    /// Two people, or nothing to tell, and which: the cut line's words go,
    /// as today, and the journal says why.
    Not(String),
}
/// Asked of the line cut short and the line that cut it, by request.
pub type SameSpeaker = Arc<dyn Fn(u64, u64) -> Joined + Send + Sync>;
/// Where a spoken turn's verdict is said (`voice/turn`, #1155): its
/// request, and whether it is answered or its answer held back.
pub type Verdicts = Arc<dyn Fn(u64, lamp_msgs::TurnVerdict) + Send + Sync>;
/// Told that `request` ended as a cut-in over the lamp's answer
/// (`ProviderInput::End { cut_in: true }`), before the rule judges it.
pub type CutIn = Arc<dyn Fn(u64) + Send + Sync>;
/// After the rule held `request`: why it was held as unclear, if it was, and
/// whether the lamp hums for it (a misheard cut-in, `LAMP_MISHEARD_HUM`), as
/// the Live worker tells the coordinator (`ProviderOutput::Unclear`, #926).
pub type AskAgain = Arc<dyn Fn(u64) -> Option<(&'static str, bool)> + Send + Sync>;
/// Whether the link is back: any service host takes a bare connection
/// (`link::any_answers`), for a line lost on the network ([`Engine::resending`]).
pub type LinkBack = Arc<dyn Fn() -> BoxFuture<'static, bool> + Send + Sync>;
/// A spoken line's sound as this route takes it, for the same-voice check
/// (`LAMP_SAME_VOICE_MODEL`, `worker::Heeding::hear`): its start, said over
/// the lamp's voice or not, its samples, and its end.
pub enum Heard<'a> {
    Began {
        over_the_lamps_voice: bool,
    },
    /// Begun over nothing of the lamp's but the last request's wait sound:
    /// in the clear, but not printed for the same-voice check.
    BeganOverTheWaitSound,
    Samples(&'a [i16]),
    Ended,
}
/// What hears each spoken line's sound, by its request.
pub type Hearing = Arc<dyn Fn(u64, Heard<'_>) + Send + Sync>;
/// Told each line the listener opened once it ends, by its request, and
/// when it was said: from its first sound read to its last, by the lamp's
/// monotonic clock ([`Engine::telling_lines_heard`]).
pub type LineHeard = Arc<dyn Fn(u64, (u64, u64)) + Send + Sync>;
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

/// What memory holds that may fit a spoken request's words ([`Engine::remembering`]),
/// asked with the request's lease when this route scopes context: its
/// admitted scope, narrowed or not, is the one searched.
pub type Remembered = Arc<
    dyn Fn(u64, &str, Option<&crate::memory_scope::ScopeLease>) -> Option<String> + Send + Sync,
>;
/// The note a spoken request earns when the lamp's name was heard as it
/// began (`worker::Heeding::telling_the_name`, `crate::called`): asked
/// after the request is judged, with its request.
pub type NameNote = Arc<dyn Fn(u64) -> Option<&'static str> + Send + Sync>;
/// Each of the staff a line's own words ask by name to do something, in
/// the order they are asked, or none (`LAMP_TASKS_NAMED_ASK`,
/// `crate::agent_tasks::named_ask`).
pub type NamedAsk = Arc<dyn Fn(&str) -> Vec<&'static str> + Send + Sync>;
/// The note a spoken request gets while a task this voice prepared waits
/// for the person's yes (`crate::agent_tasks::pending_note`): with its
/// request, to the note or nothing.
pub type TaskNote = Arc<dyn Fn(u64) -> Option<String> + Send + Sync>;
/// The note's section for a dated follow-up withheld now, to be cut from
/// what a chat is told ([`Engine::asking_after_events`]).
pub type EventWithheld = Arc<dyn Fn() -> Option<String> + Send + Sync>;
/// Told, by request, that the request's answer began to be heard.
pub type AirHeard = Arc<dyn Fn(u64) + Send + Sync>;
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
    /// Its words sent so far will have been played by this time, as if the
    /// audio plays as it is sent, each packet after the one before: a
    /// conversation goes on from the end of what it said, not from when its
    /// answer began (#1712, the first builder at 19:21). The first field is
    /// the request its words answer (`LAMP_GOING_ON_NOT_OWN`).
    Said(u64, u64),
    /// A line began right after a stop (its Start's `after_stop`): what the
    /// lamp was saying was stopped, and a conversation carried for the one
    /// it answered is over (`LAMP_SAME_SPEAKER_CARRIES`).
    Stopped,
}
/// Where the engine says what the lamp did.
pub type LampDid = Arc<dyn Fn(Did) + Send + Sync>;
/// Whether the lamp's name was heard since a time, by this process's clock
/// ([`Engine::knowing_the_name_heard`]).
pub type NameSounded = Arc<dyn Fn(u64) -> bool + Send + Sync>;
/// Whether lamp-name heard the name in a request's line, by its request
/// ([`Engine::knowing_the_name_fired`]).
pub type NameFired = Arc<dyn Fn(u64) -> bool + Send + Sync>;
/// The note a spoken request gets on whether it called the lamp by name
/// ([`Engine::noting_whether_called`]): with its request and words.
pub type CallNote = Arc<dyn Fn(u64, &str) -> Option<&'static str> + Send + Sync>;
/// Told a spoken request's words once they are heard and it is to be
/// answered, before its chat begins ([`Engine::hearing_words`]).
pub type WordsHeard = Arc<dyn Fn(u64, &str) + Send + Sync>;
/// Whether a call, by its request and name, is held as not said to the
/// lamp ([`Engine::holding_calls`]).
pub type CallHold = Arc<dyn Fn(u64, &str) -> bool + Send + Sync>;
/// Whether a call of this name goes to the skills, whose answer alone can
/// ask for a look ([`Engine::looking_for_skills`]).
pub type SkillCall = Arc<dyn Fn(&str) -> bool + Send + Sync>;
/// What the Policy makes of a call in a spoken turn
/// ([`Engine::holding_for_the_policy`], `crate::policy_holds`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policed {
    /// Run it, as without the switch.
    Run,
    /// Not run: the Policy judged the turn not for the lamp.
    Held,
}
/// The Policy's gate for a call, by its request and name: it waits for the
/// turn's decision, at most `crate::policy_holds::WAIT`.
pub type PolicyGate = Arc<dyn Fn(u64, String) -> BoxFuture<'static, Policed> + Send + Sync>;

/// Told as a spoken line begins, with the scope its turn is told in
/// ([`NoteAhead::begin`]).
pub type NoteBegin = Arc<dyn Fn(Option<&crate::memory_scope::ScopeLease>) + Send + Sync>;
/// Awaited just before a turn is told, with the scope it is told in
/// ([`NoteAhead::ready`]).
pub type NoteReady =
    Arc<dyn Fn(Option<crate::memory_scope::ScopeLease>) -> BoxFuture<'static, ()> + Send + Sync>;

/// The memory's note for a run of turns, asked as a spoken line begins and
/// read off the worker's loop ([`Engine::asking_the_note_ahead`]). Each is
/// given the scope the turn is told in ([`told`]): its lease when this route
/// scopes context, none otherwise.
#[derive(Clone)]
pub struct NoteAhead {
    /// Told as a spoken line begins: the note is asked then, on a thread of
    /// its own, if it is due. It never waits.
    pub begin: NoteBegin,
    /// Awaited just before a turn is told: done once the note asked for its
    /// scope is in, or once what is left of the note's 300 ms from that ask
    /// has passed. Building the turn then reads nothing.
    pub ready: NoteReady,
    /// Told when the camera or the microphone closes or opens: a note asked
    /// before is never told after.
    pub forget: Arc<dyn Fn() + Send + Sync>,
}

/// What every turn task shares.
struct Shared {
    services: Arc<dyn Services>,
    tools: ToolRunner,
    scoped_tools: Option<ScopedToolRunner>,
    requested_audio: Option<(String, RequestedAudioRunner)>,
    system: System,
    scoped_system: Option<ScopedSystem>,
    /// Without scoped context, the turn's `system` given its line's lease,
    /// read only for the typed name (#2602, [`Engine::reading_the_lease_for_the_name`]).
    lease_system: Option<ScopedSystem>,
    clock: Clock,
    /// When answer audio was last sent.
    answered_at: Mutex<Option<Instant>>,
    /// When the words sent so far will have been played ([`Did::Said`]).
    said_until_us: AtomicU64,
    /// The newest answer's pieces as sent, its request first: each one's
    /// words (none for a wait sound) and samples, so a cut answer is kept
    /// as what was heard of it (`LAMP_SAID_AS_HEARD`, [`Engine::said_as_heard`]).
    said: Mutex<(
        u64,
        Vec<crate::said_as_heard::Piece>,
        crate::not_speech::CallsLeftOut,
    )>,
    /// Audio packets sent by this process.
    sequence: AtomicU64,
    out: mpsc::Sender<ProviderOutput>,
    heed: Option<Heed>,
    /// Where each judged turn's verdict is said, when it is.
    verdicts: Option<Verdicts>,
    /// [`Engine::asking_again`]: told of cut-ins, and asked after a hold.
    ask_again: Option<(CutIn, AskAgain)>,
    /// When a cut-in was last held as misheard with the lamp's hum: the
    /// first request asked within `misheard_hum::NOTE_WITHIN_US` of it is
    /// told so ([`crate::misheard_hum::NOTE`]), once.
    misheard_at_us: Mutex<Option<u64>>,
    /// [`Engine::holding_answers`]: a spoken turn answered only with a
    /// holding phrase sends `HoldingAnswer`.
    holding_answer: bool,
    /// [`Engine::noting_looks`]: a look's turn whose answer called nothing
    /// sends `LookNoCall`.
    note_looks: bool,
    /// [`Engine::walking_looks`]: a search's own written turn that is no
    /// look (its note's) answered with nothing called sends `LookNoCall`
    /// too.
    walk_looks: bool,
    /// [`Engine::saying_once`]: a round that said words before its calls
    /// tells the next one so ([`said_once_note`]).
    said_once: bool,
    /// [`Engine::declining_in_words`]: a reply to a line told
    /// `worker::CONVERSATION_ALONE` that opens with "not for me" is
    /// `not_for_me`.
    declines_in_words: bool,
    /// [`Engine::speaking_tags`]: each piece of a reply is cut into what
    /// the voice is given and the lamp's words; `Some(true)` when the voice
    /// reads tags.
    speech_tags: Option<bool>,
    /// [`Engine::answering_short`] (#2877).
    short_done: bool,
    /// [`Engine::asking_again_for_no_words`] (#3704).
    no_words_again: bool,
    /// The lamp's own written turns may begin as late as
    /// [`WRITTEN_BEGIN_WITHIN`] ([`Engine::waiting_for_written`]).
    written_wait: bool,
    /// [`Engine::resending`]: a spoken line whose chat was lost on the
    /// network is asked once more when the link is back.
    resend: Option<LinkBack>,
    hearing: Option<Hearing>,
    /// Told each line's span once it ends, words or none
    /// ([`Engine::telling_lines_heard`]).
    line_heard: Option<LineHeard>,
    asked: Option<Asked>,
    /// The sounds made while a first round is awaited, when switched on.
    cues: Option<Arc<Cues>>,
    /// When the wait sound comes ([`CUE_AFTER`], or [`CUE_AFTER_ENVIRONMENT`]'s).
    cue_after: Duration,
    /// [`DOWN_LINE`], when switched on.
    down: Option<Arc<DownLine>>,
    /// A stop said over the lamp's answer is answered with
    /// [`crate::spoken_stop::LINE`], not asked of the model, its clip kept
    /// once rendered (`LAMP_SPOKEN_STOP_ENDS`, #3985).
    spoken_stop_ends: bool,
    stop_line: Mutex<Option<Vec<i16>>>,
    /// A turn after a quiet spell opens with [`ANNOUNCEMENT`]: not when
    /// this route is the lamp's only voice (`LAMP_VOICE_ROUTE=second`).
    announce: bool,
    /// The words last heard, with their request: a request that goes on
    /// from one given up after an early end begins with them.
    heard_last: Mutex<Option<(u64, String)>>,
    /// The words of the latest answer sent to the voice so far, and its
    /// request: for the journal's count of a line's words that were the
    /// lamp's own ([`shared_words`]), never its words.
    lamp_said: Mutex<(u64, String)>,
    /// The stop to be told (`after_stop`), with the request it is for.
    stop_untold: Mutex<Option<StopToTell>>,
    /// A held line is asked for the calls done though it is held
    /// ([`Engine::doing_though_held`]).
    though_held: bool,
    /// With `LAMP_STAFF_NOTE=on`: the note for words that name the staff.
    staff: Option<StaffNote>,
    /// What memory holds that may fit a spoken request's words
    /// ([`Engine::remembering`]).
    remembered: Option<Remembered>,
    /// With `LAMP_MEMORY_EVENTS`: the line a session's first turn, a
    /// greeting alone, is given for a dated follow-up
    /// ([`Engine::asking_after_events`], #2805, #2602 22 Oct 22:17).
    event_turn: Option<Remembered>,
    /// The note's section to cut for a withheld dated follow-up
    /// ([`Engine::asking_after_events`]).
    event_withheld: Option<EventWithheld>,
    /// The memory's note asked as a spoken line begins
    /// ([`Engine::asking_the_note_ahead`]).
    note_ahead: Option<NoteAhead>,
    /// With `LAMP_NAME_NOTE=on`: the note for a line the lamp's name began.
    name_note: Option<NameNote>,
    /// The note for a line let out only as the conversation going on.
    conversation_alone: Option<NameNote>,
    /// With `LAMP_TASKS_NAMED_ASK=on`: whom a line's words ask by name
    /// ([`Engine::asking_by_name`]).
    named_ask: Option<NamedAsk>,
    /// A test's copy of each named-ask journal line ([`Shared::named_ask_said`]).
    #[cfg(test)]
    named_ask_journal: Option<Arc<Mutex<Vec<String>>>>,
    /// Whether a request was judged in a busy room ([`Engine::judging_busy`]):
    /// a line that may follow on from the lamp is not told so there.
    busy: Option<Busy>,
    /// Whether a request began with the lamp's name said to a face turned
    /// to the lamp ([`Engine::answering_named_to_a_face`]).
    named_to_a_face: Option<Busy>,
    /// The Policy's decision for a line that waits for it
    /// ([`Engine::asking_the_policy`], [`WAITS_FOR_POLICY`]).
    policy_now: Option<PolicyNow>,
    /// The note on whether a line called the lamp by name.
    call_note: Option<CallNote>,
    /// Whether the lamp's name was heard since a time.
    name_sounded: Option<NameSounded>,
    /// Whether lamp-name heard the name in a request's line (#3504 N4).
    name_fired: Option<NameFired>,
    /// With the tasks part: the note that a prepared task waits for a yes.
    task_waiting: Option<TaskNote>,
    /// With `LAMP_AIR_FACTS=on`: the air's note, when it is due, and what
    /// is told once that request's answer begins to be heard.
    air: Option<(TaskNote, AirHeard)>,
    /// With `LAMP_SECOND_SEEMS=on`: how a known person in view seems from
    /// their face, when it is due, and what is told once that request's
    /// answer begins to be heard.
    seems: Option<(TaskNote, AirHeard)>,
    /// What the lamp did, told to the rule ([`Engine::telling_what_it_did`]).
    did: Option<LampDid>,
    /// Calls held as not said to the lamp ([`Engine::holding_calls`]).
    hold: Option<CallHold>,
    /// Calls that wait for the Policy's decision ([`Engine::holding_for_the_policy`]).
    policy: Option<PolicyGate>,
    /// Told a spoken request's words before its chat ([`Engine::hearing_words`]).
    words_heard: Option<WordsHeard>,
    /// The model looks by calling [`LOOK_CALL`] ([`Engine::looking_by_call`]).
    look_by_call: bool,
    /// A skill's answer with `"look": true` asks for the turn's look
    /// ([`Engine::looking_for_skills`]): which calls go to the skills.
    skill_look: Option<SkillCall>,
    /// A push on the body during a skill's search ends it for this route
    /// too ([`Engine::stopping_searches_by_hand`], #2904).
    search_pushed: Option<Arc<Mutex<crate::search_pushed::SearchPushed>>>,
    /// A spoken turn's chat begins on the line's early words
    /// ([`Engine::preparing`]).
    prepare: bool,
    /// The coordinator is told whether a line's words so far end unfinished
    /// ([`Engine::telling_unfinished_words`]).
    unfinished_words: bool,
    /// Each turn's journal line on its tools ([`Engine::noting_turn_tools`],
    /// `LAMP_MEMORY_REPLACES`, #2510).
    note_tools: bool,
    /// A test's copy of each such line ([`Shared::turn_tools_said`]).
    #[cfg(test)]
    tools_journal: Option<Arc<Mutex<Vec<String>>>>,
    /// A listener is kept connected for the next line
    /// ([`Engine::keeping_a_listener_ready`]): the one kept, and how many
    /// have been opened, so an expiry takes only its own.
    keep_listener: bool,
    /// A line whose words end before a name waits for it
    /// ([`Engine::waiting_for_a_name`]).
    open_name: bool,
    /// A line that was only the lamp's name waits for the line after it,
    /// which is then said by it ([`Engine::carrying_the_name`]).
    name_carries: bool,
    /// That line, while its answer waits ([`NameWait`]).
    name_waits: Mutex<Option<NameWait>>,
    /// The name whose words wait as words cut short because the line after
    /// it was not shown to be its person's ([`said_by_the_name`]): they are
    /// never joined to that line ([`name_refused`]).
    name_refused: Mutex<Option<u64>>,
    /// Whether one person said the name and the line after it, and where
    /// a line not judged yet is kept for that ([`Engine::telling_one_speaker`]).
    one_speaker: Option<(SameSpeaker, LineKept)>,
    /// lamp-name's near hearings, for a line's first word
    /// ([`Engine::hearing_the_name_near`]).
    name_near: Option<NameNear>,
    /// A line of nothing but the lamp's own words heard back is held, and
    /// its own words at a line's start or end are taken out
    /// ([`Engine::holding_own_echo`]).
    own_echo: bool,
    spare: Mutex<Option<Kept>>,
    spares: AtomicU64,
    /// The privacy generation one was opened under before any line
    /// ([`Engine::listening`]); `None` again when listening stops.
    warmed: Mutex<Option<u64>>,
    /// The request begun last, with its session, for a look asked by a call.
    begun: Mutex<Option<(u64, u64)>>,
    /// The request whose only sound so far is its wait sound
    /// (`LAMP_SECOND_WAIT_CUE`): a line begun over it is begun in the clear
    /// ([`Shared::only_its_wait_sound`]).
    wait_sound_only: Mutex<Option<u64>>,
    /// What a skill's answer said to do with the picture of the look it
    /// asked ([`picture_note`]), with the session the look was asked for:
    /// given once to that session's next look turn, after its question, and
    /// let go as any other turn begins.
    look_note: Mutex<Option<(u64, String)>>,
    /// The spoken request being answered and its words, until any of its
    /// answer is heard or its turn ends.
    in_flight: Mutex<Option<Carried>>,
    /// The last request whose answer was heard.
    answered_request: Mutex<Option<u64>>,
    /// A spoken request's words whose turn ended before any of its answer
    /// was heard (a newer line began): answered in the next line's place
    /// if that line is held ([`CARRY_WITHIN`], #1712).
    cut: Mutex<Option<Carried>>,
    /// The request last held, its words not answered: never chained into a
    /// later line's ([`Engine::chain_heard`]).
    held_last: Mutex<Option<u64>>,
    /// The newest request whose words were put in flight: asked once, so
    /// never chained again from what was heard ([`Engine::chain_heard`]).
    asked_last: Mutex<Option<u64>>,
    /// Whether one person said a line cut short and the line that cut it
    /// ([`Engine::joining_cut_words`]).
    same_speaker: Option<SameSpeaker>,
    /// Takes a request whose answer was never played out of the thread, when
    /// its words are joined into a later line or answered in a held line's
    /// place ([`Engine::unsaying_joined`]).
    unsay_joined: Option<Arc<dyn Fn(u64) + Send + Sync>>,
    /// The answer's first piece goes once this many words have come
    /// ([`Engine::cutting_first_words`]).
    first_words: Option<usize>,
    /// Seeing someone speak (`LAMP_SECOND_SPEAKING_FACE=on`,
    /// [`crate::speaking_faces`]): the eyes' looks and their known file.
    speaking: Option<(
        Arc<crate::speaking_faces::SpeakingFaces>,
        std::path::PathBuf,
    )>,
    /// A forget the model called unasked, held for that person's yes
    /// ([`HeldForget`]).
    held_forget: Mutex<HeldForgets>,
    /// Where the eyes read speaking (`LAMP_EYES_SPEAKING=on`): the eyes'
    /// looks and their known file, and a held forget's yes counts only
    /// from the held person's face seen saying it (#2377).
    forget_sight: Option<(
        Arc<crate::speaking_faces::SpeakingFaces>,
        std::path::PathBuf,
    )>,
    remember_speaker: bool,
    enrollment_sight: Option<Arc<crate::speaking_faces::SpeakingFaces>>,
    /// A line is a known person's only when their own face was seen
    /// speaking it (`LAMP_PERSON_SCOPE_SEEN`, [`Engine::scoping_by_sight`]).
    scope_seen: bool,
}

/// Who the eyes saw speaking a line from `begun` to `finishing`, and the
/// known person with their face, as `speaking_faces::not_theirs` reads
/// them. With `seen_only` (`LAMP_PERSON_SCOPE_SEEN`) each id is kept with
/// the eyes' boot, the known face's too ([`SpeakingFaces::known_with_boot`]),
/// as the held forget's sight is (#3037): after the eyes start again a
/// reused id is someone else, and a known face with no boot is nobody's.
/// Without it, the bare ids, as before.
///
/// [`SpeakingFaces::known_with_boot`]: crate::speaking_faces::SpeakingFaces::known_with_boot
async fn line_sight(
    eyes: &Arc<crate::speaking_faces::SpeakingFaces>,
    known: &std::path::Path,
    begun: Instant,
    finishing: Instant,
    seen_only: bool,
) -> (
    crate::speaking_faces::Speakers,
    Option<(String, String)>,
    bool,
) {
    let known = known.to_owned();
    if seen_only {
        let speakers = eyes.speakers_with_boot(begun, finishing);
        let eyes_now = eyes.clone();
        let pair = tokio::task::spawn_blocking(move || eyes_now.known_with_boot(&known))
            .await
            .ok()
            .flatten();
        // The known face the only one in view through the line (lamp-3e83).
        let alone = pair
            .as_ref()
            .is_some_and(|(_, face)| eyes.alone_in_view(face, begun, finishing));
        (speakers, pair, alone)
    } else {
        let speakers = eyes.speakers(begun, finishing);
        let pair =
            tokio::task::spawn_blocking(move || crate::speaking_faces::known_with_face(&known))
                .await
                .ok()
                .flatten();
        (speakers, pair, false)
    }
}

/// How long after its end a line whose words end before a name waits for
/// the name (`LAMP_SECOND_OPEN_NAME`, [`Engine::waiting_for_a_name`]), and
/// how soon a sound must begin after it to go on from it
/// (`coordinator::open_name`). Its own, longer than [`CARRY_AFTER_END`]: a
/// person who says "my name is" and stops is about to say it, and may think
/// a moment. On lamp-4ace at 05:53 the name came about 2.05 s after the
/// line's end, past 1.5 s, and the body's move let it go (the first
/// builder on #1722 at 06:00). The cost falls only on lines that end so.
pub const OPEN_NAME_WITHIN: Duration = Duration::from_millis(2_500);

/// How long after its end a line that was only the lamp's name waits for
/// the line after it (`LAMP_SECOND_NAME_CARRIES`, [`Engine::carrying_the_name`]).
/// A line begun in it is said by that name, and the name's own request is
/// dropped; with none, the name is answered as a call. As long as
/// [`OPEN_NAME_WITHIN`], and past [`CARRY_AFTER_END`]'s 1.5 s, which the
/// kept sound needs. In a played round on lamp-4ace on 22 Oct, 4 of 38
/// requests said with a pause after "Lamp," were lost: the name's piece
/// (heard as "Life." or "Louvre,", or as the name with more) was held, as a
/// short sound or as said to nobody, and each piece after it as said to
/// nobody.
pub const NAME_CARRIES_FOR: Duration = Duration::from_millis(2_500);

/// What a line waiting as the lamp's name said: its words, as it would
/// have been asked, and whether the coordinator ended it as only the name
/// (`Called`). They go before the line said by it when they begin with the
/// name (`LAMP_SECOND_NAME_CARRIES`).
type NameSaid = (String, bool);

/// The words of a line that is the lamp's name, for the line begun while
/// it waits.
#[derive(Debug)]
enum NameWords {
    Known(NameSaid),
    /// The coordinator's call whose speech to text was still working when
    /// the next line began: its words come here once they are back
    /// ([`Marks::words_back`]), none if it failed.
    Coming(tokio::sync::oneshot::Receiver<Option<NameSaid>>),
}

/// A line that is the lamp's name, while its answer waits for the line
/// after it ([`NAME_CARRIES_FOR`], `LAMP_SECOND_NAME_CARRIES`).
#[derive(Debug)]
struct NameWait {
    request: u64,
    session: u64,
    privacy: u64,
    words: NameWords,
    /// When its wait is over, by the runtime's clock: 2.5 s from its end.
    until: tokio::time::Instant,
    /// When its first sound was read, by this process's clock: the rule's
    /// window for the line said by it begins here, so the name heard in it
    /// counts for that line.
    said_from: u64,
}

/// The name a line is said by, once one person is shown to have said both
/// ([`said_by_the_name`]).
struct SaidBy {
    said_from: u64,
    words: String,
    called: bool,
}

/// lamp-name's best near hearing (`name/near`, `LAMP_NAME_NEAR`) around a
/// line's start, by the line's window (its first and last sound's read
/// times; `worker::Heeding::near_name`), and whether a face turned to the
/// lamp said it (`worker::Heeding::turned_to_the_lamp`).
pub type NameNear = Arc<dyn Fn((u64, u64)) -> (Option<f32>, bool) + Send + Sync>;

/// The least near score that, beside a first word set off and sounding
/// like the name, makes it the name (`LAMP_NAME_NEAR`): lamp-name's own
/// near bar. In the clean files of 22 Oct's played lines, a line with no
/// name reached it in 1 of 24, mid-line.
pub const NAME_NEAR_LEAST: f32 = 0.5;

/// Words the speech to text wrote for "Lamp" at a line's start on
/// lamp-4ace in 22 Oct's played rounds that the shape in
/// [`sounds_like_the_name`] misses (`LAMP_NAME_NEAR`).
const HEARD_FOR_THE_NAME: [&str; 5] = ["louvre", "month", "life", "light", "length"];

/// People's names of the name's shape, that a colleague may call at a desk
/// ("Lynn, pass me that"), English and Vietnamese: never the lamp's
/// (`LAMP_NAME_NEAR`, the first builder's P3 on 3d0b34d50).
const NAMES_OF_ITS_SHAPE: [&str; 15] = [
    "lynn", "lyn", "lin", "linn", "lane", "lan", "len", "lam", "lim", "liam", "loan", "luan",
    "lien", "leanne", "lorne",
];

/// How a word sounds beside the lamp's name (`LAMP_NAME_NEAR`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LikeTheName {
    /// One of [`HEARD_FOR_THE_NAME`]: the name as the speech to text wrote
    /// it on lamp-4ace, no word a colleague is called by.
    AsHeard,
    /// Only its shape: one syllable from an L to an M, P or N ("Lump",
    /// "Loom", "Leon"), which people's names have too, so it counts only
    /// with a face turned to the lamp ([`named_near`]).
    Shape,
    Not,
}

/// How `word` sounds beside the lamp's name: as it was heard for it
/// ([`HEARD_FOR_THE_NAME`]), or by its shape, one syllable from an L to an
/// M, P or N ("-mb" as "Lamb" is an M), never a person's name of that
/// shape on [`NAMES_OF_ITS_SHAPE`], compared after a silent "e" is let go
/// ("Lynne", "Lane"). Never a word with an apostrophe.
fn like_the_name(word: &str) -> LikeTheName {
    let word = word.to_ascii_lowercase();
    if !word.chars().all(|c| c.is_ascii_alphabetic()) || word.is_empty() {
        return LikeTheName::Not;
    }
    if HEARD_FOR_THE_NAME.contains(&word.as_str()) {
        return LikeTheName::AsHeard;
    }
    let spoken = spoken_form(&word);
    // Each name as it is spoken too, so "Leanne" is "leann" on both sides
    // (reviewer-three's P3 on 1600b0a69).
    if NAMES_OF_ITS_SHAPE
        .iter()
        .any(|name| spoken_form(name) == spoken)
    {
        return LikeTheName::Not;
    }
    let vowel = |c: char| "aeiouy".contains(c);
    let syllables = spoken
        .chars()
        .zip(std::iter::once(' ').chain(spoken.chars()))
        .filter(|(c, before)| vowel(*c) && !vowel(*before))
        .count();
    if spoken.starts_with('l') && syllables == 1 && spoken.ends_with(['m', 'p', 'n']) {
        LikeTheName::Shape
    } else {
        LikeTheName::Not
    }
}

/// A lower-case word as it is spoken, for its shape: a silent "e" after a
/// consonant let go ("line", "lane": it ends no syllable), and "-mb" as
/// an M ("Lamb").
fn spoken_form(word: &str) -> String {
    let vowel = |c: char| "aeiouy".contains(c);
    let spoken = match word.strip_suffix('e') {
        Some(stem) if stem.chars().last().is_some_and(|c| !vowel(c)) => stem,
        _ => word,
    };
    spoken
        .strip_suffix("mb")
        .map_or(spoken.to_owned(), |stem| format!("{stem}m"))
}

/// Whether a word sounds like the lamp's name at all ([`like_the_name`]).
#[cfg(test)]
fn sounds_like_the_name(word: &str) -> bool {
    like_the_name(word) != LikeTheName::Not
}

/// Where a line's first word lies, when it is set off as a name is: past
/// a greeting at most ("Hey"), followed by a comma or a stop, or the whole
/// line, its own piece. Its byte range in `text`.
pub(crate) fn first_word_set_off(text: &str) -> Option<(usize, usize)> {
    let word_at = |from: usize| {
        let start = from + text[from..].find(|c: char| c.is_alphanumeric() || c == '\'')?;
        let len = text[start..]
            .find(|c: char| !(c.is_alphanumeric() || c == '\''))
            .unwrap_or(text.len() - start);
        Some((start, start + len))
    };
    let (mut start, mut end) = word_at(0)?;
    if ["hey", "ok", "okay", "hi"]
        .iter()
        .any(|greeting| text[start..end].eq_ignore_ascii_case(greeting))
    {
        (start, end) = word_at(end)?;
    }
    let after = text[end..].trim_start();
    (after.is_empty() || after.starts_with([',', '.', '!', '?', ';', ':'])).then_some((start, end))
}

/// `LAMP_NAME_NEAR`: `text`'s first word, set off and sounding like the
/// lamp's name, is the name when lamp-name heard it near
/// ([`NAME_NEAR_LEAST`] or more) within 1.3 s of the line's first sound
/// (the first of `window`), and, when only its shape is the name's
/// ([`LikeTheName::Shape`]: "Leon, pass me that" is a colleague called),
/// a face turned to the lamp said it (reviewer-three's P2 on 194accd24;
/// with speaking faces read, one seen speaking, the P3 on 1600b0a69). It is
/// put in the word's place, so the rule and the model read the call. Any
/// sign missing changes nothing, and the journal says which held, counts
/// only.
fn named_near(request: u64, text: String, window: (u64, u64), near: &NameNear) -> String {
    let Some((start, end)) = first_word_set_off(&text) else {
        return text;
    };
    let word = &text[start..end];
    if word.eq_ignore_ascii_case(net::key_terms::NAME) || word.eq_ignore_ascii_case("lamb") {
        return text;
    }
    let like = like_the_name(word);
    let (heard, turned) = near(window);
    let near_enough = heard.filter(|score| *score >= NAME_NEAR_LEAST);
    let named = match (like, near_enough) {
        (LikeTheName::AsHeard, Some(score)) => Some(format!(
            "its first word, set off, is one written for the lamp's name, and lamp-name heard it near, at {score:.2}, by the line's start"
        )),
        (LikeTheName::Shape, Some(score)) if turned => Some(format!(
            "its first word, set off, has the lamp's name's shape, lamp-name heard it near, at {score:.2}, by the line's start, and a face is turned to the lamp"
        )),
        (LikeTheName::Shape, Some(score)) => {
            eprintln!(
                "lamp-voice second: request {request}: its first word, set off, has the lamp's name's shape, and lamp-name heard it near, at {score:.2}, but no face is turned to the lamp: not the name (LAMP_NAME_NEAR)"
            );
            None
        }
        (LikeTheName::AsHeard | LikeTheName::Shape, None) => {
            eprintln!(
                "lamp-voice second: request {request}: its first word, set off, sounds like the lamp's name, but lamp-name heard it near by the line's start {}: not the name (LAMP_NAME_NEAR)",
                heard.map_or("not at all".to_owned(), |score| format!(
                    "only at {score:.2}"
                ))
            );
            None
        }
        (LikeTheName::Not, Some(score)) => {
            eprintln!(
                "lamp-voice second: request {request}: lamp-name heard the name near, at {score:.2}, by the line's start, but its first word, set off, doesn't sound like it: not the name (LAMP_NAME_NEAR)"
            );
            None
        }
        (LikeTheName::Not, None) => None,
    };
    match named {
        Some(signs) => {
            eprintln!(
                "lamp-voice second: request {request}: {signs}: it is the name (LAMP_NAME_NEAR)"
            );
            format!("{}{}{}", &text[..start], net::key_terms::NAME, &text[end..])
        }
        None => text,
    }
}

/// Words a line often opens with, set off, that are said, not a wake heard
/// as another word ([`wake_heard_as`]): "Yes, please.", "Well, …", "Dạ, …".
const OPENS_SET_OFF: &[&str] = &[
    "yes",
    "yeah",
    "yep",
    "no",
    "nope",
    "so",
    "well",
    "please",
    "thanks",
    "thank",
    "sorry",
    "actually",
    "wait",
    "now",
    "and",
    "but",
    "also",
    "right",
    "sure",
    "alright",
    "fine",
    "good",
    "great",
    "cool",
    "nice",
    "hmm",
    "hm",
    "mm",
    "um",
    "umm",
    "uh",
    "oh",
    "hello",
    "listen",
    "look",
    "again",
    "then",
    "anyway",
    "v\u{e2}ng",
    "d\u{1ea1}",
    "\u{1eeb}",
    "kh\u{f4}ng",
    "nh\u{e9}",
    "\u{e0}",
    "\u{1a1}i",
];

/// #3504 finding N4: on a line lamp-name heard the lamp's name in (the
/// voice's own sign, [`Engine::knowing_the_name_fired`], or its name note
/// given), the first word past a greeting, set off as a name is
/// ([`first_word_set_off`]: "Ramon. …", "Edeland, …", "Hey, mom. …"), is
/// the lamp's name as speech to text wrote it, not a person's: when it is
/// not "Lamp" itself (the name, or `LAMP_NAME_NEAR`'s word put in its
/// place), is none of [`OPENS_SET_OFF`], and more is said after it. Text
/// alone can't tell the misheard name from a person's (Ramon, Ilan and Elan
/// are names too), so this is read only on such a line. The word, and the
/// line after it. N4b (lamp-06c1, 28 Oct): a line may open with one
/// greeting of its own before it, set off too ("Hello. Gaswa, I changed
/// jobs."), which [`GREETINGS`] passes over.
pub(crate) fn wake_heard_as(text: &str) -> Option<(&str, &str)> {
    let text = past_greeting(text);
    let (start, end) = first_word_set_off(text)?;
    let word = &text[start..end];
    let lower = unicode_normalization::UnicodeNormalization::nfc(word)
        .collect::<String>()
        .to_lowercase();
    let rest = text[end..].trim_start_matches(|c: char| !c.is_alphanumeric());
    let named = lower == "lamp" || lower == "lamb";
    let common = OPENS_SET_OFF.contains(&lower.as_str()) || NOT_A_NAME.contains(&lower.as_str());
    // A line that goes on to call the lamp by its name as itself had its
    // first word heard right, a person's name: "Hello, Anna. Lamp, …"
    // (Reviewer-three's P2 on #4069), and past "Hey" or "Hi" too ("Hey,
    // Anna. Lamp, …", the Reviewer-lead's P3 on #4078). Only the name said
    // as an address counts, never the noun ("Hello. Gaswa, turn off the
    // lamp.", the Reviewer-lead's P2 on #4078).
    (!rest.is_empty() && !named && !common && !addresses_the_name(rest)).then_some((word, rest))
}

/// Greetings a line may open with, set off by a comma or a stop, before
/// the lamp's name as heard ([`wake_heard_as`]), NFC lower case. "Hey",
/// "hi", "ok" and "okay" are passed over by [`first_word_set_off`] itself,
/// as lamp-voice reads them today.
const GREETINGS: &[&str] = &[
    "hello",
    "good morning",
    "good afternoon",
    "good evening",
    "xin ch\u{e0}o",
    "ch\u{e0}o",
    "alo",
];

/// `text` past one greeting of [`GREETINGS`] set off by a comma or a stop
/// ("Hello. …", "Good morning, …"), with what follows it; `text` itself
/// when it opens with none, or the greeting is all there is. The cut is at
/// a character boundary of `text` whose NFC lower-case prefix is the
/// greeting, so a decomposed accent ("chào" written as "cha" and a
/// combining grave) is never split (the Reviewer-lead's P3 on #4069).
fn past_greeting(text: &str) -> &str {
    let lead = text.trim_start();
    for greeting in GREETINGS {
        let Some(cut) = lead
            .char_indices()
            .map(|(at, _)| at)
            .chain([lead.len()])
            .skip(1)
            .take(2 * greeting.chars().count() + 2)
            .find(|&at| {
                unicode_normalization::UnicodeNormalization::nfc(&lead[..at])
                    .collect::<String>()
                    .to_lowercase()
                    == *greeting
                    && !lead[at..]
                        .starts_with(|c: char| unicode_normalization::char::is_combining_mark(c))
            })
        else {
            continue;
        };
        if !lead[cut..].starts_with([',', '.', '!', ';', ':']) {
            continue;
        }
        let rest = lead[cut..].trim_start_matches(|c: char| !c.is_alphanumeric());
        if !rest.is_empty() {
            return rest;
        }
    }
    text
}

/// What follows a line's opening call of the lamp by its name as itself,
/// past one greeting at most ("Hey, Lamp. …", "Lamp, …", "Hello. Hey
/// Lamp, …"), set off as [`first_word_set_off`] reads it; `None` when the
/// line opens with no such call, or the call is all there is.
fn name_call_rest(text: &str) -> Option<&str> {
    let text = past_greeting(text);
    let (start, end) = first_word_set_off(text)?;
    let word = &text[start..end];
    if !(word.eq_ignore_ascii_case("lamp") || word.eq_ignore_ascii_case("lamb")) {
        return None;
    }
    let rest = text[end..].trim_start_matches(|c: char| !c.is_alphanumeric());
    (!rest.is_empty()).then_some(rest)
}

/// Whether `text` calls the lamp by its name as itself, "Lamp" or "Lamb",
/// as an address: the word opens a sentence or clause (at the start, or
/// after a stop or a comma), or follows a greeting ("Hey Lamp", "Hey,
/// Lamp"). It needn't be set off itself, since speech to text often drops
/// that comma ("Lamp what's the weather?", "Lamp ơi, …"; both readers' P2
/// on #4274). The noun is never the call, as a word before it means it
/// opens no clause: "turn off the lamp.", "dim my lamp, please", "the lamp
/// is too bright", "lamps", "the lamp's shade". Named limit: a list
/// ("desk, lamp, and chair") reads as a call.
fn addresses_the_name(text: &str) -> bool {
    let words: Vec<(usize, usize)> = text
        .char_indices()
        .filter(|&(at, c)| {
            (c.is_alphanumeric() || c == '\'')
                && !text[..at].ends_with(|p: char| p.is_alphanumeric() || p == '\'')
        })
        .map(|(at, _)| {
            let len = text[at..]
                .find(|c: char| !(c.is_alphanumeric() || c == '\''))
                .unwrap_or(text.len() - at);
            (at, at + len)
        })
        .collect();
    words.iter().enumerate().any(|(i, &(start, end))| {
        let word = &text[start..end];
        if !(word.eq_ignore_ascii_case("lamp") || word.eq_ignore_ascii_case("lamb")) {
            return false;
        }
        let before = text[..start].trim_end();
        let opens = before.is_empty() || before.ends_with([',', '.', '!', '?', ';', ':']);
        let greeted = i > 0 && {
            let (from, to) = words[i - 1];
            ["hey", "hi", "ok", "okay", "hello"]
                .iter()
                .any(|greeting| text[from..to].eq_ignore_ascii_case(greeting))
                && text[to..start]
                    .trim_matches(|c: char| c.is_whitespace() || c == ',')
                    .is_empty()
        };
        opens || greeted
    })
}

/// Words that open a line set off and are said, never a name heard for the
/// lamp's (the reviewer-lead's HOLD on `0990f0a27`: "Tomorrow, I see the
/// dentist at 3" must keep its day): time and day words, verbs a request
/// opens with, and discourse words, English and Vietnamese, NFC lower case.
const NOT_A_NAME: &[&str] = &[
    // Time and day.
    "today",
    "tomorrow",
    "tonight",
    "yesterday",
    "now",
    "later",
    "soon",
    "morning",
    "afternoon",
    "evening",
    "night",
    "noon",
    "weekend",
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
    "mai",
    "nay",
    "s\u{e1}ng",
    "tr\u{1b0}a",
    "chi\u{1ec1}u",
    "t\u{1ed1}i",
    "\u{111}\u{00ea}m",
    "h\u{f4}m",
    // Verbs a request opens with.
    "remember",
    "remind",
    "forget",
    "delete",
    "tell",
    "set",
    "turn",
    "play",
    "stop",
    "show",
    "call",
    "add",
    "look",
    "find",
    "check",
    "make",
    "put",
    "start",
    "open",
    "close",
    "read",
    "nh\u{1edb}",
    "qu\u{ea}n",
    "nh\u{1eaf}c",
    "b\u{1ead}t",
    "t\u{1eaf}t",
    "m\u{1edf}",
    "x\u{f3}a",
    "k\u{1ec3}",
    "n\u{f3}i",
    "cho",
    // Discourse words.
    "honestly",
    "seriously",
    "really",
    "first",
    "second",
    "finally",
    "besides",
    "otherwise",
    "plus",
    "though",
    "however",
    "anyway",
    "basically",
    "luckily",
    "sadly",
    "guys",
    "everyone",
    "everybody",
    "hello",
    "hi",
    "okay",
    "ok",
    "alright",
    "listen",
    "excuse",
    "wow",
    "nh\u{01b0}ng",
    "v\u{e0}",
    "r\u{1ed3}i",
    "th\u{1ebf}",
    "\u{1eeb}",
];

/// What the model is told of [`wake_heard_as`]'s word: not a name.
pub(crate) fn wake_note(word: &str) -> String {
    format!(
        "(For you to know, not to answer: \"{word}\" at the start of what they said is how your name, Lamp, was heard. It is nobody's name: never call them by it, and never keep it as a name or in anything you remember.)"
    )
}

/// With `LAMP_MEMORY_KEEP_HEARD` (`keep_heard`) and lamp-name's hearing of
/// the name in the line (`named`), a line opening with [`wake_heard_as`]'s word: its request's
/// own line is held without it, so a kept fact is never worded with it
/// (lamp-06c1, 27 Oct: "Edeland's dog is called Moe"), and the model is
/// told it is no name. `None` changes nothing: the switch off, no name
/// heard, or no such word.
pub(crate) fn wake_not_a_name(
    request: u64,
    text: &str,
    named: bool,
    keep_heard: bool,
) -> Option<String> {
    if !(named && keep_heard) {
        return None;
    }
    let Some((word, rest)) = wake_heard_as(text) else {
        // The name heard as itself is no part of what they said to keep
        // either (lamp-06c1, 28 Oct: "They said: Hey, Lamp. …" kept): the
        // line is held without the call, and the model is told nothing.
        if let Some(rest) = name_call_rest(text)
            && let Ok(mut thread) = crate::thread::THREAD.lock()
        {
            thread.line_of(request, rest);
            eprintln!(
                "lamp-voice second: request {request}: it opens by calling the lamp by name: its line is kept without the call (LAMP_MEMORY_KEEP_HEARD)"
            );
        }
        return None;
    };
    if let Ok(mut thread) = crate::thread::THREAD.lock() {
        thread.line_of(request, rest);
    }
    eprintln!(
        "lamp-voice second: request {request}: its first word, set off, is the lamp's name as heard, not a name: the model is told, and its line is kept without it (LAMP_MEMORY_KEEP_HEARD)"
    );
    Some(wake_note(word))
}

/// Told how the room was, and who spoke, as a line the rule has not judged
/// ended, by its request and window (`worker::Heeding::keep_unjudged`):
/// the name's line, and the line after it before it is judged.
pub type LineKept = Arc<dyn Fn(u64, (u64, u64)) + Send + Sync>;

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
    /// The line before it in this session and how long after that one's
    /// end it began, or, when its sound or words began with the line just
    /// before it, that line's own (`LAMP_JOIN_CUT_WORDS`): a chain goes on
    /// across a line folded into the next.
    link: Option<(u64, Duration)>,
    /// When it began by the engine's clock, or, folded, when the line it
    /// began with did: where a chain's piece is timed from
    /// ([`CHAIN_MOST_FOR`]).
    began: Instant,
}

/// A spoken request's words, kept while it is answered ([`Shared::in_flight`],
/// [`Shared::cut`]).
#[derive(Clone)]
struct Carried {
    request: u64,
    /// Its words, oldest first: one part, or with `LAMP_JOIN_CUT_WORDS` the
    /// chain of lines cut short before it and then its own.
    parts: Vec<Part>,
    at: Instant,
    /// The request whose line said these words: `request`'s own, or, for
    /// words answered in a held line's place, the line they came from. A
    /// join compares that line's speaker, never the held line's
    /// ([`joined_with_cut`]).
    said_by: u64,
    /// Links of the chain no line's verdict has judged one person's yet,
    /// the earlier request and the later: a line cut short before it was
    /// asked ([`Engine::chain_heard`]), or one cut while its join waited.
    unchecked: Vec<(u64, u64)>,
    /// The request that first tried a memory write for these words
    /// ([`WriteOwner`]), shared with a held line answering them again.
    memory_write_owner: WriteOwner,
}

/// Which request first tried `remember`, `forget` or `forget_everything`
/// for one spoken line's words. A held line with no words of its own that
/// answers those words again shares it, so it cannot write again what the
/// first one wrote, or may have (lamp-06c1, 26 Oct, build 6963: one
/// "remember my favourite planet is Saturn" was kept three times, two
/// empty held lines answering its words again). New words get their own.
type WriteOwner = Arc<Mutex<Option<u64>>>;

/// Whether `name` writes memory, for [`WriteOwner`]: `remember`, `forget`
/// and `forget_everything`.
fn writes_memory(name: &str) -> bool {
    matches!(
        name,
        crate::remember::REMEMBER | crate::remember::FORGET | crate::remember::FORGET_EVERYTHING
    )
}

/// Whether `request` may write memory for its words ([`WriteOwner`]):
/// with no owner, always; else it claims the owner if no request has, and
/// may if it is the one that did. A poisoned owner refuses. Called only
/// for a call [`run_tool`] would send ([`reaches_memory`]): one refused
/// before it is sent claims nothing, so a later answer to the same words
/// may still try (Reviewer-two and reviewer-three on #3614).
fn claims_write(owner: Option<&WriteOwner>, request: u64) -> bool {
    owner.is_none_or(|owner| {
        owner
            .lock()
            .is_ok_and(|mut owner| *owner.get_or_insert(request) == request)
    })
}

/// Whether [`run_tool`] would send `name` on: not a memory call on a line
/// held only by a gap, nor a call with the route's scoped tools and no
/// scope (`unavailable`).
fn reaches_memory(
    shared: &Shared,
    name: &str,
    scope: Option<&crate::memory_scope::ScopeLease>,
) -> bool {
    let held = held_by_gap(scope) && MEMORY_CALLS.contains(&name);
    let unavailable = shared.scoped_tools.is_some() && scope.is_none();
    !held && !unavailable
}

/// What the model is told of a memory write refused by [`WriteOwner`].
const CARRIED_MEMORY_WRITE: &str = "Not done: a memory change was already tried for these words. Do not call it again; if you are not sure it was kept, say so.";

/// One line's words in a chain cut short ([`Carried::parts`]): its request,
/// its words, when its line began by the engine's clock, and how many lines
/// they are (two when its sound or words began with the line before it).
#[derive(Clone)]
struct Part {
    request: u64,
    text: String,
    at: Instant,
    lines: u16,
}

impl Carried {
    /// `request`'s own `text`, kept at `at`, said by `said_by`.
    fn own(request: u64, text: String, at: Instant, said_by: u64) -> Self {
        Self {
            request,
            parts: vec![Part {
                request,
                text,
                at,
                lines: 1,
            }],
            at: Instant::now(),
            said_by,
            unchecked: Vec::new(),
            memory_write_owner: WriteOwner::default(),
        }
    }

    /// All its words, oldest first.
    fn text(&self) -> String {
        words_of(&self.parts)
    }
}

/// The words of `parts`, oldest first, one space between.
fn words_of(parts: &[Part]) -> String {
    parts
        .iter()
        .map(|part| part.text.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
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

/// [`DOWN_LINE`] and [`DOWN_LINE_AGAIN`], rendered by [`Services::speak`]
/// after a turn, while the relay answers, and kept in memory, so they can be
/// said when the chat no longer does.
#[derive(Default)]
struct DownLine {
    clip: Mutex<Option<Vec<i16>>>,
    again: Mutex<Option<Vec<i16>>>,
    cannot: Mutex<Option<Vec<i16>>>,
    cannot_at: Mutex<Option<tokio::time::Instant>>,
    rendering: AtomicBool,
    /// When the last was said, and whether it was [`DOWN_LINE_AGAIN`].
    said_at: Mutex<Option<(tokio::time::Instant, bool)>>,
}

impl DownLine {
    fn render(self: &Arc<Self>, services: &Arc<dyn Services>) {
        if self.rendering.swap(true, Ordering::AcqRel) {
            return;
        }
        let line = self.clone();
        let services = services.clone();
        tokio::spawn(async move {
            // Each clip rendered until it is kept: one that failed (the
            // short one alone, say) is asked again after the next turn, and
            // the ones kept are not rendered again (Third's P2 on #3771).
            let missing =
                |slot: &Mutex<Option<Vec<i16>>>| slot.lock().is_ok_and(|kept| kept.is_none());
            for (slot, text) in [
                (&line.clip, DOWN_LINE),
                (&line.again, DOWN_LINE_AGAIN),
                (&line.cannot, DOWN_LINE_CANNOT),
            ] {
                if !missing(slot) {
                    continue;
                }
                let clip = render_line(&services, text).await;
                if clip.is_empty() {
                    continue;
                }
                if text == DOWN_LINE {
                    // The real length, for whether the 4 s cap ever cuts it
                    // (the reviewer on #1195).
                    eprintln!(
                        "lamp-voice second: the down line is {} ms",
                        clip.len() as u64 * 1000 / 24_000
                    );
                }
                if let Ok(mut kept) = slot.lock() {
                    *kept = Some(clip);
                }
            }
            if [&line.clip, &line.again, &line.cannot]
                .into_iter()
                .any(missing)
            {
                eprintln!(
                    "lamp-voice second: a down line could not be said; trying again after the next turn"
                );
                line.rendering.store(false, Ordering::Release);
            }
        });
    }

    /// The line to say now, if rendered, and whether it is
    /// [`DOWN_LINE_AGAIN`]: that one when [`DOWN_LINE`] was the last said,
    /// less than [`DOWN_LINE_EVERY`] ago; otherwise [`DOWN_LINE`]. Never the
    /// same line twice in a row within the minute (#3755).
    fn due(&self) -> Option<(Vec<i16>, bool)> {
        let said_at = *self.said_at.lock().ok()?;
        let now = tokio::time::Instant::now();
        let again = said_at
            .is_some_and(|(at, was_again)| !was_again && now.duration_since(at) < DOWN_LINE_EVERY);
        // The short line not rendered (yet): the long one rather than
        // silence (Third's P2 on #3771).
        let short = again.then(|| self.again.lock().ok()?.clone()).flatten();
        match short {
            Some(clip) => Some((clip, true)),
            None => Some((self.clip.lock().ok()?.clone()?, false)),
        }
    }

    /// [`DOWN_LINE_CANNOT`], if rendered and not said in the last
    /// [`DOWN_LINE_EVERY`].
    fn cannot_due(&self) -> Option<Vec<i16>> {
        let at = *self.cannot_at.lock().ok()?;
        if at.is_some_and(|at| tokio::time::Instant::now().duration_since(at) < DOWN_LINE_EVERY) {
            return None;
        }
        self.cannot.lock().ok()?.clone()
    }

    fn cannot_said(&self) {
        if let Ok(mut at) = self.cannot_at.lock() {
            *at = Some(tokio::time::Instant::now());
        }
    }

    /// The line was played (Nine on #1195: a cue that failed keeps it due).
    fn said(&self, again: bool) {
        if let Ok(mut said_at) = self.said_at.lock() {
            *said_at = Some((tokio::time::Instant::now(), again));
        }
    }
}

/// `text` said by the route's voice into one clip, at most
/// `DOWN_LINE_MOST_SAMPLES`; empty if the voice failed.
async fn render_line(services: &Arc<dyn Services>, text: &str) -> Vec<i16> {
    let mut clip = Vec::new();
    let mut speech = services.speak(text.to_owned());
    while let Some(packet) = speech.next().await {
        match packet {
            Ok(packet) => clip.extend(packet),
            Err(_) => return Vec::new(),
        }
        if clip.len() >= DOWN_LINE_MOST_SAMPLES {
            clip.truncate(DOWN_LINE_MOST_SAMPLES);
            break;
        }
    }
    clip
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

/// What a line's task is given while the line goes on: its sound, or that
/// it has gone quiet for a moment (`ProviderInput::Paused`,
/// `LAMP_SECOND_EARLY_FINALIZE`).
enum LinePart {
    /// The flag distinguishes this request's input from an older carried piece.
    Sound(Vec<i16>, bool, u64),
    Pause,
}

struct Turn {
    request: u64,
    /// Open until the request's End; `None` after it.
    audio: Option<mpsc::Sender<LinePart>>,
    /// The End came marked as a short sound let through; set before the
    /// audio queue closes, read by the turn's task after.
    sound: Arc<AtomicBool>,
    /// When its first and last packets were read from the microphone.
    said: Arc<Mutex<(u64, u64)>>,
    /// What the coordinator marked it with, told beside its words.
    marks: Arc<Marks>,
    task: JoinHandle<()>,
    memory_scope: Option<crate::memory_scope::RequestScope>,
}

/// Whether a request was judged in a busy room (`worker::Heeding`).
pub type Busy = Arc<dyn Fn(u64) -> bool + Send + Sync>;

/// What the rule says of a line that waits for the Policy's decision
/// (`LAMP_GOING_ON_ASKS`, #1522): in a busy room, a line it would let out
/// only as a conversation going on. The turn then asks
/// [`Engine::asking_the_policy`]'s hook every [`POLICY_POLL`] until it
/// decides, as the Live worker keeps such a line's first output.
pub(crate) const WAITS_FOR_POLICY: &str = "it waits for the Policy's decision";

/// The Policy's decision for a line that waits for it: `None` while it may
/// still come, then the line's verdict (`None` to play it, or why it is
/// held).
pub type PolicyNow = Arc<dyn Fn(u64) -> Option<Option<&'static str>> + Send + Sync>;

/// How often a line that waits for the Policy asks again.
const POLICY_POLL: Duration = Duration::from_millis(50);

/// What a spoken line is told beside its words from the coordinator's
/// marks, as the Live route tells them (#1712, item 4 of the first
/// builder's 20:15: the second worker dropped `Called`, and told none of
/// these). A written or reminded turn has none.
#[derive(Default)]
struct Marks {
    /// The privacy generation the line began under: a listener kept for it
    /// must be of the same one ([`Kept`]).
    privacy: u64,
    /// Begun over the lamp's audible answer, and not the first after a
    /// stop, which is told that instead, with `--cut-in-told`
    /// (`LAMP_CUT_IN_TOLD`, [`crate::cut_in_told::NOTE`]).
    cut_in: bool,
    /// Begun soon after the lamp's words finished playing (`Start`'s
    /// `follows_on`, `LAMP_FOLLOWS_ON`): told it may follow on from them
    /// ([`crate::follows_on::NOTE`]), unless its End marks a sound or it
    /// was judged in a busy room.
    follows_on: bool,
    /// Its End marked a short sound; the turn's [`Turn::sound`].
    sound: Arc<AtomicBool>,
    /// Only the lamp's name, its call's wait run out
    /// (`ProviderInput::Called`, [`crate::provider_worker::CALLED_NOTE`]).
    called: AtomicBool,
    /// Onboarding's sentence (#1803): its words are counted, never asked
    /// of the model nor kept ([`ProviderInput::CountOnly`]).
    count_only: AtomicBool,
    /// Begun while onboarding waits for a call (#3504): only the lamp's
    /// name is the step's call, not a turn ([`ProviderInput::StepCall`]).
    step_call: AtomicBool,
    /// The line before it in this session and how long after that one's
    /// end this one began, when none of that one's words or sound began
    /// this one already ([`Engine::joining_cut_words`]). When its sound or
    /// words began with the line just before it (`folded`), that line's
    /// own, so a chain goes on across it.
    after_end: Option<(u64, Duration)>,
    /// Its sound or words began with the line just before it.
    folded: bool,
    /// Begun while the lamp's answer could be heard, not only over its wait
    /// sound (`Start`'s `over_the_lamps_voice`): judged for its echo
    /// ([`Engine::holding_own_echo`]).
    over_the_lamps_voice: bool,
    /// When it began by the engine's clock, or, folded, when the line it
    /// began with did ([`KeptSound::began`]); none for a line not spoken.
    began: Option<Instant>,
    /// Begun while the line before it, the lamp's name, waited for it
    /// (`LAMP_SECOND_NAME_CARRIES`): said by that name when one person said
    /// both ([`said_by_the_name`]); taken by its task.
    name_of: Mutex<Option<NameWait>>,
    /// When its End came, by the runtime's clock (`LAMP_SECOND_NAME_CARRIES`).
    ended_at: Mutex<Option<tokio::time::Instant>>,
    /// Whether its words are back, and, when the line after it began first
    /// and waits for them, where they go ([`NameWords::Coming`]).
    words_back: Mutex<(bool, Option<tokio::sync::oneshot::Sender<Option<NameSaid>>>)>,
    /// Said by the lamp's name before it ([`said_by_the_name`],
    /// `LAMP_SECOND_NAME_CARRIES`): its words are two lines in a chain. It
    /// goes on only from the name, never from a line before the name, often
    /// the end of other talk (reviewer-three's P2, 23 Oct).
    name_carried: AtomicBool,
    /// How long its sound began before its usual start by a soft start's
    /// lead (`Start`'s `soft_lead_us`, #4226): only while its window begins
    /// with its own first sound, and 0 once it is dated from sound carried
    /// before it.
    soft_lead_us: AtomicU64,
}

/// What a line that was only the lamp's name is asked as when no words of
/// it came back: the name the coordinator heard, as Live's model hears it.
const CALLED_WORDS: &str = "Lamp.";

/// A line's words past a "hey" or "ok" before them, any case and
/// punctuation aside, an apostrophe kept in its word ("what's", "Lamp's")
/// (`LAMP_SECOND_NAME_CARRIES`).
fn past_a_greeting(text: &str) -> Vec<&str> {
    let mut words: Vec<&str> = text
        .split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '\u{2019}'))
        .filter(|word| !word.is_empty())
        .collect();
    if words.first().is_some_and(|first| {
        ["hey", "ok", "okay"]
            .iter()
            .any(|greeting| first.eq_ignore_ascii_case(greeting))
    }) {
        words.remove(0);
    }
    words
}

/// Whether a line's words are only the lamp's name
/// ([`net::key_terms::NAME`]), in any case and with any punctuation, with
/// "hey" or "ok" before it at most (`LAMP_SECOND_NAME_CARRIES`).
fn name_alone(text: &str) -> bool {
    matches!(past_a_greeting(text)[..], [word] if word.eq_ignore_ascii_case(net::key_terms::NAME))
}

/// The most words a short line may have after the lamp's name and still
/// wait for the line after it ([`name_first`]).
const NAME_FIRST_MORE: usize = 3;

/// Whether a line is short and begins with the lamp's name, set off as a
/// call ([`crate::attention::begins_with_the_name`]), with at most
/// [`NAME_FIRST_MORE`] words after it: "Lamp. So", "Lamp, can", "Hey Lamp,
/// what's"; and the name alone ([`name_alone`]). Not "Lamp is broken.",
/// nor a line whole as it is ([`whole_already`]), which is answered at
/// once (`LAMP_SECOND_NAME_CARRIES`).
fn name_first(text: &str) -> bool {
    let words = past_a_greeting(text);
    words
        .first()
        .is_some_and(|first| first.eq_ignore_ascii_case(net::key_terms::NAME))
        && words.len() <= 1 + NAME_FIRST_MORE
        && !whole_already(text)
        && crate::attention::begins_with_the_name(text)
}

/// Whether a line is whole as it is, so never waits for the line after it
/// (`LAMP_SECOND_NAME_CARRIES`, El's P2 on the branch: "Lamp, stop."
/// waited 2.5 s): a stop or "never mind" ([`says_stop`], the stop words'
/// one list); a question past the name ("Lamp, what?"); or two words or
/// more past the name ending a sentence ("Lamp, lights off."). This route
/// has no turn-end verdict for its lines, and no verb list: a sentence's
/// end over two words is the simplest bound that keeps "Lamp. So",
/// "Lamp, can" and "Lamp. So." waiting.
fn whole_already(text: &str) -> bool {
    let past = past_a_greeting(text).len().saturating_sub(1);
    let end = text.trim_end();
    says_stop(text) || past >= 1 && end.ends_with('?') || past >= 2 && end.ends_with(['.', '!'])
}

impl Shared {
    /// Whether `request` was judged in a busy room: by the rule's hook, or
    /// else by this process's room now, as the Live worker reads its own.
    fn busy(&self, request: u64) -> bool {
        match &self.busy {
            Some(busy) => busy(request),
            None => crate::room_told::room()
                .lock()
                .is_ok_and(|mut room| room.at(monotonic_us()) == crate::room_told::Room::Busy),
        }
    }
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
    /// The coordinator's call whose words were not back when the next line
    /// began, kept running for them (`LAMP_SECOND_NAME_CARRIES`,
    /// [`NameWords::Coming`]): it says and keeps nothing. Let go at the
    /// next start or a stop.
    superseded: Option<Turn>,
    /// `--sounds-checked`: a short sound is answered only when heard as a
    /// short word.
    sounds_checked: bool,
    /// `--cut-in-told` ([`Engine::telling_cut_ins`]).
    cut_in_told: bool,
    /// An answer's hand-on to memory is held until it is known how much of
    /// it was heard (`--said-as-heard`, [`crate::said_as_heard::HeldHandOn`]).
    holds_hand_on: bool,
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
    /// The camera's and the microphone's permission and generation last
    /// installed, and what is told when they change
    /// ([`Engine::forgetting_on_privacy`]).
    privacy: Option<(bool, u64, bool, u64)>,
    on_privacy: Option<Box<dyn FnMut() + Send>>,
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
                requested_audio: None,
                system,
                scoped_system: None,
                lease_system: None,
                clock,
                answered_at: Mutex::new(None),
                said_until_us: AtomicU64::new(0),
                said: Mutex::new((0, Vec::new(), crate::not_speech::CallsLeftOut::new())),
                sequence: AtomicU64::new(0),
                out,
                heed: None,
                verdicts: None,
                ask_again: None,
                misheard_at_us: Mutex::new(None),
                holding_answer: false,
                note_looks: false,
                walk_looks: false,
                said_once: false,
                declines_in_words: false,
                speech_tags: None,
                short_done: false,
                no_words_again: false,
                written_wait: false,
                resend: None,
                hearing: None,
                line_heard: None,
                asked: None,
                cues: None,
                cue_after: CUE_AFTER,
                down: None,
                spoken_stop_ends: false,
                stop_line: Mutex::new(None),
                announce: true,
                heard_last: Mutex::new(None),
                lamp_said: Mutex::new((0, String::new())),
                stop_untold: Mutex::new(None),
                though_held: false,
                staff: None,
                remembered: None,
                event_turn: None,
                event_withheld: None,
                note_ahead: None,
                name_note: None,
                conversation_alone: None,
                named_ask: None,
                #[cfg(test)]
                named_ask_journal: None,
                busy: None,
                named_to_a_face: None,
                policy_now: None,
                call_note: None,
                name_sounded: None,
                name_fired: None,
                task_waiting: None,
                air: None,
                seems: None,
                did: None,
                hold: None,
                policy: None,
                words_heard: None,
                look_by_call: false,
                skill_look: None,
                search_pushed: None,
                prepare: false,
                unfinished_words: false,
                note_tools: false,
                #[cfg(test)]
                tools_journal: None,
                keep_listener: false,
                open_name: false,
                name_carries: false,
                name_waits: Mutex::new(None),
                name_refused: Mutex::new(None),
                one_speaker: None,
                name_near: None,
                own_echo: false,
                spare: Mutex::new(None),
                spares: AtomicU64::new(0),
                warmed: Mutex::new(None),
                begun: Mutex::new(None),
                wait_sound_only: Mutex::new(None),
                look_note: Mutex::new(None),
                in_flight: Mutex::new(None),
                answered_request: Mutex::new(None),
                cut: Mutex::new(None),
                held_last: Mutex::new(None),
                asked_last: Mutex::new(None),
                same_speaker: None,
                unsay_joined: None,
                first_words: None,
                speaking: None,
                held_forget: Mutex::new(HeldForgets::default()),
                forget_sight: None,
                remember_speaker: false,
                enrollment_sight: None,
                scope_seen: false,
            }),
            output,
            urgent: VecDeque::new(),
            turn: None,
            retired_through: 0,
            begun_last: None,
            superseded: None,
            sounds_checked: false,
            cut_in_told: false,
            holds_hand_on: false,
            look: None,
            kept: None,
            reminders: false,
            room_tick: None,
            memory: None,
            memory_camera_allowed: false,
            memory_camera_generation: None,
            privacy: None,
            on_privacy: None,
        }
    }

    /// `forget` is told each time the camera or the microphone is closed or
    /// opened, or its privacy generation changes (a close and reopen
    /// coalesced), as the worker installs each authority: what was seen
    /// before goes then (#2305's review). Before any turn only.
    pub fn forgetting_on_privacy(mut self, forget: Box<dyn FnMut() + Send>) -> Self {
        self.on_privacy = Some(forget);
        self
    }

    /// The camera's and the microphone's permission and generation from an
    /// installed authority; a change from the last tells
    /// [`Self::forgetting_on_privacy`]'s hook.
    pub(crate) fn privacy(&mut self, now: (bool, u64, bool, u64)) {
        if self.privacy.replace(now).is_some_and(|was| was != now) {
            if self.shared.remember_speaker {
                crate::remember_speaker::forget();
                if let Some(eyes) = &self.shared.enrollment_sight {
                    eyes.forget();
                }
            }
            if let Some(forget) = self.on_privacy.as_mut() {
                forget();
            }
            if let Some(ahead) = &self.shared.note_ahead {
                (ahead.forget)();
            }
        }
    }

    /// Bind tools to the observation at admission, independent of later face changes.
    pub fn with_memory_scope(
        mut self,
        boot: lamp_turn_owner::BootId,
        cache: Arc<crate::memory_scope::FaceCache>,
        runner: ScopedToolRunner,
        system: Option<ScopedSystem>,
    ) -> Self {
        let shared = Arc::get_mut(&mut self.shared).expect("configured before admission");
        shared.scoped_tools = Some(runner);
        shared.scoped_system = system;
        self.memory = Some((boot, cache));
        self
    }

    /// Without scoped context, a turn is still told for the person counted
    /// at the lamp, but through `system`, which reads the line's own lease
    /// for the typed name alone (#2602, the reviewer's P2 on #3050): the
    /// counted person stays the owner's for 30 s after they go.
    pub fn reading_the_lease_for_the_name(mut self, system: Option<ScopedSystem>) -> Self {
        let shared = Arc::get_mut(&mut self.shared).expect("configured before admission");
        shared.lease_system = system;
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

    /// Whether turns are told and kept for their admitted memory scope
    /// (a [`ScopedSystem`] given to [`Self::with_memory_scope`]).
    pub(crate) fn scopes_context(&self) -> bool {
        self.shared.scoped_system.is_some()
    }

    /// The current request's turn ended: if the lamp spoke its answer (its
    /// own first audio went, not an announcement's or a cue's), its known
    /// person is the one the lamp is talking with now, for the face cache's
    /// held gap (`LAMP_FACE_GAP_HOLD`). A line left unclear, judged not for
    /// the lamp, held or given up marks no one (the reviewers' P2 and P3 on
    /// #2265), nor does a request of no words left unanswered (a sound):
    /// the gap runs on from the answer before it. Called at the
    /// turn's end, so the gap is measured from the end of the answer.
    pub(crate) fn memory_answered(&self, request: u64, at: Instant) {
        let spoken = self
            .shared
            .answered_request
            .lock()
            .is_ok_and(|last| *last == Some(request));
        if spoken
            && let (Some((_, cache)), Some(turn)) = (&self.memory, self.turn.as_ref())
            && turn.request == request
            && let Some(lease) = turn.memory_scope.as_ref().map(|scope| scope.lease())
            && let Some(scope) = lease.scope()
        {
            cache.answered(scope, at);
        }
    }

    /// Authority of the current admitted request, including its immutable scope.
    pub(crate) fn memory_lease(&self) -> Option<crate::memory_scope::ScopeLease> {
        self.turn
            .as_ref()?
            .memory_scope
            .as_ref()
            .map(|scope| scope.lease())
    }

    fn memory_scope(
        &self,
        request: u64,
        session: u64,
        heard: bool,
    ) -> Option<crate::memory_scope::RequestScope> {
        self.memory.as_ref().map(|(worker, cache)| {
            // A heard line with the camera allowed, its turn told and kept
            // by its own scope (`LAMP_SECOND_SCOPED_CONTEXT`): admitted as
            // the room's, its keeper is decided at its end (#2807,
            // `ScopeLease::face_at_end`), armed only if the face cache was
            // cold at its start or went cold before its end plus
            // `KEEPER_WAIT` (after a restart, a privacy change, a face
            // remembered or a known-file change, for up to `COLD_FOR` with
            // no face named), and if only during the line, only for a face
            // the eyes saw in view as it began; a warm cache with no change
            // through the line arms nothing (El's P2 on #3108).
            let late = heard && self.memory_camera_allowed && self.scopes_context();
            let scope = if heard && self.memory_camera_allowed {
                let now = std::time::Instant::now();
                let scope = cache.snapshot(now);
                // With `LAMP_FACE_GAP_HOLD`, why the line is or isn't
                // held: counts and ms only (c0a4's run on #2449).
                if let Some(told) = cache.gap_told(now) {
                    let whose = match (scope.held(), scope.person()) {
                        (true, _) => "held by a gap",
                        (false, Some(_)) => "a known face's",
                        (false, None) => "the room's",
                    };
                    eprintln!("lamp-voice second: request {request}: memory scope {whose}: {told}");
                }
                // A line held only by a gap is never journalled, whether or
                // not its exchange is filed by its scope
                // (`LAMP_SECOND_SCOPED_CONTEXT`): kept out as it is admitted,
                // before any of its words or its end (the reviewer's P2 on
                // #2616).
                if scope.held() {
                    crate::remember::held_by_gap(request);
                }
                scope
            } else {
                crate::memory_scope::ScopeSnapshot::room()
            };
            let key = crate::memory_scope::RequestKey {
                worker: *worker,
                session,
                request,
            };
            if late {
                crate::memory_scope::RequestScope::looking_later(key, scope, cache.clone())
            } else {
                crate::memory_scope::RequestScope::new(key, scope)
            }
        })
    }

    /// A line any face other than the known person's was seen speaking
    /// through is told and kept as the room's (`LAMP_SECOND_SPEAKING_FACE=on`,
    /// [`crate::speaking_faces`]): `known` is the eyes' known file, with its
    /// face beside it. Only after [`Self::with_memory_scope`] with a scoped
    /// builder (`LAMP_SECOND_SCOPED_CONTEXT=on`); refused otherwise.
    pub fn seeing_who_speaks(
        mut self,
        eyes: Arc<crate::speaking_faces::SpeakingFaces>,
        known: std::path::PathBuf,
    ) -> Self {
        let shared = Arc::get_mut(&mut self.shared).expect("configured before admission");
        // Without a scoped builder a turn is told and filed for the sticky
        // person, whatever its lease says, so a narrowed line would still
        // carry their memory and history (Second on #2008): refused.
        if shared.scoped_system.is_none() {
            eprintln!(
                "lamp-voice second: seeing who speaks is not on: it needs LAMP_SECOND_SCOPED_CONTEXT=on"
            );
            return self;
        }
        shared.speaking = Some((eyes, known));
        self
    }

    /// A line is in a known person's scope only when their own face was
    /// seen speaking it, else the room's, at its admission and for a late
    /// look's keeper alike (`LAMP_PERSON_SCOPE_SEEN=on`,
    /// `speaking_faces::not_theirs`). Without [`Self::seeing_who_speaks`]
    /// (its reader never started) no face is seen: every line the room's.
    pub fn scoping_by_sight(mut self, on: bool) -> Self {
        let shared = Arc::get_mut(&mut self.shared).expect("configured before admission");
        shared.scope_seen = on;
        self
    }

    /// A held forget's yes counts only from the held person's face seen
    /// saying it (`LAMP_EYES_SPEAKING=on`, #2377): `eyes` are the eyes'
    /// looks and `known` their known file, with its face beside it. Without
    /// it, the yes runs it while their face is the one at the lamp, as
    /// #2340 did.
    pub fn confirming_forgets_by_sight(
        mut self,
        eyes: Arc<crate::speaking_faces::SpeakingFaces>,
        known: std::path::PathBuf,
    ) -> Self {
        let shared = Arc::get_mut(&mut self.shared).expect("configured before admission");
        shared.forget_sight = Some((eyes, known));
        self
    }

    /// Opt-in enrollment uses only the face seen speaking the exact audio
    /// interval. Missing eyes leaves it enabled but unable to save a face.
    pub fn remembering_speaker(
        mut self,
        on: bool,
        eyes: Option<Arc<crate::speaking_faces::SpeakingFaces>>,
    ) -> Self {
        let shared = Arc::get_mut(&mut self.shared).expect("configured before admission");
        shared.remember_speaker = on;
        shared.enrollment_sight = on.then_some(eyes).flatten();
        self
    }

    /// Short sounds let through are checked (`--sounds-checked`, #800).
    pub fn checking_sounds(mut self, on: bool) -> Self {
        self.sounds_checked = on;
        self
    }

    /// A spoken line begun over the lamp's audible answer, and not the
    /// first after a stop, is told [`crate::cut_in_told::NOTE`] beside its
    /// words (`--cut-in-told`, `LAMP_CUT_IN_TOLD=on`), as the Live route
    /// tells it.
    pub fn telling_cut_ins(mut self, on: bool) -> Self {
        self.cut_in_told = on;
        self
    }

    /// With `on` (`--said-as-heard`, `LAMP_SAID_AS_HEARD=on`), the worker
    /// holds each answer's hand-on to memory until it is known how much of it
    /// was heard ([`crate::said_as_heard::HeldHandOn`]).
    pub fn holding_hand_on(mut self, on: bool) -> Self {
        self.holds_hand_on = on;
        self
    }

    /// Whether answers' hand-ons to memory are held ([`Self::holding_hand_on`]).
    pub fn holds_hand_on(&self) -> bool {
        self.holds_hand_on
    }

    /// The Policy's decision for a line the rule says waits for it
    /// ([`WAITS_FOR_POLICY`], `LAMP_GOING_ON_ASKS`). Before any turn only.
    pub fn asking_the_policy(mut self, policy: PolicyNow) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.policy_now = Some(policy);
        }
        self
    }

    /// Whether a request was judged in a busy room, so a line that may
    /// follow on from the lamp is not told so there (`LAMP_FOLLOWS_ON`).
    /// Without it, this process's room says, as the Live worker reads its
    /// own. Before any turn only.
    pub fn judging_busy(mut self, busy: Busy) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.busy = Some(busy);
        }
        self
    }

    /// Whether a request began with the lamp's name said to a face turned
    /// to the lamp (`LAMP_NAMED_ALWAYS=on`, #2191): the model's `not_for_me`
    /// on it is not taken, and it is asked once more, told it was called
    /// ([`NAMED_TO_A_FACE_ANSWER`]). Before any turn only.
    pub fn answering_named_to_a_face(mut self, named: Busy) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.named_to_a_face = Some(named);
        }
        self
    }

    /// Installed only by a worker with the explicit requested-audio switch.
    pub(crate) fn requesting_audio(
        mut self,
        boot: lamp_turn_owner::BootId,
        runner: RequestedAudioRunner,
    ) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.requested_audio = Some((crate::process::boot_text(boot), runner));
        }
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

    /// Whether the microphone may be listened to, and under which privacy
    /// generation, from each authority the worker installs: a kept listener
    /// goes at once when listening stops (the privacy switch, a mute) or
    /// the generation changes, before any line's sound could reach it
    /// (#1712, the first builder at 00:31).
    ///
    /// And with [`Self::keeping_a_listener_ready`], the first line's is
    /// opened here, once a generation, when listening is allowed and none
    /// is kept: the first line after the voice starts, or after listening
    /// comes back, goes to a socket already open. Each line opened the
    /// next one's, so the first had none: on lamp-4ace the first request
    /// after a start waited 753 ms for its socket, with 1,050 ms of its
    /// sound waiting (#2135, the first builder on #1722 at 01:15). It is
    /// kept [`KEPT_FOR`], as the next line's is. Outside a runtime, nothing
    /// is opened.
    pub fn listening(&self, allowed: bool, generation: u64) {
        if let Ok(mut spare) = self.shared.spare.lock()
            && spare
                .as_ref()
                .is_some_and(|kept| !allowed || kept.generation != generation)
            && let Some(kept) = spare.take()
        {
            kept.opening.abort();
        }
        if !self.shared.keep_listener {
            return;
        }
        let Ok(mut warmed) = self.shared.warmed.lock() else {
            return;
        };
        if !allowed {
            *warmed = None;
            return;
        }
        if *warmed == Some(generation) || tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        *warmed = Some(generation);
        drop(warmed);
        if self.shared.spare.lock().is_ok_and(|spare| spare.is_none()) {
            keep_a_listener(&self.shared, generation);
            eprintln!(
                "lamp-voice second: speech to text's socket opened before the first line (LAMP_SECOND_STT_WARM)"
            );
        }
    }

    /// Speech to text's socket for the next line is opened while this one
    /// is heard and kept with `KeepAlive`, so the next line's sound goes to
    /// it from its first block (`LAMP_SECOND_STT_WARM=on`, #1712, the first
    /// builder at 00:25). Each line opened its own socket as it began, and
    /// its sound waited until the socket was open, then went at once: on
    /// lamp-4ace 8 of 12 lines' words waited 0.9 to 2.3 s for Deepgram to
    /// hear the burst. A kept socket found closed is not used, and the line
    /// opens its own as before. The first line's is opened before it, as
    /// listening is allowed ([`Self::listening`], #2135). Before any turn
    /// only.
    pub fn keeping_a_listener_ready(mut self, on: bool) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.keep_listener = on;
        }
        self
    }

    /// A spoken line whose words end before a name ("Hey Lamp, my name
    /// is", [`crate::known_faces::opens_a_name`]) is answered no sooner
    /// than [`OPEN_NAME_WITHIN`] after its end (`LAMP_SECOND_OPEN_NAME=on`,
    /// #2135): the name said next then goes on from it, and the two are
    /// answered as one line. On lamp-4ace "my name is" and 400 ms of quiet
    /// ended a line, the name came 9 ms after, and the lamp answered "my
    /// name is" alone (the first builder on #1722 at 02:16). A newer line
    /// ends the wait with the request; with none, it is answered as it is.
    /// Before any turn only.
    pub fn waiting_for_a_name(mut self, on: bool) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.open_name = on;
        }
        self
    }

    /// A spoken line that is only the lamp's name ([`name_alone`]), short
    /// and begun with it ([`name_first`]), or that the coordinator says was
    /// only the name ([`ProviderInput::Called`]), is never held as a short
    /// sound, and its answer waits up to [`NAME_CARRIES_FOR`] after its end
    /// (`LAMP_SECOND_NAME_CARRIES=on`). A line begun in that wait, in the
    /// same session, drops the name's request and is said by the name: the
    /// rule judges it with the name's words before its own (or "Lamp, "
    /// for a name misheard) and its window from the name's first sound,
    /// and it is asked so. Not when the eyes saw a face speak it that they
    /// did not see say the name. With none, the name alone is answered as
    /// a call ([`crate::provider_worker::CALLED_NOTE`]), a short line as it
    /// is. Before any turn only.
    pub fn carrying_the_name(mut self, on: bool) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.name_carries = on;
        }
        self
    }

    /// With [`Self::carrying_the_name`]: `same` says whether one person said
    /// the name and the line after it (`worker::Heeding::one_speaker`, the
    /// join's judgement), and `kept` keeps each of the two for it before
    /// the rule judges either. Without it the name never carries. Before
    /// any turn only.
    pub fn telling_one_speaker(mut self, same: SameSpeaker, kept: LineKept) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.one_speaker = Some((same, kept));
        }
        self
    }

    /// `LAMP_NAME_NEAR=on`: a spoken line's first word, set off and
    /// sounding like the lamp's name, is the name when `near` says
    /// lamp-name heard it near, [`NAME_NEAR_LEAST`] or more, within 1.3 s
    /// of the line's start ([`named_near`]). Without it, as before. Before
    /// any turn only.
    pub fn hearing_the_name_near(mut self, near: NameNear) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.name_near = Some(near);
        }
        self
    }

    /// `LAMP_SECOND_OWN_ECHO=on` ([`own_echo`], #2997): a spoken line begun
    /// over the lamp's answer or just after it, nothing but runs of the
    /// lamp's own words in its answer's order, is held as its echo; one such
    /// run at its start or end is taken out of what the model is sent. A
    /// line adding words, or begun after its words and answering its offer
    /// or question, is never held. Before any turn only.
    pub fn holding_own_echo(mut self, on: bool) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.own_echo = on;
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

    /// While a spoken line is heard, the coordinator is told whenever its
    /// words so far come to end on an unfinished word, or no longer do
    /// (`LAMP_SECOND_UNFINISHED_WAIT=on`, `ProviderOutput::WordsEnd`,
    /// [`crate::unfinished_words`]). Before any turn only.
    pub fn telling_unfinished_words(mut self, on: bool) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.unfinished_words = on;
        }
        self
    }

    /// `LAMP_HOLDING_ANSWER=on` (`--holding-answer`), as on Live: a spoken
    /// turn whose whole answer only asks the person to wait ("One moment.",
    /// `holding_answer::holding_only`), with nothing called in any round,
    /// sends `ProviderOutput::HoldingAnswer` before its idle `TurnComplete`,
    /// and the coordinator asks the route once to go on
    /// (`holding_answer::NOTE`). Before any turn only.
    pub fn holding_answers(mut self, on: bool) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.holding_answer = on;
        }
        self
    }

    /// `LAMP_SECOND_LOOK_NOTE=on` ([`LOOK_NOTE_ENVIRONMENT`]): a look's turn
    /// (`Written` with `look`) whose whole answer called nothing in any
    /// round sends `ProviderOutput::LookNoCall` before its idle
    /// `TurnComplete`; the coordinator decides whether it was a skill's
    /// search and notes it once (`coordinator::look_note`). Before any turn
    /// only.
    pub fn noting_looks(mut self, on: bool) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.note_looks = on;
        }
        self
    }

    /// `LAMP_SECOND_LOOK_WALK=on` ([`LOOK_WALK_ENVIRONMENT`]), with
    /// [`Engine::noting_looks`]: a skill's search's own written turn
    /// (`Written`'s `search`) whose whole answer called nothing sends
    /// `LookNoCall` too, look or not, so the coordinator walks the search on
    /// after its note's answer as after a look's. Before any turn only.
    pub fn walking_looks(mut self, on: bool) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.walk_looks = on;
        }
        self
    }

    /// `LAMP_SECOND_SAID_ONCE=on` ([`SAID_ONCE_ENVIRONMENT`]): a round that
    /// said words before its calls ("A minute, starting now." with
    /// `set_timer`) tells the next round, after the calls' results, that the
    /// person heard them ([`said_once_note`]), so it adds only what is new,
    /// such as the time. Before any turn only.
    pub fn saying_once(mut self, on: bool) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.said_once = on;
        }
        self
    }

    /// `LAMP_NOT_FOR_ME_WORDS=on` ([`NOT_FOR_ME_WORDS_ENVIRONMENT`]): on a
    /// line told `worker::CONVERSATION_ALONE`, a reply that is "not for me",
    /// whole or with the refusal's own clause after it
    /// ([`crate::not_speech::opens_not_for_me`]), ends as the `not_for_me`
    /// call does: nothing said, the line held back. Its first piece, and
    /// the announcement, wait only until its opening is known. A
    /// line with the name, a call, a turned face or an answer to the lamp's
    /// own question is never told that note, and nothing of it waits.
    /// Before any turn only.
    pub fn declining_in_words(mut self, on: bool) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.declines_in_words = on;
        }
        self
    }

    /// `LAMP_SECOND_SPEECH_TAGS=on` ([`SPEECH_TAGS_ENVIRONMENT`]): `Some`, each
    /// piece of a reply goes through [`speech_tags::Tags`]: the voice is given
    /// one listed tag a reply at most, only when it reads them (`true`), and
    /// the lamp's words none. `None`, pieces are as written. Before any
    /// turn only.
    pub fn speaking_tags(mut self, voiced: Option<bool>) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.speech_tags = voiced;
        }
        self
    }

    /// `LAMP_SHORT_DONE=on` ([`SHORT_DONE_ENVIRONMENT`], #2877): a turn
    /// whose every call is the light's or the volume's, each done and
    /// answered short by lamp-actions (its `"say"`), with no words of the
    /// model's before them, asks the next round told the lamp says the
    /// calls' own line ([`short_done_note`]), and says that line
    /// ([`short_done_line`]) only when the round gives no words and no
    /// calls; otherwise the turn goes on as without the switch (the
    /// Reviewer-lead's P2 on #2930). Before any turn only.
    pub fn answering_short(mut self, on: bool) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.short_done = on;
        }
        self
    }

    /// `LAMP_NO_WORDS_AGAIN=on` ([`NO_WORDS_AGAIN_ENVIRONMENT`], #3704): a
    /// spoken line that begins with the lamp's name, whose round gives no
    /// words and calls nothing, with nothing said or called earlier in the
    /// turn, is asked once more ([`NO_WORDS_AGAIN_NOTE`]); a second answer
    /// with no words either is answered [`NO_WORDS_LINE`]. A turn that
    /// called anything ends as before: a call may be told to say nothing.
    /// Before any turn only.
    pub fn asking_again_for_no_words(mut self, on: bool) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.no_words_again = on;
        }
        self
    }

    /// The chat's and the voice's connections opened as the route starts,
    /// as [`Services::warm`] opens them as each line begins: the first turn
    /// after a start, a written one above all, otherwise opens them inside
    /// its first round's [`BEGIN_WITHIN`]. On lamp-29e7 an onboarding
    /// invitation 3 s after the second route started over had not begun in
    /// 3 s and ended unanswered (`no_audio_answer`, #3754). Within a Tokio
    /// runtime; no turn waits for it.
    pub fn warm_ahead(&self) {
        tokio::spawn(self.shared.services.warm());
    }

    /// `LAMP_SECOND_WRITTEN_WAIT=on` ([`WRITTEN_WAIT_ENVIRONMENT`]): a turn
    /// the lamp wrote itself, news above all, waits as long as
    /// [`WRITTEN_BEGIN_WITHIN`] for its first round, not [`BEGIN_WITHIN`].
    /// Before any turn only.
    pub fn waiting_for_written(mut self, on: bool) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.written_wait = on;
        }
        self
    }

    /// `LAMP_RESEND_LOST_LINE=on`, as on Live (the first builder's 00:02 on
    /// #1722): a spoken line whose chat was lost on the network
    /// ([`link::lost`]) before any word was said or anything called is asked
    /// once more, if `back` finds the link back within
    /// `resend::RESEND_WITHIN_US` of the failure and no newer line has begun
    /// meanwhile; otherwise the turn fails as before, and the link's own
    /// check and down line follow. Never twice. Before any turn only.
    pub fn resending(mut self, back: LinkBack) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.resend = Some(back);
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

    /// A line that cut the one before it short, that one's answer not yet
    /// heard, is asked with that one's words before its own, as one line,
    /// when `same` says one person said both (`LAMP_JOIN_CUT_WORDS`): begun
    /// within [`JOIN_WITHIN`] of that one's end, in the same session.
    /// Let out, it is answered so; held, so are the words it cut short,
    /// which alone would be answered in its place. A line let out is also
    /// joined when `same` sees no sign of a second person
    /// ([`Joined::NoSecond`]); a held one is not. Otherwise as without it:
    /// in a two-person scene the second person's line let out is answered
    /// alone, and that one's words are let go. Before any turn only.
    pub fn joining_cut_words(mut self, same: SameSpeaker) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.same_speaker = Some(same);
        }
        self
    }

    /// With [`Self::joining_cut_words`]: a line cut short is joined only
    /// while none of its answer was heard, so any of that answer's words the
    /// thread kept were never played. Each request whose words are joined
    /// into a later line is given to `unsay` (`crate::thread::unsay` in the
    /// worker) once that line is asked ([`asked_unsaying`]): its unplayed answer is not the lamp's words in the history,
    /// and its question goes with the joined line, asked once with both
    /// (the first builder on #2602, 23 Oct 02:43: lamp-4ace's "Twenty past
    /// eight in…", never played, kept as said, and only Saigon answered).
    /// Words answered in a held line's place, not joined (#1712's carry),
    /// are unsaid too ([`unsay_carried`], reviewer-three's P3 on #3129).
    /// Before any turn only.
    pub fn unsaying_joined(mut self, unsay: Arc<dyn Fn(u64) + Send + Sync>) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.unsay_joined = Some(unsay);
        }
        self
    }

    /// A line the rule held as unclear (another script, a number alone, a
    /// cut-in with no English word) is told to the coordinator as
    /// `ProviderOutput::Unclear`, before its `TurnComplete`, as the Live
    /// worker tells it (#926, `LAMP_OTHER_LANGUAGE=sorry`): the coordinator
    /// asks again, or hums (`LAMP_UNCLEAR_HUM`, `LAMP_MISHEARD_HUM`). A
    /// misheard cut-in hummed for has the next request told so
    /// ([`crate::misheard_hum::NOTE`]). Before any turn only.
    /// A stop said over the lamp's answer ([`crate::spoken_stop`]) is
    /// answered with its one line and kept in the thread, the model not
    /// asked (`LAMP_SPOKEN_STOP_ENDS`, #3985). Before any turn only.
    pub fn ending_spoken_stops(mut self, on: bool) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.spoken_stop_ends = on;
        }
        self
    }

    pub fn asking_again(mut self, cut_in: CutIn, ask: AskAgain) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.ask_again = Some((cut_in, ask));
        }
        self
    }

    /// Each spoken line's sound is heard by `hearing` too, as the Live
    /// worker gives it to the same-voice check. Before any turn only.
    pub fn hearing(mut self, hearing: Hearing) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.hearing = Some(hearing);
        }
        self
    }

    /// Each line the listener opened is told to `heard` once it ends, with
    /// when it was said, whatever its words turn out to be: none, a hum or
    /// a sound included (`LAMP_FIRST_SECONDS_QUIET`, the ears' stream read
    /// outside every line). Before any turn only.
    pub fn telling_lines_heard(mut self, heard: LineHeard) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.line_heard = Some(heard);
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

    /// A spoken request is asked with what memory holds that may fit its
    /// words (`LAMP_SECOND_MEMORY_HITS=on`, #2176 row 5c,
    /// `crate::remember::hits_note`), computed as its notes are, within a
    /// short bound. Before any turn only.
    pub fn remembering(mut self, note: Remembered) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.remembered = Some(note);
        }
        self
    }

    /// With `LAMP_MEMORY_EVENTS` (the first builder on #2805, 22 Oct
    /// 20:28, and on #2602, 22 Oct 22:17): a spoken request, not one held
    /// only by a gap, is asked with `note`'s line last among its notes, its
    /// own instructions, after the session's note is read, ahead or as the
    /// turn is told ([`Self::asking_the_note_ahead`]), and taken ([`event_line`]):
    /// `crate::remember::event_turn_note` gives it on the person's first
    /// line of a session they were offered a dated follow-up in, when that
    /// line only greets. Every chat is told without the note's section for
    /// it that `withheld` gives (`crate::remember::event_withheld`): before
    /// that line, and for the rest of a session whose first line asked
    /// something (reviewer-two's P2 on #3059). Before any turn only.
    pub fn asking_after_events(mut self, note: Remembered, withheld: EventWithheld) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.event_turn = Some(note);
            shared.event_withheld = Some(withheld);
        }
        self
    }

    /// The memory's note is asked as each spoken line begins, off the
    /// worker's loop, and a turn waits for it without blocking the loop
    /// ([`NoteAhead`]). Before any turn only.
    pub fn asking_the_note_ahead(mut self, ahead: NoteAhead) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.note_ahead = Some(ahead);
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

    /// `heard` says whether the lamp's name was heard since a time: a line
    /// going on from a wordless piece whose name was heard takes that
    /// piece's time, so the name counts for it, and not its sound, which
    /// held the words back about 2 s (#1712, the first builder at 19:21).
    /// Before any turn only.
    /// `fired` says whether lamp-name heard the name in a request's line:
    /// the signal for [`wake_not_a_name`] (#3504 finding N4). Before any
    /// turn only.
    pub fn knowing_the_name_fired(mut self, fired: NameFired) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.name_fired = Some(fired);
        }
        self
    }

    pub fn knowing_the_name_heard(mut self, heard: NameSounded) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.name_sounded = Some(heard);
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

    /// With `LAMP_TASKS_NAMED_ASK=on` (#1722): a line whose own words ask
    /// one of the staff by name (`named`) has its round 1 call of
    /// `start_agent_task` given to that one, whoever the call named; one
    /// that asks several has each call kept with the asked agent it names,
    /// and only a call naming nobody or one not asked given to an asked
    /// one ([`given_to_the_named`]); and
    /// each line's journal says what became of its named ask, with no
    /// words ([`named_ask_line`]). The chat's first round is made to call
    /// it by `net::Chat::forcing`. Before any turn only.
    pub fn asking_by_name(mut self, named: NamedAsk) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.named_ask = Some(named);
        }
        self
    }

    /// Each turn says in the journal whether `remember` was among the
    /// functions its chat was given, how many calls of each function it
    /// made, and whether its chat begun on the early words stood for round
    /// 1 (`LAMP_MEMORY_REPLACES=on`, #2510): names and counts only. Before
    /// any turn only.
    pub fn noting_turn_tools(mut self, on: bool) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.note_tools = on;
        }
        self
    }

    /// A test's copy of each turn's tools line, as it is said.
    #[cfg(test)]
    fn keeping_turn_tools_journal(mut self, journal: Arc<Mutex<Vec<String>>>) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.tools_journal = Some(journal);
        }
        self
    }

    /// A test's copy of each named-ask journal line, as it is said.
    #[cfg(test)]
    fn keeping_named_ask_journal(mut self, journal: Arc<Mutex<Vec<String>>>) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.named_ask_journal = Some(journal);
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

    /// `LAMP_SECOND_SKILL_LOOK=on` ([`SKILL_LOOK_ENVIRONMENT`]): a call that
    /// `skill` says goes to the skills, answered with `"look": true` (a
    /// skill's look step, #941), asks the coordinator for the turn's full
    /// look as [`LOOK_CALL`] does: the round's other calls are answered,
    /// the lamp says [`LOOKING`] if nothing of the answer was said, and the
    /// turn ends there, with no round for the model to say it will look;
    /// the picture comes in the look turn after it, as on the Live route.
    /// Before any turn only.
    pub fn looking_for_skills(mut self, skill: SkillCall) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.skill_look = Some(skill);
        }
        self
    }

    /// A push on the body during a skill's search (`body/hand`, read into
    /// `pushed` by [`crate::search_pushed::read_from`], #2904) ends it here
    /// as a spoken stop would: no look call of that conversation runs for
    /// [`crate::search_pushed::QUIET_US`] unless the person asks again in
    /// words, and the next request they ask is told
    /// ([`crate::search_pushed::NOTE`]). Before any turn only.
    pub fn stopping_searches_by_hand(
        mut self,
        pushed: Arc<Mutex<crate::search_pushed::SearchPushed>>,
    ) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.search_pushed = Some(pushed);
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

    /// With `LAMP_POLICY_HOLDS_ACTIONS=on` (#800, #1712): each call of a
    /// spoken turn is put to `gate` before it runs, which waits for the
    /// Policy's decision about the turn; a round's calls together, so the
    /// round waits `policy_holds::WAIT` at most. One it holds is not run: the model
    /// is told it was not done ([`crate::policy_holds::HELD`]), the round's
    /// other calls run as they would, and the turn ends there in silence,
    /// as `not_for_me` ends it: nothing more is said, and the model is not
    /// asked again. Before any turn only.
    pub fn holding_for_the_policy(mut self, gate: PolicyGate) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.policy = Some(gate);
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

    /// Each judged turn's verdict is said with `told` (#1155): held back,
    /// or answered when it was judged and let out. Before any turn only.
    pub fn telling_verdicts(mut self, told: Verdicts) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.verdicts = Some(told);
        }
        self
    }

    /// A spoken request is told, beside its words, that the air has been
    /// stuffy a while, when `note` says so (`LAMP_AIR_FACTS=on`,
    /// `crate::air_told`, as the Live route tells it), and `heard` is told
    /// the request once its answer's first audio goes: only then is the
    /// note spent, so a line cut short before any word keeps it (the
    /// reviewer's P3 on #1960). Before any turn only.
    pub fn noting_the_air(mut self, note: TaskNote, heard: AirHeard) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.air = Some((note, heard));
        }
        self
    }

    /// A spoken request from a known person in view, not one held only by
    /// a gap, is told how they seem from their face, when `note` says so
    /// (`LAMP_SECOND_SEEMS=on`, `crate::seems_told`), and `heard` is told
    /// the request once its answer's first audio goes, as for the air
    /// ([`Engine::noting_the_air`]). Before any turn only.
    pub fn noting_how_they_seem(mut self, note: TaskNote, heard: AirHeard) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.seems = Some((note, heard));
        }
        self
    }

    /// The wait sound comes `after` the words, not at [`CUE_AFTER`]
    /// ([`CUE_AFTER_ENVIRONMENT`]). Before any turn only.
    pub fn sounding_after(mut self, after: Duration) -> Self {
        let shared = Arc::get_mut(&mut self.shared);
        debug_assert!(
            shared.is_some(),
            "the wait sound's time is set before any turn"
        );
        if let Some(shared) = shared {
            shared.cue_after = after;
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

    /// A line the rule holds, when its heard words name reports
    /// ([`crate::attention::HELD_ASKED_WORDS`]), is still asked once, told
    /// only the functions done though a line's answer is held, and those
    /// calls are done; its answer stays held and nothing of it is said
    /// (`LAMP_REPORTS=on`, the first builder on #1722, 16:44). Any other
    /// held line is sent nowhere (#1925). Before any turn only.
    pub fn doing_though_held(mut self, on: bool) -> Self {
        let shared = Arc::get_mut(&mut self.shared);
        debug_assert!(shared.is_some(), "set before any turn");
        if let Some(shared) = shared {
            shared.though_held = on;
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
    /// The wait sounds and the down lines rendered now, as the worker
    /// starts, so the first turn after a restart has its "hmm" ready and
    /// the speech connection is warm (#3755). Each renders once; one that
    /// fails is tried again at the next line. In a runtime only.
    pub fn render_lines(&self) {
        if let Some(cues) = &self.shared.cues {
            cues.render(&self.shared.services);
        }
        if let Some(down) = &self.shared.down {
            down.render(&self.shared.services);
        }
    }

    pub fn submit(&mut self, input: ProviderInput) {
        match input {
            ProviderInput::Start {
                request,
                session,
                privacy_generation,
                continues,
                continues_cut_off,
                after_stop,
                follows_on,
                over_the_lamps_voice,
                soft_lead_us,
                ..
            } => {
                let previous = self.begun_last;
                // Begun over nothing of the lamp's but the last request's
                // wait sound: the person going on after a pause, not a
                // cut-in (lamp-4ace on 21 Oct at 18:36, the first builder on
                // #2602). Begun in the clear for the room's talk, the
                // conversation and the cut-in note, but with no voice print:
                // the sound is in its first samples, and on lamp-52e6 (21 Oct
                // 22:14-22:16) one such line of three read 0.17 against its
                // own voice and was held.
                let over_the_wait_sound = over_the_lamps_voice
                    && previous.is_some_and(|(previous, _)| {
                        let only = self.shared.only_its_wait_sound(previous);
                        if only {
                            eprintln!(
                                "lamp-voice second: request {request} began over only request {previous}'s wait sound: taken as begun in the clear"
                            );
                        }
                        only
                    });
                let over_the_lamps_voice = over_the_lamps_voice && !over_the_wait_sound;
                // The coordinator's call just before, its words not back:
                // kept running for them, unless its newer owner's authority
                // came first and kept it already (`LAMP_SECOND_NAME_CARRIES`).
                if request > self.retired_through {
                    self.keep_a_coming_name();
                }
                if !self.begin(request, session) {
                    return;
                }
                // The first line after a stop: what was stopped is never
                // asked again, in its place or joined to it.
                if after_stop {
                    self.let_go_cut(&format!("request {request} is the first line after a stop"));
                    // The rule hears of it before this line is judged.
                    if let Some(did) = &self.shared.did {
                        did(Did::Stopped);
                    }
                }
                // The lamp's name said alone just before, its answer waiting
                // for this line (`LAMP_SECOND_NAME_CARRIES`): begin dropped
                // that request, and this line is said by its name.
                let name_of = self.name_said_before(
                    request,
                    (session, privacy_generation),
                    previous,
                    after_stop,
                );
                // A call kept for its words feeds only the line it waits for.
                if !name_of
                    .as_ref()
                    .is_some_and(|name| matches!(name.words, NameWords::Coming(_)))
                {
                    self.superseded = None;
                }
                room(|room| {
                    room.heard(request, monotonic_us());
                    if over_the_lamps_voice {
                        room.began_over_the_lamp(request);
                    }
                });
                if let Some(hearing) = &self.shared.hearing {
                    hearing(
                        request,
                        if over_the_wait_sound {
                            Heard::BeganOverTheWaitSound
                        } else {
                            Heard::Began {
                                over_the_lamps_voice,
                            }
                        },
                    );
                }
                // The voice's connection opened while the person speaks, not
                // by the answer's first sentence (#1712).
                tokio::spawn(self.shared.services.warm());
                // The down line rendered by a person's first line too, not
                // only after a turn: a lamp started while the relay is down
                // has it for that line (lamp-52e6, #1195's run of 21 Oct).
                // Once only; a turn's chat begins after the line ends, but a line
                // said over the lamp's voice begins here too, and a short line
                // can end before a slow relay's render does: once a start, the
                // render can overlap a turn.
                if let Some(down) = &self.shared.down {
                    down.render(&self.shared.services);
                }
                // The wait sounds too, as the line begins, not only once its
                // words are heard: one rendered by the time the chat is due
                // makes its "hmm" (#3755).
                if let Some(cues) = &self.shared.cues {
                    cues.render(&self.shared.services);
                }
                // A new line is not the sentence unless it is said to be.
                crate::thread::count_only(None);
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
                // The line before's words, for a chain (`LAMP_JOIN_CUT_WORDS`).
                let heard_before = last.clone();
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
                // The piece's words, when they came back before this line
                // began.
                let words_back = |kept: &KeptSound| {
                    last.as_ref()
                        .filter(|(of, _)| *of == kept.request)
                        .map(|(_, words)| words.trim())
                };
                // How long after the line before it, in this session, this
                // one began (`LAMP_JOIN_CUT_WORDS`).
                let after_end = self
                    .kept
                    .as_ref()
                    .filter(|kept| kept.session == session && kept.request < request)
                    .and_then(|kept| {
                        kept.ended
                            .map(|end| (kept.request, now.saturating_duration_since(end)))
                    });
                // The line before's own link, which one its sound or words
                // begin goes on with.
                let before_link = self
                    .kept
                    .as_ref()
                    .filter(|kept| kept.session == session && kept.request < request)
                    .map(|kept| (kept.request, kept.link, kept.began));
                let carried = self.kept.take().filter(|kept| {
                    // The name waiting for this line goes to it only when
                    // one person said both ([`said_by_the_name`]), never
                    // as the kept sound, which asks nothing of who spoke.
                    if name_of
                        .as_ref()
                        .is_some_and(|name| name.request == kept.request)
                    {
                        return false;
                    }
                    let since = kept.ended.map(|end| now.saturating_duration_since(end));
                    // Only the name cut off on its own: a piece that came
                    // back with no words, or with the lamp's name alone. One
                    // whose other words came back carries nothing (#1712,
                    // 18:56). With the next line's socket kept ready
                    // (`LAMP_SECOND_STT_WARM`) the name's piece comes back as
                    // "Lamp", where a socket opened for it heard nothing
                    // (#2002's P1, the first builder at 01:23).
                    let carries = words_back(kept).is_none_or(|words| {
                        words.is_empty() || crate::attention::only_the_name(words)
                    });
                    kept.session == session
                        && kept.request < request
                        && !kept.samples.is_empty()
                        && carries
                        && since.is_none_or(|since| since <= KEEP_AFTER_END)
                        && (continues
                            || since.is_some_and(|since| since <= CARRY_AFTER_END)
                                && answered != Some(kept.request))
                });
                // The name's words, when they came back: they begin this
                // line's, so it is heard and judged whole, and its sound
                // need not go again.
                let name_words = carried
                    .as_ref()
                    .and_then(words_back)
                    .filter(|words| !words.is_empty())
                    .map(str::to_owned);
                let before = match &name_words {
                    Some(words) => Some(words.clone()),
                    None => last
                        .filter(|_| carried.is_none())
                        .filter(|(of, _)| continues_cut_off && previous == Some((*of, session)))
                        .map(|(_, words)| words),
                };
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
                // Its link: the line before's, when it begins with that
                // line's sound or words, else its own.
                let folded = carried.is_some() || before.is_some();
                let (link, began) = match before_link.filter(|_| folded) {
                    Some((_, link, began)) => (link, began),
                    None => (after_end, now),
                };
                // The line before, cut short after its words came back but
                // before it was asked, and not folded into this one: its
                // words go on in the chain (`LAMP_JOIN_CUT_WORDS`).
                // The lamp's name waiting for this line is carried by its
                // own rule, never chained ([`said_by_the_name`]).
                let name_waiting = name_of.as_ref().map(|name| name.request);
                if !folded
                    && !after_stop
                    && let Some((of, words)) = heard_before
                    && name_waiting != Some(of)
                    && let Some((before, before_link, before_began)) = before_link
                    && of == before
                {
                    self.chain_heard(request, (of, words, before_began), before_link);
                }
                let (audio, heard) = mpsc::channel(AUDIO_QUEUE);
                let sound = Arc::new(AtomicBool::new(false));
                let checked = self.sounds_checked.then(|| sound.clone());
                let marks = Arc::new(Marks {
                    cut_in: self.cut_in_told && over_the_lamps_voice && !after_stop,
                    follows_on,
                    sound: sound.clone(),
                    called: AtomicBool::new(false),
                    privacy: privacy_generation,
                    count_only: AtomicBool::new(false),
                    step_call: AtomicBool::new(false),
                    after_end: link,
                    folded,
                    over_the_lamps_voice,
                    began: Some(began),
                    name_of: Mutex::new(name_of),
                    ended_at: Mutex::new(None),
                    words_back: Mutex::default(),
                    name_carried: AtomicBool::new(false),
                    soft_lead_us: AtomicU64::new(soft_lead_us),
                });
                let said = Arc::new(Mutex::new((0, 0)));
                self.kept = Some(match carried {
                    Some(kept) => {
                        // The name heard in the piece counts by its time
                        // alone; its sound goes again only when the name was
                        // not heard, for speech to text to find it.
                        let name_heard = self
                            .shared
                            .name_sounded
                            .as_ref()
                            .is_some_and(|heard| heard(kept.said_from));
                        if let Some(words) = &name_words {
                            eprintln!(
                                "lamp-voice second: request {request} goes on from request {}: its {} words, the lamp's name alone, begin it; its sound is not sent again",
                                kept.request,
                                words.split_whitespace().count()
                            );
                        } else if name_heard {
                            eprintln!(
                                "lamp-voice second: request {request} goes on from request {}: the name heard in it counts; its sound is not sent again",
                                kept.request
                            );
                        } else {
                            eprintln!(
                                "lamp-voice second: request {request} goes on from request {}: its {} ms of sound go first",
                                kept.request,
                                kept.samples.len() / 16
                            );
                            let _ = audio.try_send(LinePart::Sound(
                                kept.samples.iter().copied().collect(),
                                false,
                                0,
                            ));
                        }
                        // The line began with the sound carried when the
                        // name was in it: the name heard there is this
                        // line's. Sound carried with no name heard in it
                        // leaves the line dated from its own first sound,
                        // or a name said at its start reads as said seconds
                        // into it, `said_from` being when the held line
                        // began, not its last 1.5 s (#4226). A name in that
                        // sound still calls the lamp by the words it gives,
                        // and its hearing, come late, counts from up to
                        // `attention::NAME_BEFORE_US` before the line.
                        if (name_words.is_some() || name_heard)
                            && let Ok(mut said) = said.lock()
                        {
                            *said = (kept.said_from, kept.said_from);
                            // Dated from the carried sound, not its own
                            // first: its soft lead no longer says where its
                            // usual start is (#4226).
                            marks.soft_lead_us.store(0, Ordering::Release);
                        }
                        KeptSound {
                            request,
                            ended: None,
                            link,
                            began,
                            ..kept
                        }
                    }
                    None => KeptSound {
                        request,
                        session,
                        samples: VecDeque::new(),
                        said_from: 0,
                        ended: None,
                        link,
                        began,
                    },
                });
                let memory_scope = self.memory_scope(request, session, true);
                let lease = memory_scope.as_ref().map(|scope| scope.lease());
                // The memory's note, asked now while the person speaks, off
                // the loop, so it is in by the line's end (lamp-4ace, 22 Oct
                // 17:45: asked at the end, it held the loop 300 ms and came
                // 60 ms too late).
                if let Some(ahead) = &self.shared.note_ahead {
                    (ahead.begin)(told_scope(&self.shared, lease.as_ref()));
                }
                let task = tokio::spawn(spoken(
                    self.shared.clone(),
                    (
                        SpokenTurn {
                            request,
                            session,
                            privacy_generation,
                            captured: false,
                            enrollment_generation: crate::remember_speaker::generation(),
                        },
                        lease,
                        marks.clone(),
                    ),
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
                    marks,
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
                if let Some(hearing) = &self.shared.hearing {
                    hearing(request, Heard::Samples(&samples));
                }
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
                match audio.try_send(LinePart::Sound(samples, true, read_completed_at_us)) {
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
            // The line has gone quiet for a moment: its words so far are
            // asked for, the stream kept open (`LAMP_SECOND_EARLY_FINALIZE`).
            // A pause that can't be queued only loses the time it saves.
            ProviderInput::Paused { request, .. } => {
                if let Some(audio) = self
                    .turn
                    .as_ref()
                    .filter(|turn| turn.request == request)
                    .and_then(|turn| turn.audio.as_ref())
                {
                    let _ = audio.try_send(LinePart::Pause);
                }
            }
            // Only the lamp's name: told with the line's words, as Gemini's
            // model is told with its End (#1712).
            ProviderInput::Called { request, .. } => {
                if let Some(turn) = self.turn.as_ref().filter(|turn| turn.request == request) {
                    turn.marks.called.store(true, Ordering::Release);
                }
            }
            // Onboarding's sentence (#1803): set before the line's end, read
            // by the turn's task once its words are in. Its sound is not kept,
            // so no later line's speech to text hears it (the reviewer-lead
            // on #2031).
            ProviderInput::CountOnly { request, .. } => {
                self.kept.take_if(|kept| kept.request == request);
                if let Some(turn) = self.turn.as_ref().filter(|turn| turn.request == request) {
                    turn.marks.count_only.store(true, Ordering::Release);
                    crate::thread::count_only(Some(request));
                }
            }
            // Begun while onboarding is under way (#3748): a sleep asked
            // in it is not now.
            ProviderInput::Onboarding { request, .. } => {
                crate::sleep_asked::mark_onboarding(request);
            }
            // Begun while onboarding waits for a call (#3504): read by the
            // turn's task once its words are in.
            ProviderInput::StepCall { request, .. } => {
                if let Some(turn) = self.turn.as_ref().filter(|turn| turn.request == request) {
                    turn.marks.step_call.store(true, Ordering::Release);
                }
            }
            ProviderInput::End {
                request,
                sound,
                cut_in,
                ..
            } => {
                // Before the rule judges the line: a cut-in over the lamp's
                // answer is judged as one (#828).
                if cut_in && let Some((told, _)) = &self.shared.ask_again {
                    told(request);
                }
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
                    if let Ok(mut at) = turn.marks.ended_at.lock() {
                        *at = Some(tokio::time::Instant::now());
                    }
                    // Closing the queue is the end of input for the task.
                    turn.audio = None;
                    if let Some(hearing) = &self.shared.hearing {
                        hearing(request, Heard::Ended);
                    }
                    if let Some(heard) = &self.shared.line_heard
                        && let Ok(said) = turn.said.lock()
                        && said.0 != 0
                    {
                        heard(request, *said);
                    }
                }
            }
            ProviderInput::Written {
                request,
                session,
                text,
                look,
                search,
                quoted,
                ..
            } => {
                // A skill's note for the picture of the look it asked, kept
                // for this session's look turn ([`picture_note`]): taken
                // here, once, as any other turn would let it go.
                let skill_note = self
                    .shared
                    .look_note
                    .lock()
                    .ok()
                    .and_then(|mut note| note.take())
                    .filter(|(of, _)| look && *of == session)
                    .map(|(_, note)| note);
                if !self.begin(request, session) {
                    return;
                }
                // A search's own turn: the search a push may end goes on
                // from now, the walk's looks included (#2904).
                if search {
                    self.shared
                        .search_pushed(|pushed| pushed.went_on(monotonic_us()));
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
                // The skill's note goes after the question, in the same
                // words beside the picture; with no picture it would ask
                // the model about one it was not given, so it is left out.
                let text = match skill_note {
                    Some(note) if picture.is_some() => {
                        eprintln!(
                            "lamp-voice second: request {request}: the skill's note for the picture goes with it"
                        );
                        format!("{text}{PICTURE_NOTE_BEFORE}{note}{PICTURE_NOTE_AFTER}")
                    }
                    Some(_) => {
                        eprintln!(
                            "lamp-voice second: request {request}: no picture, so the skill's note for it is left out"
                        );
                        text
                    }
                    None => text,
                };
                // An add-on's quoted line is never the person's: no one's
                // memory is read or written with it (#2530 part 2).
                let memory_scope = if quoted {
                    eprintln!(
                        "lamp-voice second: request {request} is an add-on's quoted line: asked with no functions, kept out of memory"
                    );
                    // Its exchange never reaches the memory's journal either.
                    crate::remember::keep_out(request);
                    None
                } else {
                    self.memory_scope(request, session, false)
                };
                let lease = memory_scope.as_ref().map(|scope| scope.lease());
                let shared = self.shared.clone();
                let begun = Instant::now();
                let task = tokio::spawn(async move {
                    answer(
                        &shared,
                        (request, lease),
                        text,
                        Asker::Written {
                            looking,
                            search,
                            quoted,
                        },
                        None,
                        picture,
                        Vec::new(),
                        Times::new(begun),
                        None,
                        None,
                    )
                    .await
                });
                self.turn = Some(Turn {
                    request,
                    audio: None,
                    sound: Arc::default(),
                    said: Arc::default(),
                    marks: Arc::default(),
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
                        None,
                        Vec::new(),
                        Times::new(Instant::now()),
                        None,
                        None,
                    )
                    .await
                });
                self.turn = Some(Turn {
                    request,
                    audio: None,
                    sound: Arc::default(),
                    said: Arc::default(),
                    marks: Arc::default(),
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
        // The relay's routes this turn's chat and speech keep to its end are
        // settled now, for every turn begun, spoken, written or a reminder,
        // before it is warmed or asked (#2882, reviewer-two's P2 on #2919).
        self.shared.services.turn_begins();
        self.begun_last = Some((request, session));
        if let Ok(mut begun) = self.shared.begun.lock() {
            *begun = Some((request, session));
        }
        // A skill's note for its picture is for the look turn alone, which
        // takes it before it begins: any other turn lets it go.
        if let Ok(mut note) = self.shared.look_note.lock()
            && let Some((of, _)) = note.take()
        {
            eprintln!(
                "lamp-voice second: request {request} began: the skill's note for session {of}'s picture is let go"
            );
        }
        self.urgent.push_back(ProviderOutput::Started {
            request,
            session,
            waiting_for_barrier: false,
        });
        true
    }

    /// With `LAMP_SAID_AS_HEARD=on`, the words of `cut.request`'s answer
    /// played before a line or a Stop cut it ([`crate::said_as_heard::heard`]),
    /// and how many words it sent in all. `None` when this route sent no
    /// answer for it, or a newer one since.
    pub fn said_as_heard(&self, cut: crate::said_as_heard::HeardOfCut) -> Option<(String, usize)> {
        let said = self.shared.said.lock().ok()?;
        (said.0 == cut.request && !said.1.is_empty()).then(|| {
            (
                crate::said_as_heard::heard(&said.1, cut.samples),
                crate::said_as_heard::words_in(&said.1),
            )
        })
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
            && !self.keep_a_coming_name()
        {
            self.turn = None;
        }
        let retired = self.retired_through;
        self.urgent
            .retain(|event| output_request(event).is_none_or(|r| r > retired));
    }

    /// Requests up to `through` were stopped, not cut by a newer line: the
    /// authority holds no owner (a hand or head stop, a spoken stop, the
    /// mute, a gap, or their turn over). As [`Self::retire_through`], but
    /// the words of one still being answered are let go, not kept for the
    /// next line, and so are words cut short before and not yet taken:
    /// stopped words never come back in a later line's place or joined to
    /// it (a reviewer's P2 on the join, 22 Oct: "Lamp, play some jazz", a
    /// hand stop, then "what time is it?" was asked as one line).
    pub fn stop_through(&mut self, through: u64) {
        let stopped = self
            .shared
            .in_flight
            .lock()
            .ok()
            .and_then(|mut in_flight| in_flight.take_if(|carried| carried.request <= through));
        if let Some(stopped) = stopped {
            eprintln!(
                "lamp-voice second: request {}: stopped before its answer: its words are let go, not kept for the next line",
                stopped.request
            );
        }
        self.let_go_cut("a stop came after it");
        // Nor do its words heard and not yet asked go on in a chain
        // ([`Self::chain_heard`]).
        if let Ok(mut last) = self.shared.heard_last.lock() {
            last.take_if(|(of, _)| *of <= through);
        }
        // A name waiting for the line after it carries to none
        // (`LAMP_SECOND_NAME_CARRIES`): the stopped turn is dropped, never
        // kept for its words.
        if self
            .turn
            .as_ref()
            .is_some_and(|turn| turn.request <= through)
        {
            self.turn = None;
        }
        self.retire_through(through);
        self.superseded = None;
        if let Ok(mut waits) = self.shared.name_waits.lock() {
            waits.take();
        }
    }

    /// With `LAMP_JOIN_CUT_WORDS`: the line `of`, just before `request`,
    /// was cut short after its `words` came back but before it was asked, so
    /// they were in no slot and its turn is over. They go on in the chain:
    /// after the words cut short before it, when it began within
    /// [`JOIN_WITHIN`] of that line's end (`link`), the link to be judged at
    /// the next verdict; else on their own, and older ones are let go.
    /// Never when the words are those already kept or in flight, a line
    /// once asked (its answer may have been heard), nor a held line's.
    fn chain_heard(
        &self,
        request: u64,
        (of, words, began): (u64, String, Instant),
        link: Option<(u64, Duration)>,
    ) {
        let held = self
            .shared
            .held_last
            .lock()
            .ok()
            .is_some_and(|last| *last == Some(of))
            || self
                .shared
                .asked_last
                .lock()
                .ok()
                .is_some_and(|last| last.is_some_and(|last| last >= of));
        if self.shared.same_speaker.is_none() || words.trim().is_empty() || held {
            return;
        }
        // A stop or "never mind" withdraws the chain before it, as it does
        // in a line's own verdict (a reviewer's P2 on the chain, 22 Oct), in
        // any room (reviewer-three's P2, 23 Oct): before the busy room's
        // return below, which only skips the chaining.
        if says_stop(&words) {
            self.let_go_cut(&format!(
                "request {of}, cut short before it was asked, says stop or never mind (LAMP_JOIN_CUT_WORDS)"
            ));
            return;
        }
        // Chained not in a busy room: there a line cut while its verdict waits (the
        // Policy's, up to 2 s) is as likely talk to someone else
        // (reviewer-three, 23 Oct: "…after lunch." then "Lamp, what's on my
        // calendar?" was asked as one line).
        let busy_now = crate::room_told::room()
            .lock()
            .is_ok_and(|mut room| room.at(monotonic_us()) == crate::room_told::Room::Busy);
        if self.shared.busy(of) || busy_now {
            eprintln!(
                "lamp-voice second: request {of}: cut short before it was asked, in a busy room: its words are not chained (LAMP_JOIN_CUT_WORDS)"
            );
            return;
        }
        let Ok(mut cut) = self.shared.cut.lock() else {
            return;
        };
        let in_flight = self
            .shared
            .in_flight
            .lock()
            .ok()
            .is_some_and(|in_flight| in_flight.as_ref().is_some_and(|kept| kept.request >= of));
        if in_flight || cut.as_ref().is_some_and(|kept| kept.request >= of) {
            return;
        }
        let at = began;
        let part = Part {
            request: of,
            text: words,
            at,
            lines: 1,
        };
        let chain = match cut.take() {
            Some(mut chain)
                if chain.said_by == chain.request
                    && link.is_some_and(|(before, gap)| {
                        before == chain.request && gap <= JOIN_WITHIN
                    }) =>
            {
                chain.unchecked.push((chain.request, of));
                chain.parts.push(part);
                chain.request = of;
                chain.said_by = of;
                chain.at = Instant::now();
                chain
            }
            older => {
                if let Some(older) = older {
                    eprintln!(
                        "lamp-voice second: request {}'s words, cut short, are let go: request {of}, cut short before it was asked, did not go on from it (LAMP_JOIN_CUT_WORDS)",
                        older.request
                    );
                }
                Carried::own(of, part.text, at, of)
            }
        };
        eprintln!(
            "lamp-voice second: request {of}: cut short after its words came back, before it was asked; they wait for request {request}'s verdict, a chain of {} lines (LAMP_JOIN_CUT_WORDS)",
            chain.parts.iter().map(|part| part.lines).sum::<u16>()
        );
        *cut = Some(chain);
    }

    /// Words cut short and not yet taken are let go, and the journal says
    /// `why`.
    fn let_go_cut(&self, why: &str) {
        if let Some(cut) = self.shared.cut.lock().ok().and_then(|mut cut| cut.take()) {
            eprintln!(
                "lamp-voice second: request {}'s words, cut short, are let go: {why}",
                cut.request
            );
        }
    }

    /// The turn in hand, when it is the coordinator's call (`Called`) that
    /// ended within [`NAME_CARRIES_FOR`] and its words are not back yet
    /// (`LAMP_SECOND_NAME_CARRIES`): kept running, not dropped, so its words
    /// go to the line begun after it once they come ([`NameWords::Coming`]);
    /// it says and keeps nothing itself. Its newer owner's authority
    /// retires it first, or that line's start does. Whether it was kept.
    fn keep_a_coming_name(&mut self) -> bool {
        if !self.shared.name_carries {
            return false;
        }
        let Some(turn) = self.turn.as_ref() else {
            return false;
        };
        let Some((request, session)) = self.begun_last.filter(|(begun, _)| *begun == turn.request)
        else {
            return false;
        };
        let Some(ended) = turn.marks.ended_at.lock().ok().and_then(|at| *at) else {
            return false;
        };
        let until = ended + NAME_CARRIES_FOR;
        if !turn.marks.called.load(Ordering::Acquire) || tokio::time::Instant::now() > until {
            return false;
        }
        let (tell, words) = tokio::sync::oneshot::channel();
        {
            let Ok(mut back) = turn.marks.words_back.lock() else {
                return false;
            };
            if back.0 {
                return false;
            }
            back.1 = Some(tell);
        }
        let wait = NameWait {
            request,
            session,
            privacy: turn.marks.privacy,
            words: NameWords::Coming(words),
            until,
            said_from: turn.said.lock().map_or(0, |said| said.0),
        };
        if let Ok(mut waits) = self.shared.name_waits.lock() {
            *waits = Some(wait);
        }
        if let Some(mut kept) = self.turn.take() {
            // It answers nothing now: its memory scope goes.
            kept.memory_scope.take();
            self.superseded = Some(kept);
        }
        eprintln!(
            "lamp-voice second: request {request}: the coordinator's call, its words not back yet, is kept for them until 2.5 s after its end: the next line may be said by it (LAMP_SECOND_NAME_CARRIES)"
        );
        true
    }

    /// The line that was only the lamp's name, begun just before `request`
    /// and still waiting for the line after it (`LAMP_SECOND_NAME_CARRIES`):
    /// `request` is said by it. Any start takes the wait; one in another
    /// session or privacy generation, after a stop, past the wait, or not
    /// begun just after the name leaves it dropped.
    fn name_said_before(
        &self,
        request: u64,
        (session, privacy): (u64, u64),
        previous: Option<(u64, u64)>,
        after_stop: bool,
    ) -> Option<NameWait> {
        let wait = self.shared.name_waits.lock().ok()?.take()?;
        let carries = previous == Some((wait.request, wait.session))
            && wait.session == session
            && wait.privacy == privacy
            && !after_stop
            && tokio::time::Instant::now() <= wait.until;
        if !carries {
            eprintln!(
                "lamp-voice second: request {request}: request {}'s name is not carried: begun past its wait, in another session or after a stop (LAMP_SECOND_NAME_CARRIES)",
                wait.request
            );
        }
        carries.then_some(wait)
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
            if self.current_output(&event) {
                return Some(event);
            }
        }
    }

    /// `event`, as if the engine had made it, for a test of what the worker
    /// does with its output.
    #[cfg(test)]
    pub(crate) fn queue_for_test(&mut self, event: ProviderOutput) {
        self.urgent.push_back(event);
    }

    /// The next output, waiting for it.
    #[cfg(test)]
    async fn next(&mut self) -> ProviderOutput {
        loop {
            let event = match self.urgent.pop_front() {
                Some(event) => event,
                None => self.output.recv().await.expect("the engine holds a sender"),
            };
            if self.current_output(&event) {
                return event;
            }
        }
    }

    fn current_output(&self, event: &ProviderOutput) -> bool {
        output_request(event).is_none_or(|r| r > self.retired_through)
            && match event {
                ProviderOutput::RequestedAudio {
                    request,
                    session,
                    offer,
                } => {
                    self.begun_last == Some((*request, *session))
                        && self.shared.scoped_tools.as_ref().is_none_or(|_| {
                            self.turn
                                .as_ref()
                                .and_then(|turn| turn.memory_scope.as_ref())
                                .is_some_and(|scope| {
                                    audio_scope_current(&scope.lease(), &offer.context)
                                })
                        })
                }
                _ => true,
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
        | ProviderOutput::RequestedAudio { request, .. }
        | ProviderOutput::Unclear { request, .. }
        | ProviderOutput::LookAsked { request, .. }
        | ProviderOutput::LookNoCall { request, .. }
        | ProviderOutput::WordsEnd { request, .. }
        | ProviderOutput::NewsAnswered { request, .. }
        | ProviderOutput::SleepAsked { request }
        | ProviderOutput::StepCallHeard { request }
        | ProviderOutput::OwnWordsSaid { request } => Some(*request),
        ProviderOutput::Transcript { request, .. } => *request,
    }
}

/// The closest provider fault for a client error. No text is carried.
/// The down line ([`DOWN_LINE`], or [`DOWN_LINE_AGAIN`] within the minute),
/// when it is switched on and kept: as audio only, so it is no answer in the
/// thread. Only to someone who spoke: a lamp's own turn (a hello, a skill's
/// say) never says it (the reviewer on #1195).
async fn say_down(shared: &Shared, request: u64, heard: bool) -> Result<(), Stop> {
    if let Some((down, clip)) = shared
        .down
        .as_ref()
        .filter(|_| heard)
        .and_then(|down| Some((down, down.due()?)))
    {
        let (clip, again) = clip;
        cue(shared, request, clip).await?;
        down.said(again);
        eprintln!(
            "lamp-voice second: request {request} said its thoughts are slow{}",
            if again {
                " (again, the shorter line)"
            } else {
                ""
            }
        );
    }
    Ok(())
}

/// [`DOWN_LINE_CANNOT`], when switched on, kept and due, to someone who
/// spoke only, as [`say_down`].
async fn say_cannot(shared: &Shared, request: u64, heard: bool) -> Result<(), Stop> {
    if let Some((down, clip)) = shared
        .down
        .as_ref()
        .filter(|_| heard)
        .and_then(|down| Some((down, down.cannot_due()?)))
    {
        cue(shared, request, clip).await?;
        down.cannot_said();
        eprintln!("lamp-voice second: request {request} said it can't answer just now");
    }
    Ok(())
}

/// How often a lost line's wait asks whether the link is back.
const LINK_BACK_EVERY: Duration = Duration::from_millis(250);

/// Whether the link is back (`back`) within `resend::RESEND_WITHIN_US` of
/// now, with no newer line begun meanwhile (`begun` no longer `request`):
/// asked every [`LINK_BACK_EVERY`], each ask bounded by what is left.
async fn link_back_in_time(shared: &Shared, request: u64, back: &LinkBack) -> bool {
    let until =
        tokio::time::Instant::now() + Duration::from_micros(crate::resend::RESEND_WITHIN_US);
    let newer = || {
        shared
            .begun
            .lock()
            .ok()
            .and_then(|begun| *begun)
            .is_some_and(|(of, _)| of != request)
    };
    loop {
        if newer() {
            eprintln!(
                "lamp-voice second: request {request} lost on the network: a newer line has begun, so it is not asked again"
            );
            return false;
        }
        if tokio::time::timeout_at(until, back()).await == Ok(true) {
            return !newer();
        }
        if tokio::time::Instant::now() + LINK_BACK_EVERY >= until {
            eprintln!(
                "lamp-voice second: request {request} lost on the network: the link was not back within {} s, so it is not asked again",
                crate::resend::RESEND_WITHIN_US / 1_000_000
            );
            return false;
        }
        tokio::time::sleep(LINK_BACK_EVERY).await;
    }
}

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

    /// `then` with the search a push may end ([`Engine::stopping_searches_by_hand`]),
    /// if there is one.
    fn search_pushed<T>(
        &self,
        then: impl FnOnce(&mut crate::search_pushed::SearchPushed) -> T,
    ) -> Option<T> {
        let pushed = self.search_pushed.as_ref()?;
        Some(then(
            &mut pushed
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        ))
    }

    async fn fail(&self, request: u64, stage: FailedStage, error: &net::Error) {
        self.emit(ProviderOutput::TurnFailed {
            request,
            stage,
            reason: fault(error),
        })
        .await;
    }

    /// When `samples` more of the lamp's words sent at `now_us` will have
    /// been played, after those sent before them ([`Did::Said`]).
    fn said_through(&self, samples: usize, now_us: u64) -> u64 {
        let length_us = samples as u64 * 1_000_000 / u64::from(net::TTS_SAMPLE_RATE);
        let mut until = 0;
        let _ = self
            .said_until_us
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |before| {
                until = before.max(now_us) + length_us;
                Some(until)
            });
        until
    }

    /// A piece of `request`'s answer begins: its words, none for a wait
    /// sound. The first of a newer request's starts its record afresh.
    fn said_piece(&self, request: u64, words: Option<&str>) {
        // Any piece but the wait sound, which marks itself after this.
        if let Ok(mut only) = self.wait_sound_only.lock() {
            *only = None;
        }
        if let Ok(mut said) = self.said.lock() {
            if said.0 != request {
                *said = (request, Vec::new(), crate::not_speech::CallsLeftOut::new());
            }
            // A call written in the answer is left out of the thread
            // (`Thread::lamp_for`), so its words are left out here too, on
            // the same text: its sound still counts (the reviewer's P3 on
            // #2145).
            let words =
                words.filter(|words| said.2.keeps(request, &format!("{} ", words.trim_start())));
            said.1.push(crate::said_as_heard::Piece {
                words: words.map(str::to_owned),
                samples: 0,
            });
        }
    }

    /// Whether all the lamp has sounded for `request` is its wait sound,
    /// sounding still or not.
    fn only_its_wait_sound(&self, request: u64) -> bool {
        self.wait_sound_only
            .lock()
            .is_ok_and(|only| *only == Some(request))
    }

    /// `samples` more of `request`'s newest piece sent.
    fn said_samples(&self, request: u64, samples: usize) {
        if let Ok(mut said) = self.said.lock()
            && said.0 == request
            && let Some(piece) = said.1.last_mut()
        {
            piece.samples += samples as u64;
        }
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
    context: (
        SpokenTurn,
        Option<crate::memory_scope::ScopeLease>,
        Arc<Marks>,
    ),
    mut heard: mpsc::Receiver<LinePart>,
    checked: Option<Arc<AtomicBool>>,
    said: Arc<Mutex<(u64, u64)>>,
    before: Option<String>,
    continues_from: Option<u64>,
) {
    let (mut turn, scope, marks) = context;
    let request = turn.request;
    // Known from its start, so a chat begun on its early words is asked
    // with it too.
    let cut_in: Vec<String> = marks
        .cut_in
        .then(|| crate::cut_in_told::NOTE.to_owned())
        .into_iter()
        .collect();
    let begun = Instant::now();
    // The listener kept ready for this line, if one is, of this privacy
    // generation, and still open; otherwise its own, opened now. One found
    // closed is dropped without a word.
    let kept = shared.spare.lock().ok().and_then(|mut spare| spare.take());
    let kept = match kept {
        Some(kept) if kept.generation == marks.privacy => match kept.opening.await {
            Ok(Ok(listener)) if listener.open() => Some(listener),
            _ => None,
        },
        Some(kept) => {
            kept.opening.abort();
            None
        }
        None => None,
    };
    let from = if kept.is_some() {
        "kept ready"
    } else {
        "opened"
    };
    let connected = match kept {
        Some(listener) => Ok(listener),
        None => shared.services.listen().await,
    };
    let mut listener = match connected {
        Ok(listener) => listener,
        Err(error) => {
            shared.stages(request, &Times::new(begun)).await;
            return shared.fail(request, FailedStage::InputOpen, &error).await;
        }
    };
    // How long the line's sound waited for it, in its 10 ms blocks.
    eprintln!(
        "lamp-voice second: request {request}: speech to text {from} in {} ms, {} ms of the line's sound waiting",
        begun.elapsed().as_millis(),
        heard.len() * 10
    );
    // And the next line's, opened while this one is heard.
    if shared.keep_listener {
        keep_a_listener(&shared, marks.privacy);
    }
    // The words before they are final, for a chat begun on them.
    let mut early = if shared.prepare {
        listener.early()
    } else {
        None
    };
    let mut prepared: Option<Prepared> = None;
    // Whether the words so far end unfinished, told to the coordinator as
    // it changes, while the line is heard (`LAMP_SECOND_UNFINISHED_WAIT`).
    let mut words_said = if shared.unfinished_words {
        listener.said()
    } else {
        None
    };
    let mut told_unfinished = None;
    let mut enrollment_audio = None;
    let mut enrollment_audio_valid = true;
    loop {
        tokio::select! {
            Some(words) = next_said(&mut words_said) => {
                let unfinished = crate::unfinished_words::trailing(&words);
                if unfinished != told_unfinished {
                    told_unfinished = unfinished;
                    shared.emit(ProviderOutput::WordsEnd { request, unfinished }).await;
                }
            }
            part = heard.recv() => match part {
                None => break,
                Some(LinePart::Sound(samples, fresh, read_completed_at_us)) => {
                    turn.captured |= fresh && !samples.is_empty();
                    if shared.remember_speaker {
                        // The microphone clock, not STT's delayed queue:
                        // 16 kHz samples, timestamped when the read ended.
                        let duration_us = (samples.len() as u64).saturating_mul(1_000_000)
                            / u64::from(lamp_audio::CAPTURE_RATE);
                        let began_us = read_completed_at_us.checked_sub(duration_us);
                        match (enrollment_audio, began_us) {
                            (_, _) if !fresh || samples.is_empty() => enrollment_audio_valid = false,
                            (None, Some(began)) => enrollment_audio = Some((began, read_completed_at_us)),
                            (Some((began, end)), Some(_)) if read_completed_at_us > end => {
                                enrollment_audio = Some((began, read_completed_at_us));
                            }
                            _ => enrollment_audio_valid = false,
                        }
                    }
                    if let Err(error) = listener.send(&samples) {
                        shared.stages(request, &Times::new(begun)).await;
                        return shared.fail(request, FailedStage::InputOpen, &error).await;
                    }
                }
                // Only time is lost if it can't go: the End asks anyway.
                Some(LinePart::Pause) => {
                    if listener.flush().is_ok() {
                        eprintln!("lamp-voice second: request {request}: a pause: its words so far are asked for");
                    }
                }
            },
            Some(said) = next_early(&mut early) => {
                prepared = prepared_on(&shared, (request, scope.as_ref()), prepared, said, (&cut_in, &marks));
            }
        }
    }
    let finishing = Instant::now();
    let times = Times::new(finishing);
    // A line admitted as the room's with the camera allowed (#2807): the
    // known face the cache names as of its end is looked for from the end
    // itself, not once the words are back, which on a lamp is often past the
    // end plus `KEEPER_WAIT` (reviewer-three's P2 on #3108). Whether it is
    // armed (the cache cold at its start, or gone cold by the end plus
    // `KEEPER_WAIT`: on lamp-c0a4 the eyes' known file changed mid-line) is
    // decided there too; with the eyes' view (their passing id beside the
    // known file, `LAMP_EYES_SPEAKING=on`) it keeps only for a face they saw
    // in view as the line began, cold at its start or not: the known file
    // also changes when a known person walks in (El's and reviewer-three's
    // P2s on #3108). Without the eyes' view, a line armed mid-line keeps for
    // nobody, and one begun cold only for a face named within one refresh
    // of its start. Who was seen speaking it is checked once the words are
    // in.
    let late_face = scope
        .as_ref()
        .filter(|scope| scope.may_look_later())
        .map(|lease| {
            let ends = lease.clone();
            let sight = shared.forget_sight.clone();
            tokio::task::spawn_blocking(move || match &sight {
                Some((eyes, known)) => ends.face_at_end_seen(
                    finishing,
                    Some(&|person: &str, since: Instant| {
                        eyes.known_in_view_since(known, person, since)
                    }),
                ),
                None => ends.face_at_end(finishing),
            })
        });
    let finish = listener.finish(request);
    tokio::pin!(finish);
    let finished = loop {
        tokio::select! {
            finished = &mut finish => break finished,
            Some(said) = next_early(&mut early) => {
                prepared = prepared_on(&shared, (request, scope.as_ref()), prepared, said, (&cut_in, &marks));
            }
        }
    };
    // The service's fillers were asked for the end's judgement alone
    // (`LAMP_SECOND_UNFINISHED_WAIT`, #3126): nothing after it reads them.
    let finished = if shared.unfinished_words {
        finished.map(|words| crate::unfinished_words::without_fillers(&words))
    } else {
        finished
    };
    // Every early word sent before the words were final counts: a
    // `TurnResumed` just before the end still takes them back.
    while let Some(said) = early.as_mut().and_then(|early| early.try_recv().ok()) {
        prepared = prepared_on(
            &shared,
            (request, scope.as_ref()),
            prepared,
            said,
            (&cut_in, &marks),
        );
    }
    // The coordinator's call, its words back after the next line began
    // (`LAMP_SECOND_NAME_CARRIES`, [`NameWords::Coming`]): they go to that
    // line, kept for whom it was said by, and nothing of this one is said,
    // told or kept.
    let superseded = marks.words_back.lock().ok().and_then(|mut back| {
        back.0 = true;
        back.1.take()
    });
    if let Some(tell) = superseded {
        // Only within its 2.5 s: words later than that carry nothing.
        let in_time = marks
            .ended_at
            .lock()
            .ok()
            .and_then(|at| *at)
            .is_some_and(|at| tokio::time::Instant::now() <= at + NAME_CARRIES_FOR);
        let back = finished.as_ref().ok().filter(|_| in_time).cloned();
        if back.is_some()
            && let Some((_, kept)) = &shared.one_speaker
        {
            kept(request, said.lock().map_or((0, 0), |said| *said));
        }
        eprintln!(
            "lamp-voice second: request {request}: {} after the next line began, and nothing of this line is said (LAMP_SECOND_NAME_CARRIES)",
            match (&finished, in_time) {
                (Ok(words), true) => format!(
                    "its {} words came back: they go to it",
                    words.split_whitespace().count()
                ),
                (Ok(_), false) => "its words came back past 2.5 s from its end".to_owned(),
                (Err(_), _) => "its speech to text failed".to_owned(),
            }
        );
        let _ = tell.send(back.map(|words| (words, true)));
        return;
    }
    let text = match finished {
        Ok(text) => {
            reached(&times, |times| &mut times.words);
            eprintln!(
                "lamp-voice second: request {request} heard {} words, the last {} ms after the line ended",
                text.split_whitespace().count(),
                finishing.elapsed().as_millis()
            );
            tell_whose_words(&shared, request, &text);
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
    // Snapshot the actual line before name carries or older words are
    // joined. Those may belong to someone else and never prove this name.
    let enrollment_allowed = shared.remember_speaker
        && !marks.count_only.load(Ordering::Acquire)
        && scope.as_ref().is_some_and(|lease| lease.is_current());
    let enrollment = if enrollment_allowed && enrollment_audio_valid {
        shared.enrollment_sight.as_ref().and_then(|eyes| {
            let (began_us, ended_us) = enrollment_audio?;
            let now_us = monotonic_us();
            let now = Instant::now();
            let from = now.checked_sub(Duration::from_micros(now_us.checked_sub(began_us)?))?;
            let to = now.checked_sub(Duration::from_micros(now_us.checked_sub(ended_us)?))?;
            let face = eyes.enrollment_speaker(from, to)?;
            crate::remember_speaker::Proof::heard(
                face,
                began_us,
                ended_us,
                &text,
                turn.enrollment_generation,
            )
        })
    } else {
        None
    };
    if enrollment_allowed && let Some(lease) = scope.as_ref() {
        crate::speaking_faces::saw_line_scoped(
            crate::speaking_faces::LineSeen {
                request,
                enrollment: enrollment.clone(),
                ..crate::speaking_faces::LineSeen::default()
            },
            lease,
        );
    }
    // Seeing someone speak: a line admitted as the known person's that any
    // other face spoke is the room's, before anything is told or filed with
    // it. Only ever narrowed (the first builder, #1722 00:23).
    // With `LAMP_PERSON_SCOPE_SEEN`, no eyes to read is no face seen
    // speaking it: the room's (reviewer-two's P2 on #3437).
    let mut named = false;
    if let Some(scope) = scope.as_ref()
        && let Some(person) = scope
            .scope()
            .and_then(|scope| scope.person())
            .map(str::to_owned)
        && let Some(why) = match &shared.speaking {
            Some((eyes, known)) => {
                let (speakers, pair, alone) =
                    line_sight(eyes, known, begun, finishing, shared.scope_seen).await;
                // Their face alone in view, none seen speaking it: the
                // room's scope, but its reply told their name (lamp-3e83;
                // reviewer-three's P2 on #4090).
                named = shared.scope_seen
                    && crate::speaking_faces::named_alone(&speakers, &person, pair.as_ref(), alone);
                let known_face = pair.as_ref().map(|(_, face)| face.clone());
                let theirs =
                    !crate::speaking_faces::not_theirs(&speakers, &person, pair, shared.scope_seen);
                let note = crate::speaking_faces::seen_speaking_note(
                    &speakers,
                    theirs,
                    known_face.as_deref(),
                )
                // With no face seen speaking it, what the eyes' looks held, in
                // counts (lamp-29e7: 18 of 20 lines so, nothing to say why).
                .map(|note| {
                    if speakers.is_empty() && shared.scope_seen {
                        format!(
                            "{note} [{}]",
                            eyes.sight_told(known_face.as_deref(), begun, finishing)
                        )
                    } else {
                        note
                    }
                });
                // Kept as theirs: one line saying whose face it was, so a
                // journal with two faces in view tells which one it went to.
                if theirs && let Some(note) = &note {
                    eprintln!(
                        "lamp-voice second: request {request}: {note}: told and kept as theirs"
                    );
                }
                note.filter(|_| !theirs)
            }
            None => shared
                .scope_seen
                .then(|| "no eyes read who spoke it (LAMP_PERSON_SCOPE_SEEN)".to_owned()),
        }
    {
        scope.narrow_to_room();
        if named {
            scope.name_for_reply();
            eprintln!(
                "lamp-voice second: request {request}: {why}, the known person's face alone in view: kept as the room's, its reply told their name"
            );
        } else {
            eprintln!("lamp-voice second: request {request}: {why}: told and kept as the room's");
        }
        if prepared.take().is_some() {
            eprintln!(
                "lamp-voice second: request {request}: the chat begun on its early words is dropped: it was told as the person's"
            );
        }
    }
    // A late look's keeper (#2807): the known face named by the line's end
    // (looked for from `finishing`, above), waiting at most `KEEPER_WAIT`
    // past it, and only if no other face was seen speaking it (the same
    // evidence and rule as the guard above, Eighth's P1 on #3108). Decided
    // beside the turn, never delaying it, and recorded for the keep
    // (`ScopeLease::keeper_within`).
    if let (Some(lease), Some(late_face)) = (scope.as_ref(), late_face) {
        let lease = lease.clone();
        let speaking = shared.speaking.clone();
        let seen_only = shared.scope_seen;
        tokio::spawn(async move {
            let found = late_face.await.ok().flatten();
            let keeper = match (found, speaking) {
                (Some(person), Some((eyes, known))) => {
                    let (speakers, pair, _) =
                        line_sight(&eyes, &known, begun, finishing, seen_only).await;
                    if crate::speaking_faces::not_theirs(&speakers, &person, pair, seen_only) {
                        eprintln!(
                            "lamp-voice second: request {request}: {}: its keep stays the room's",
                            if speakers.is_empty() {
                                "no face was seen speaking it (LAMP_PERSON_SCOPE_SEEN)"
                            } else {
                                "a face not the known face's was seen speaking it"
                            }
                        );
                        None
                    } else {
                        Some(person)
                    }
                }
                (Some(_), None) if seen_only => {
                    eprintln!(
                        "lamp-voice second: request {request}: no eyes read who spoke it (LAMP_PERSON_SCOPE_SEEN): its keep stays the room's"
                    );
                    None
                }
                (found, _) => found,
            };
            lease.record_keeper(keeper);
        });
    }
    // Who the eyes saw saying this line, with the known person and their
    // face as they were when it was heard: for a forget held for a yes
    // ([`take_held_forget`], #2377), and for a line asking to forget, whose
    // "forget my face" on a room's line forgets only a face seen saying it
    // (`known_faces::forget_name`, El's P2 on #3018). Read only then.
    if let Some((eyes, known)) = &shared.forget_sight {
        let held = shared
            .held_forget
            .lock()
            .is_ok_and(|hold| hold.held.is_some());
        let asks = crate::remember::asks_to_forget(&text);
        if held || asks {
            // Each id with the eyes' boot, the known face's too: after the
            // eyes start again a passing id names someone else (#3037).
            let speakers = eyes.speakers_with_boot(begun, finishing);
            let (eyes, known) = (eyes.clone(), known.clone());
            let pair = tokio::task::spawn_blocking(move || eyes.known_with_boot(&known))
                .await
                .ok()
                .flatten();
            // And with `LAMP_MEMORY_ROOM_SPEAKER`, for who said it (#2849).
            if asks || crate::remember::room_speaker_on() {
                let seen = crate::speaking_faces::LineSeen {
                    request,
                    speakers: speakers.clone(),
                    known: pair.clone(),
                    enrollment: enrollment.clone(),
                };
                if shared.remember_speaker {
                    if let Some(lease) = scope.as_ref() {
                        crate::speaking_faces::saw_line_scoped(seen, lease);
                    }
                } else {
                    crate::speaking_faces::saw_line(seen);
                }
            }
            if held && let Ok(mut hold) = shared.held_forget.lock() {
                hold.sight = Some((request, speakers, pair));
            }
        } else if crate::remember::room_speaker_on() {
            // Who said a room line (#2849, `LAMP_MEMORY_ROOM_SPEAKER`): the
            // same reading, kept for the line's own writes (its exchange,
            // a remember it asks for), never waited for: the known file is
            // read beside the turn, and a line whose reading isn't in yet
            // when it is written simply has no speaker.
            let speakers = eyes.speakers_with_boot(begun, finishing);
            let (eyes, known) = (eyes.clone(), known.clone());
            let enrollment = enrollment.clone();
            let lease = scope.clone();
            let remember_speaker = shared.remember_speaker;
            drop(tokio::task::spawn_blocking(move || {
                publish_room_line(
                    crate::speaking_faces::LineSeen {
                        request,
                        speakers,
                        known: eyes.known_with_boot(&known),
                        enrollment,
                    },
                    remember_speaker,
                    lease.as_ref(),
                );
            }));
        }
    }
    // Onboarding's sentence (#1803): its words go to the coordinator to
    // be counted, and nowhere else. The model is never asked, so there is
    // no answer to it to keep or say, and no later request begins with it:
    // not its words, nor its sound (dropped at `CountOnly`).
    if marks.count_only.load(Ordering::Acquire) {
        drop(prepared.take());
        if shared
            .emit(ProviderOutput::Transcript {
                request: None,
                text,
                finished: true,
            })
            .await
        {
            shared
                .emit(ProviderOutput::TurnComplete {
                    request,
                    idle: true,
                })
                .await;
        }
        return;
    }
    // Onboarding's call (#3504): begun while `how_you_sound` waits for the
    // lamp's name, a line that is only the name (or the name heard and no
    // words came back) is the step's, measured from `name/heard`. It is not
    // a turn: no chat is asked, nothing is answered, and it carries to no
    // later line. On lamp-52e6 each such "Lamp" was answered, and the answer
    // cut the walk's next prompt. Any other words go on as a turn.
    if marks.step_call.load(Ordering::Acquire)
        && before
            .as_ref()
            .is_none_or(|before| before.trim().is_empty())
        && (name_alone(&text) || (text.trim().is_empty() && marks.called.load(Ordering::Acquire)))
    {
        drop(prepared.take());
        eprintln!(
            "lamp-voice second: request {request}: only the lamp's name, while how_you_sound waits for a call: left to the step, not answered"
        );
        // Its words were the name (#3923): the step counts it as a call
        // when the detector missed it. A line with no words, which only the
        // detector's call made one, is not told: the detector heard it.
        let worded = name_alone(&text);
        if !worded || shared.emit(ProviderOutput::StepCallHeard { request }).await {
            shared
                .emit(ProviderOutput::TurnComplete {
                    request,
                    idle: true,
                })
                .await;
        }
        return;
    }
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
    // Only current captured words authorize a recording: neither a carried
    // prefix nor the synthetic name below can supply a new spoken identity.
    turn.captured &= !text.trim().is_empty();
    // What was heard in this request is told as heard; the model is asked
    // with the words of the one it goes on from before them.
    let went_on = before
        .as_ref()
        .is_some_and(|before| !before.trim().is_empty());
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
    // Only the lamp's name, and its words came back empty, as a lone name
    // often does: asked as the name the coordinator heard, or it would go
    // unanswered as a sound (#1712).
    let called = marks.called.load(Ordering::Acquire);
    let text = if called && text.trim().is_empty() {
        eprintln!(
            "lamp-voice second: request {request}: only the lamp's name was said, and no words of it came back: asked as the name"
        );
        CALLED_WORDS.to_owned()
    } else {
        text
    };
    // A first word set off and sounding like the lamp's name, which
    // lamp-name heard near at the line's start, is the name
    // (`LAMP_NAME_NEAR`): both signs, never one. Not when the line begins
    // with an earlier piece's words, whose start is not this line's
    // (reviewer-three's P3 on 194accd24).
    let text = match &shared.name_near {
        Some(near) if !went_on => named_near(
            request,
            text,
            said.lock().map_or((0, 0), |said| *said),
            near,
        ),
        _ => text,
    };
    // Said by the lamp's name just before, whose request this line dropped
    // (`LAMP_SECOND_NAME_CARRIES`), when one person said both: the name's
    // words go before its own when they begin with the name, else "Lamp, "
    // does, unless its words begin with the name already; with no words of
    // its own, it is the name's line. Otherwise it is judged as its own.
    let name_of = match marks.name_of.lock().ok().and_then(|mut name| name.take()) {
        Some(name) => {
            let own = said.lock().map_or((0, 0), |said| *said);
            said_by_the_name(&shared, request, name, own).await
        }
        None => None,
    };
    let wordless = text.trim().is_empty();
    if name_of.is_some() {
        marks.name_carried.store(true, Ordering::Release);
    }
    let (text, called) = match &name_of {
        Some(name) if wordless && name_first(&name.words) => (
            name.words.clone(),
            called || name.called || name_alone(&name.words),
        ),
        Some(_) if wordless => (CALLED_WORDS.to_owned(), true),
        Some(_) if crate::attention::begins_with_the_name(&text) => (text, called),
        Some(name) if name_first(&name.words) => {
            (format!("{} {}", name.words.trim(), text.trim()), called)
        }
        Some(_) => (format!("{}, {}", net::key_terms::NAME, text.trim()), called),
        None => (text, called),
    };
    // Only the lamp's name, short and begun with it, or the coordinator
    // heard the name and nothing more (`Called`; the words may be a name
    // misheard, "Life."): its answer waits for the line after it, which
    // drops this request; with none, the name alone is answered as a call,
    // a short line as it is (`LAMP_SECOND_NAME_CARRIES`). A line said by a
    // name waits no more.
    let name_waits = shared.name_carries
        && name_of.is_none()
        && !whole_already(&text)
        && (called || name_first(&text));
    if name_waits {
        let left = NAME_CARRIES_FOR.saturating_sub(finishing.elapsed());
        if called || name_alone(&text) {
            eprintln!(
                "lamp-voice second: request {request}: only the lamp's name: its answer waits for the line after it (LAMP_SECOND_NAME_CARRIES)"
            );
        } else {
            eprintln!(
                "lamp-voice second: request {request}: its {} words begin with the lamp's name and stop short: its answer waits for the line after it (LAMP_SECOND_NAME_CARRIES)",
                text.split_whitespace().count()
            );
        }
        let window = said.lock().map_or((0, 0), |said| *said);
        // Kept for whom the line after it is said by, before either is
        // judged: this one never is if one comes.
        if let Some((_, kept)) = &shared.one_speaker {
            kept(request, window);
        }
        let wait = NameWait {
            request,
            session: turn.session,
            privacy: turn.privacy_generation,
            words: NameWords::Known((text.clone(), called)),
            until: tokio::time::Instant::now() + left,
            said_from: window.0,
        };
        if let Ok(mut waits) = shared.name_waits.lock() {
            *waits = Some(wait);
        }
        tokio::time::sleep(left).await;
        // Nothing began in it: the wait, and the name's carry, are over.
        if let Ok(mut waits) = shared.name_waits.lock() {
            waits.take_if(|wait| wait.request == request);
        }
        eprintln!(
            "lamp-voice second: request {request}: no line came after its name: answered as {}",
            if called || name_alone(&text) {
                "a call"
            } else {
                "it is"
            }
        );
    }
    // The name alone is answered as a call: the name for one word misheard,
    // else its words, told it was a call as without the switch. A short
    // line begun with the name is answered as it is.
    let (text, called) = match name_waits {
        true if !name_first(&text) && past_a_greeting(&text).len() <= 1 => {
            (CALLED_WORDS.to_owned(), true)
        }
        true => {
            let alone = name_alone(&text);
            (text, called || alone)
        }
        false => (text, called),
    };
    // A line already whole ([`whole_already`]: a question, a stop, or a
    // sentence past its first word) is never only the name: it is asked as
    // it is, not told it was a call and answered with "Yes?" (lamp-4ace,
    // 25 Oct: the name heard in "Nice. And what was my sister's name
    // again?" was told as a call, and the model answered "May.").
    let called = called && !whole_already(&text);
    // Its words end before a name: answered no sooner than
    // [`OPEN_NAME_WITHIN`] after the line's end, so the name said next goes
    // on from it (`LAMP_SECOND_OPEN_NAME`). A newer line drops this request,
    // and this wait with it.
    if shared.open_name && crate::known_faces::opens_a_name(&text) {
        let left = OPEN_NAME_WITHIN.saturating_sub(finishing.elapsed());
        eprintln!(
            "lamp-voice second: request {request}: its words end before a name: its answer waits {} ms for it (LAMP_SECOND_OPEN_NAME)",
            left.as_millis()
        );
        tokio::time::sleep(left).await;
        eprintln!("lamp-voice second: request {request}: no name came after it: answered as it is");
    }
    let empty = text.trim().is_empty();
    // Only the lamp's name, or said by it (`LAMP_SECOND_NAME_CARRIES`): it
    // shows it was said to the lamp, so it is never held as a short sound.
    let marked = checked.is_some_and(|sound| sound.load(Ordering::Acquire))
        && !crate::not_speech::short_word(&text)
        && !name_waits
        && name_of.is_none();
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
    // The lamp's own words heard back (`LAMP_SECOND_OWN_ECHO`, #2997): held
    // as its echo when mostly its own; taken out at the line's start or end
    // when fewer, before the model is sent it. Judged before the rule.
    let echo = if shared.own_echo && !empty && !held {
        own_echo_of(&shared, request, &text, &said, &marks)
    } else {
        own_echo::Echo::Answered(None)
    };
    let (echoed, stripped) = match echo {
        own_echo::Echo::Answered(line) => {
            if let Some(line) = line {
                eprintln!("{line}");
            }
            (false, None)
        }
        own_echo::Echo::Held(line) => {
            eprintln!("{line}");
            (true, None)
        }
        own_echo::Echo::Stripped(kept, line) => {
            eprintln!("{line}");
            (false, Some(kept))
        }
    };
    let mut heeded = match &shared.heed {
        _ if echoed => Some(OWN_ECHO_HELD),
        Some(heed) if !empty && !held => {
            let window = said.lock().map_or((0, 0), |said| *said);
            // Said by the name just before: its window begins with the
            // name's line, so the name heard there is this line's, as for
            // a heard name.
            let own = window;
            let window = match &name_of {
                Some(name) if name.said_from != 0 && window.0 != 0 => {
                    (window.0.min(name.said_from), window.1)
                }
                _ => window,
            };
            // The soft lead counts only for a window that begins with the
            // line's own first sound (#4226).
            let lead_us = if window.0 == own.0 {
                marks.soft_lead_us.load(Ordering::Acquire)
            } else {
                0
            };
            heed(request, window, &text, continues_from, lead_us)
        }
        _ => None,
    };
    // In a busy room, a line going on waits for the Policy's decision
    // (`LAMP_GOING_ON_ASKS`, #1522), as the Live worker keeps such a line's
    // first output; a chat begun on its early words goes on meanwhile. The
    // rule ends the wait at `policy_holds::WAIT`; past a little more, or
    // with no hook, the line is played, as with no decision.
    if heeded == Some(WAITS_FOR_POLICY) {
        let bound = tokio::time::Instant::now() + crate::policy_holds::WAIT + POLICY_POLL * 4;
        heeded = loop {
            tokio::time::sleep(POLICY_POLL).await;
            match shared.policy_now.as_ref().map(|now| now(request)) {
                Some(Some(verdict)) => break verdict,
                Some(None) if tokio::time::Instant::now() < bound => {}
                _ => break None,
            }
        };
    }
    // A stop said over the lamp's own answer (#3985: on lamp-29e7 "Stop"
    // heard as "Stop them." was asked of the model, which said "Stop who?",
    // then "Stop telling story." got "I wasn't telling a story"). The cut-in
    // already stopped the answer; this line is answered here, in the route's
    // voice, and kept in the thread as the lamp's, so the next turn sees the
    // story, the stop and that the lamp stopped. Never carried words.
    // Judged after the echo, held and heed verdicts, so the lamp's own
    // "Stop it!" heard back, or desk talk not for it, is never answered
    // (the Reviewer-lead's P2 on #4004).
    if shared.spoken_stop_ends
        && !echoed
        && !held
        && heeded.is_none()
        && marks.over_the_lamps_voice
        && !went_on
        && crate::spoken_stop::is_stop_request(&text)
        && !shared
            .lamp_said
            .lock()
            .is_ok_and(|said| crate::spoken_stop::could_be_echo(&text, &said.1))
    {
        drop(prepared.take());
        // Still a turn for the forget hold (Reviewer-three's P1 on #4004): a
        // held forget is let go and this line asked nothing, as `answer()`
        // does for any line that is not a yes, so a later "Yes." runs nothing.
        let asker = forget_asker(scope.as_ref());
        let _ = asked_just_before(
            &shared,
            request,
            true,
            forgets_asked(&text),
            asker.as_ref(),
            (shared.clock)(),
        );
        let _ = take_held_forget(
            &shared,
            request,
            true,
            &text,
            asker.as_ref(),
            (shared.clock)(),
        );
        eprintln!(
            "lamp-voice second: request {request}: a stop said over the lamp's answer: answered \"{}\", the model not asked (LAMP_SPOKEN_STOP_ENDS)",
            crate::spoken_stop::LINE
        );
        let said = shared
            .emit(ProviderOutput::Transcript {
                request: Some(request),
                text: crate::spoken_stop::LINE.to_owned(),
                finished: false,
            })
            .await;
        if said {
            let kept = shared.stop_line.lock().ok().and_then(|kept| kept.clone());
            let clip = match kept {
                Some(clip) => clip,
                None => {
                    let clip = render_line(&shared.services, crate::spoken_stop::LINE).await;
                    if !clip.is_empty()
                        && let Ok(mut kept) = shared.stop_line.lock()
                    {
                        *kept = Some(clip.clone());
                    }
                    clip
                }
            };
            if (clip.is_empty() || cue(&shared, request, clip).await.is_ok())
                && shared
                    .emit(ProviderOutput::GenerationComplete { request })
                    .await
            {
                shared
                    .emit(ProviderOutput::TurnComplete {
                        request,
                        idle: true,
                    })
                    .await;
            }
        }
        return;
    }
    // A held echo's journal line, with its counts, is said above.
    if let Some(why) = heeded.filter(|_| !echoed) {
        eprintln!("lamp-voice second: request {request}: its answer is held back: {why}");
    }
    // Words cut short by this line: answered in its place if it is held,
    // dropped if it is answered itself (#1712).
    let carried = shared
        .cut
        .lock()
        .ok()
        .and_then(|mut cut| cut.take())
        .filter(|carried| carried.request < request && carried.at.elapsed() <= CARRY_WITHIN)
        // A stop or "never mind" withdraws them: never asked again, in its
        // place or joined to it.
        .filter(|carried| {
            let stop = says_stop(&text);
            if stop {
                eprintln!(
                    "lamp-voice second: request {}'s words, cut short, are let go: request {request} says stop or never mind",
                    carried.request
                );
            }
            !stop
        });
    // The turn's verdict, as the journal says it above (#1155): only for a
    // turn something judged, and never for one with nothing heard.
    if let Some(told) = shared.verdicts.as_ref().filter(|_| !empty) {
        if held || heeded.is_some() {
            told(request, lamp_msgs::TurnVerdict::HeldBack);
        } else if shared.heed.is_some() || marked {
            told(request, lamp_msgs::TurnVerdict::Answered);
        }
    }
    if empty || held || heeded.is_some() {
        if let Ok(mut last) = shared.held_last.lock() {
            *last = Some(request);
        }
        // A held line's named ask is said, as an answered one's is.
        if let Some(named) = shared.named_ask.as_ref().filter(|_| !empty) {
            shared.named_ask_said(named_ask_held_line(
                request,
                &named(&text),
                text.split_whitespace().count(),
            ));
        }
        // Left unanswered: a line held by the rule is not talk, nor are no
        // words at all, which are a sound (as a short sound already is).
        room(|room| {
            if empty {
                room.ended_as_sound(request, monotonic_us());
            }
            room.unanswered(request, monotonic_us());
        });
        // Done before the turn is over, since an over turn's task is
        // stopped; beside the words carried, so their answer never waits.
        let though_held = async {
            if heeded.is_some()
                && !echoed
                && shared.though_held
                && crate::attention::held_line_names_reports(&text)
            {
                eprintln!("{}", held_asked_line(request));
                done_though_held(&shared, (request, scope.as_ref()), &text).await;
            }
        };
        if let Some(carried) = carried {
            // In flight before any wait, so a line begun meanwhile carries
            // them (the Reviewer on #2657; reviewer-three's P3 on the chain).
            let holding = holding_while_joining(
                &shared,
                Carried {
                    request,
                    ..carried.clone()
                },
            );
            // A chain's links no verdict judged, judged for a held line:
            // only the voices or one face join them, against the chain's
            // newest piece (words answered in a held line's place are that
            // line's, not the held one's), all within one wait.
            let bound = tokio::time::Instant::now() + JOIN_WAIT;
            let against = carried
                .parts
                .last()
                .map_or(carried.request, |part| part.request);
            let carried = settled(
                shared.same_speaker.as_ref(),
                request,
                carried,
                (false, against, bound),
                None,
            )
            .await;
            if holding && !still_holding(&shared, request) {
                return;
            }
            // The same person going on: its own words after them
            // (`LAMP_JOIN_CUT_WORDS`), so a request's second half held on
            // its own is not lost either.
            // The line after the name, held as not shown said by it: the name
            // is answered in its place, and the model is told words came, never
            // which (`worker::NAME_THEN_HELD`).
            let refused = !empty && name_refused(&shared, request, &carried);
            // Only the carried words again keep their write's owner; joined
            // with new words, the line may ask for a write of its own.
            let (words, said_by, parts, owner) = if empty || refused {
                unsay_carried(&shared, request, &carried);
                (
                    carried.text(),
                    carried.said_by,
                    carried.parts.clone(),
                    carried.memory_write_owner.clone(),
                )
            } else {
                // While the join waits for a print, the carried words are
                // where a newer line's cut finds them (the Reviewer on
                // #2657).
                let joining = holding_while_joining(
                    &shared,
                    Carried {
                        request,
                        ..carried.clone()
                    },
                );
                let joined = joined_with_cut(
                    &shared,
                    (request, false, bound),
                    &marks,
                    carried.clone(),
                    text.clone(),
                )
                .await;
                if !asked_unsaying(&shared, (request, joining), &joined.2) {
                    return;
                }
                match joined {
                    (joined, None, parts) => (joined, request, parts, WriteOwner::default()),
                    (_, Some(carried), _) => {
                        unsay_carried(&shared, request, &carried);
                        (
                            carried.text(),
                            carried.said_by,
                            carried.parts,
                            carried.memory_write_owner,
                        )
                    }
                }
            };
            eprintln!(
                "lamp-voice second: request {request} is held, so request {}'s words, cut short by it, are answered in its place",
                carried.request
            );
            put_in_flight(
                &shared,
                Carried {
                    request,
                    parts,
                    at: carried.at,
                    said_by,
                    unchecked: Vec::new(),
                    memory_write_owner: owner.clone(),
                },
            );
            let lease = scope.clone();
            let notes = if refused {
                vec![worker::NAME_THEN_HELD.to_owned()]
            } else {
                Vec::new()
            };
            tokio::join!(
                though_held,
                answer(
                    &shared,
                    (request, lease),
                    words,
                    Asker::Spoken,
                    None,
                    None,
                    notes,
                    times,
                    None,
                    Some(owner),
                )
            );
            return;
        }
        though_held.await;
        // Held as unclear: the coordinator asks again, or hums, as for the
        // Live route (#926, #828). Told once, before the turn completes.
        if heeded.is_some()
            && !echoed
            && let Some((_, ask)) = &shared.ask_again
            && let Some((why, cut_in)) = ask(request)
        {
            eprintln!(
                "lamp-voice second: request {request} was held as unclear ({why}): {}",
                if cut_in {
                    "the lamp hums, and the next request is told it was not caught"
                } else {
                    "the coordinator asks again"
                }
            );
            if cut_in && let Ok(mut at) = shared.misheard_at_us.lock() {
                *at = Some(monotonic_us());
            }
            let unclear = ProviderOutput::Unclear {
                request,
                why: why.to_owned(),
                cut_in,
            };
            if !shared.emit(unclear).await {
                return;
            }
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
    // The lamp's own words at its start or end are not sent to the model
    // (`LAMP_SECOND_OWN_ECHO`); the line was told as heard above.
    let text = stripped.unwrap_or(text);
    // Words cut short by this line, the same person's: asked before its
    // own, as one line (`LAMP_JOIN_CUT_WORDS`).
    let own = own_part(request, &text, &marks);
    let (text, carried, parts) = match carried {
        Some(carried) if name_refused(&shared, request, &carried) => {
            (text, Some(carried), vec![own])
        }
        Some(carried) => {
            // While the join waits for a print, this line's own words are
            // where a newer line's cut finds them, as an answered line's are
            // once it is asked (the Reviewer on #2657); after the chain it
            // goes on from, when it began soon enough after it, that link
            // judged at the newer line's verdict.
            let joining = holding_while_joining(
                &shared,
                waiting_chain(request, &marks, &carried, own.clone()),
            );
            let bound = tokio::time::Instant::now() + JOIN_WAIT;
            let joined =
                joined_with_cut(&shared, (request, true, bound), &marks, carried, text).await;
            if !asked_unsaying(&shared, (request, joining), &joined.2) {
                return;
            }
            joined
        }
        None => (text, None, vec![own]),
    };
    // The first request answered after a misheard cut-in was hummed for,
    // within `NOTE_WITHIN_US`: the model is told it did not catch it, as the
    // Live worker tells it (`LAMP_MISHEARD_HUM`, #828). Once.
    let misheard = shared
        .misheard_at_us
        .lock()
        .ok()
        .and_then(|mut at| at.take())
        .filter(|held| crate::misheard_hum::answers_the_hum(*held, monotonic_us()))
        .map(|_| crate::misheard_hum::NOTE.to_owned());
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
    // The first request asked after a push on the body ended a skill's
    // search: the model is told so, as of a stop, and these words may ask
    // for a look again (#2904).
    let pushed = shared
        .search_pushed(|pushed| pushed.asked(request, monotonic_us()))
        .flatten()
        .map(|note| {
            eprintln!(
                "lamp-voice second: request {request}: the model is told the person stopped the search with a hand"
            );
            note.to_owned()
        });
    let staff = shared
        .staff
        .as_ref()
        .and_then(|staff| staff(request, &text));
    // What memory holds for these words: the prepared chat's, when it was
    // begun on them (so its round still stands), else asked now, off the
    // runtime's thread (Fourth's P3 on #2229).
    let remembered = match prepared
        .as_ref()
        .filter(|prepared| plain_words(&prepared.words) == plain_words(&text))
    {
        Some(prepared) => prepared.remembered.clone(),
        // A line held only by a gap is told no hits ([`held_by_gap`]).
        None if held_by_gap(scope.as_ref()) => None,
        None => match shared.remembered.clone() {
            Some(note) => {
                let words = text.clone();
                let lease = scope.clone();
                tokio::task::spawn_blocking(move || note(request, &words, lease.as_ref()))
                    .await
                    .ok()
                    .flatten()
            }
            None => None,
        },
    };
    // Its name heard as it began: told as the Live route tells it (#1712:
    // without it this route missed two calls this morning).
    let named = shared
        .name_note
        .as_ref()
        .and_then(|note| note(request))
        .map(str::to_owned);
    // The lamp's name heard as another word at the line's start is no name
    // (#3504 finding N4).
    let fired = shared
        .name_fired
        .as_ref()
        .is_some_and(|fired| fired(request));
    let wake = wake_not_a_name(
        request,
        &text,
        fired || named.is_some(),
        crate::remember::heard::on(),
    );
    // A task prepared and waiting for a yes: this route's history keeps the
    // words of its last exchanges, not its calls, so the model is told, or a
    // yes is taken as a new hand-over (#1712).
    let waiting = shared.task_waiting.as_ref().and_then(|note| note(request));
    // The coordinator's marks, told as the Live route tells them (#1712).
    if !cut_in.is_empty() {
        eprintln!(
            "lamp-voice second: request {request}: it began over the lamp's answer, and the model is told so"
        );
    }
    let follows_on = (marks.follows_on && !marks.sound.load(Ordering::Acquire))
        .then(|| {
            if shared.busy(request) {
                eprintln!(
                    "lamp-voice second: request {request}: begun soon after the lamp spoke, in a busy room; the model is not told it may follow on"
                );
                None
            } else {
                eprintln!(
                    "lamp-voice second: request {request}: the model is told it may follow on from what the lamp said"
                );
                Some(crate::follows_on::NOTE.to_owned())
            }
        })
        .flatten();
    let called = called.then(|| {
        eprintln!(
            "lamp-voice second: request {request}: only the lamp's name was said, and the model is told so"
        );
        crate::provider_worker::CALLED_NOTE.to_owned()
    });
    // A stuffy room, told beside what a person says (#1712's map, item 6).
    let air = shared.air.as_ref().and_then(|(note, _)| note(request));
    // How a known person in view seems, never the room's or a gap's (#1671):
    // the scope is the room's when the camera may not be used.
    let seems = shared
        .seems
        .as_ref()
        .filter(|_| {
            scope
                .as_ref()
                .and_then(crate::memory_scope::ScopeLease::scope)
                .is_some_and(|scope| scope.person().is_some() && !scope.held())
        })
        .and_then(|(note, _)| note(request));
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
        .chain(pushed)
        .chain(cut_in)
        .chain(misheard)
        .chain(staff)
        .chain(remembered)
        .chain(named)
        .chain(wake)
        .chain(call)
        .chain(alone)
        .chain(waiting)
        .chain(follows_on)
        .chain(called)
        .chain(air)
        .chain(seems)
        .collect();
    if let Some(carried) = carried {
        eprintln!(
            "lamp-voice second: request {}'s words, cut short, are let go: request {request} is answered",
            carried.request
        );
    }
    let memory_write_owner = WriteOwner::default();
    put_in_flight(
        &shared,
        Carried {
            request,
            parts,
            at: Instant::now(),
            said_by: request,
            unchecked: Vec::new(),
            memory_write_owner: memory_write_owner.clone(),
        },
    );
    if let Some(heard) = &shared.words_heard {
        heard(request, &text);
    }
    answer(
        &shared,
        (request, scope),
        text,
        Asker::Spoken,
        Some(turn),
        None,
        notes,
        times,
        prepared,
        Some(memory_write_owner),
    )
    .await;
}

/// Where the worker reads [`Engine::keeping_a_listener_ready`]: `on`, and
/// nothing else.
pub const STT_WARM_ENVIRONMENT: &str = "LAMP_SECOND_STT_WARM";

/// How long a listener kept for the next line is kept: a conversation's
/// lines come seconds apart, and after this the lamp keeps nothing open
/// (#1712, the first builder at 00:31: "at most a few minutes").
pub const KEPT_FOR: Duration = Duration::from_secs(180);

/// Whether a line's words are a stop, not a request going on: "never mind"
/// anywhere in them, or a stop word or pair of the stop words' one list
/// ([`crate::stop_words::stop_phrase`]: "Stop.", "Lamp, hold on.") in a
/// line of three words or fewer. The words it cut short are then let go,
/// held or let out (a reviewer's P2 on the join, 22 Oct).
fn says_stop(text: &str) -> bool {
    let words = crate::not_speech::plain_words(text);
    words
        .windows(2)
        .any(|pair| pair[0] == "never" && pair[1] == "mind")
        || words.iter().any(|word| word == "nevermind")
        || (words.len() <= 3 && crate::stop_words::stop_phrase(text))
}

/// Whether `carried` are the lamp's name's words that `request`, the line
/// after it, was not shown to be said by (`LAMP_SECOND_NAME_CARRIES`, El's
/// P3): never joined to it (`joined_with_cut`, whose no sign of a second
/// person takes one face in view at each in a busy room), only answered
/// in its place if it is held, else let go.
fn name_refused(shared: &Shared, request: u64, carried: &Carried) -> bool {
    let refused = shared
        .name_refused
        .lock()
        .is_ok_and(|refused| *refused == Some(carried.request));
    if refused {
        eprintln!(
            "lamp-voice second: request {request}: not joined with request {}'s words: they are the lamp's name it was not shown to be said by (LAMP_SECOND_NAME_CARRIES)",
            carried.request
        );
    }
    refused
}

/// With `LAMP_JOIN_CUT_WORDS`, the words a line holds while its join waits
/// for a print, put in flight before the wait: a third line begun during
/// it is a cut, as any line's is, and carries them. Before, they were out
/// of every slot through the wait, and a line held after it found nothing:
/// the first question was lost (the Reviewer on #2657). `true` when the
/// join can wait.
fn holding_while_joining(shared: &Shared, held: Carried) -> bool {
    if shared.same_speaker.is_none() {
        return false;
    }
    put_in_flight(shared, held);
    true
}

/// `carried` in flight, its request recorded as asked
/// ([`Shared::asked_last`]).
fn put_in_flight(shared: &Shared, carried: Carried) {
    if let Ok(mut asked) = shared.asked_last.lock() {
        *asked = Some(asked.map_or(carried.request, |last| last.max(carried.request)));
    }
    if let Ok(mut in_flight) = shared.in_flight.lock() {
        *in_flight = Some(carried);
    }
}

/// The line still holds its words after its join's wait: no newer line's
/// cut took them. Once one has, the line stops, as a cut line's task does,
/// and the newer line has the words.
fn still_holding(shared: &Shared, request: u64) -> bool {
    shared.in_flight.lock().ok().is_some_and(|in_flight| {
        in_flight
            .as_ref()
            .is_some_and(|held| held.request == request)
    })
}

/// Whether `request`'s line is asked after its join: with `joining`, only
/// while it still holds its words ([`still_holding`]). Then the pieces
/// joined into it (`parts`, but its own) had none of their answers heard,
/// and with [`Engine::unsaying_joined`] those answers leave the thread,
/// newest first so the oldest question leads the joined line's words
/// there. Checked and unsaid under the lock a newer line's cut takes the
/// words with, so a line whose chain is taken unsays nothing: its cut
/// exchanges stay, and the person's question is not lost (El's P2 on
/// #3129, #2145's P2). `false`: the line stops, its words the newer line's.
fn asked_unsaying(shared: &Shared, (request, joining): (u64, bool), parts: &[Part]) -> bool {
    let Ok(in_flight) = shared.in_flight.lock() else {
        return !joining;
    };
    if joining
        && in_flight
            .as_ref()
            .is_none_or(|held| held.request != request)
    {
        return false;
    }
    let Some(unsay) = shared.unsay_joined.as_ref() else {
        return true;
    };
    let unsaid: Vec<u64> = parts
        .iter()
        .rev()
        .map(|part| part.request)
        .filter(|piece| *piece != request)
        .collect();
    if unsaid.is_empty() {
        return true;
    }
    for piece in &unsaid {
        unsay(*piece);
    }
    drop(in_flight);
    eprintln!(
        "lamp-voice second: request {request}: {} lines joined into it had none of their answers played; those answers are not kept as the lamp's words (LAMP_JOIN_CUT_WORDS)",
        unsaid.len()
    );
    true
}

/// `request`'s own words in a chain: two lines when its sound or words
/// began with the line before it.
fn own_part(request: u64, text: &str, marks: &Marks) -> Part {
    Part {
        request,
        text: text.to_owned(),
        at: marks.began.unwrap_or_else(Instant::now),
        lines: 1 + u16::from(marks.folded) + u16::from(marks.name_carried.load(Ordering::Acquire)),
    }
}

/// What a line let out holds while its join waits for a print
/// ([`holding_while_joining`]): after `carried`'s chain, with that link to
/// be judged at a newer line's verdict, when it began within
/// [`JOIN_WITHIN`] of that chain's end and the words are that line's own;
/// else its own words alone.
fn waiting_chain(request: u64, marks: &Marks, carried: &Carried, own: Part) -> Carried {
    let linked = carried.said_by == carried.request
        && marks
            .after_end
            .is_some_and(|(of, gap)| of == carried.request && gap <= JOIN_WITHIN);
    if !linked {
        return Carried::own(request, own.text, own.at, request);
    }
    let mut chain = carried.clone();
    chain.unchecked.push((carried.request, request));
    chain.parts.push(own);
    Carried {
        request,
        at: Instant::now(),
        said_by: request,
        // New words, so a write of their own ([`WriteOwner`]).
        memory_write_owner: WriteOwner::default(),
        ..chain
    }
}

/// `carried`'s chain judged for `request`, let out or held (`let_out`),
/// by `bound` (one wait for the verdict and the whole chain, a print still
/// to come waited for until then): its links no verdict judged, and every
/// older piece against `against`, the line the whole chain must be one
/// person's with, each pair once; `known`, the pair the verdict judged
/// already. While no pair says two people, all of it is kept. Once one
/// does, only the pieces one face seen speaking both or one voice shows to
/// be `against`'s speaker's are kept, never one that passed only on no sign
/// of a second person (a reviewer's P2 on the chain, 22 Oct, and
/// reviewer-three's: X seen saying the first piece, nobody the short
/// middle one, Y the last; the middle one is not Y's for want of a sign).
/// The journal says how many pieces go. Without a judge, as it came.
async fn settled(
    same: Option<&SameSpeaker>,
    request: u64,
    mut carried: Carried,
    (let_out, against, bound): (bool, u64, tokio::time::Instant),
    known: Option<((u64, u64), Joined)>,
) -> Carried {
    let Some(same) = same else {
        return carried;
    };
    let mut pairs = std::mem::take(&mut carried.unchecked);
    pairs.extend(
        carried
            .parts
            .iter()
            .map(|part| (part.request, against))
            .filter(|(earlier, against)| earlier < against),
    );
    pairs.sort_unstable();
    pairs.dedup();
    let mut said: Vec<((u64, u64), Joined)> = known.into_iter().collect();
    for pair in pairs {
        if said.iter().all(|(judged, _)| *judged != pair) {
            said.push((pair, said_by_one(same, pair, bound).await));
        }
    }
    let refused = said.iter().find_map(|((earlier, later), joined)| {
        link_holds(joined, let_out)
            .err()
            .map(|why| (*earlier, *later, why))
    });
    let Some((earlier, later, why)) = refused else {
        return carried;
    };
    let before = carried.parts.len();
    carried.parts.retain(|part| {
        part.request == against
            || said.iter().any(|(pair, joined)| {
                *pair == (part.request, against) && matches!(joined, Joined::Same(_))
            })
    });
    eprintln!(
        "lamp-voice second: request {request}: the chain cut short has two people in it, requests {earlier} and {later}: {why}; of its pieces only those one face or voice shows the same speaker said are kept, {} let go (LAMP_JOIN_CUT_WORDS)",
        before - carried.parts.len()
    );
    carried
}

/// What `same` says of whether one person said `before` and `request`, a
/// voice print still to come waited for until `bound`, then what is known
/// without it ([`Joined::Coming`]): for a join ([`joined_with_cut`]), a
/// chain's later-judged links ([`settled`]) and the lamp's name
/// ([`said_by_the_name`]).
async fn said_by_one(
    same: &SameSpeaker,
    (before, request): (u64, u64),
    bound: tokio::time::Instant,
) -> Joined {
    loop {
        match same(before, request) {
            Joined::Coming(without) if tokio::time::Instant::now() >= bound => {
                break match *without {
                    Joined::Not(why) => Joined::Not(format!(
                        "a voice print did not come in {} ms; {why}",
                        JOIN_WAIT.as_millis()
                    )),
                    without => without,
                };
            }
            Joined::Coming(_) => tokio::time::sleep(JOIN_POLL).await,
            said => break said,
        }
    }
}

/// Whether `joined` shows one person said both pieces of a link: no sign
/// of a second person only for a line let out.
fn link_holds(joined: &Joined, let_out: bool) -> Result<(), String> {
    match joined {
        Joined::Same(_) => Ok(()),
        Joined::NoSecond(_) if let_out => Ok(()),
        Joined::NoSecond(why) => Err(format!("{why}, but this line was held")),
        Joined::Not(why) => Err(why.clone()),
        Joined::Coming(_) => Err("a voice print still to come".to_owned()),
    }
}

/// The name `request` is said by (`LAMP_SECOND_NAME_CARRIES`): `name`'s
/// words, once back (waited for up to its 2.5 s), and only when one person
/// said both ([`Engine::telling_one_speaker`]: one face seen speaking each,
/// the same voice, or no sign of a second person in a quiet room). `own`
/// is `request`'s window, for the rule to keep it before it is judged.
/// Not carried, `request` is judged as its own, and the name's words wait
/// where words cut short do: answered in its place if it is held.
async fn said_by_the_name(
    shared: &Shared,
    request: u64,
    name: NameWait,
    own: (u64, u64),
) -> Option<SaidBy> {
    let (words, called) = match name.words {
        NameWords::Known(said) => said,
        NameWords::Coming(coming) => match tokio::time::timeout_at(name.until, coming).await {
            Ok(Ok(Some(said))) => said,
            _ => {
                eprintln!(
                    "lamp-voice second: request {request}: request {}'s words did not come back within 2.5 s of its end: its name is not carried (LAMP_SECOND_NAME_CARRIES)",
                    name.request
                );
                return None;
            }
        },
    };
    let said = match &shared.one_speaker {
        Some((same, kept)) => {
            kept(request, own);
            said_by_one(
                same,
                (name.request, request),
                tokio::time::Instant::now() + JOIN_WAIT,
            )
            .await
        }
        None => Joined::Not("nothing here tells whether one person said both".to_owned()),
    };
    let why = match said {
        Joined::Same(why) => Ok(why.to_owned()),
        Joined::NoSecond(why) => Ok(why),
        Joined::Not(why) => Err(why),
        Joined::Coming(_) => Err("a voice print still to come past the wait".to_owned()),
    };
    match why {
        Ok(why) => {
            eprintln!(
                "lamp-voice second: request {request} is said by the name of request {}: {why} (LAMP_SECOND_NAME_CARRIES)",
                name.request
            );
            Some(SaidBy {
                said_from: name.said_from,
                words,
                called,
            })
        }
        Err(why) => {
            eprintln!(
                "lamp-voice second: request {request} is not said by the name of request {}: {why}; it is judged as its own (LAMP_SECOND_NAME_CARRIES)",
                name.request
            );
            let asked = if name_first(&words) {
                words
            } else {
                CALLED_WORDS.to_owned()
            };
            if let Ok(mut cut) = shared.cut.lock()
                && cut.is_none()
            {
                *cut = Some(Carried::own(
                    name.request,
                    asked,
                    (shared.clock)(),
                    name.request,
                ));
                if let Ok(mut refused) = shared.name_refused.lock() {
                    *refused = Some(name.request);
                }
            }
            None
        }
    }
}

/// `text`, the words of `request`, let out or held (`let_out`), and
/// `carried`, the words of the line it cut short, none of whose answer was
/// heard, with the chain cut short before that line: all asked as one line
/// when one person said each link ([`Engine::joining_cut_words`]), or, for
/// a line let out, when nothing shows a second person ([`Joined::NoSecond`]);
/// each line begun within [`JOIN_WITHIN`] of the one before's end, and the
/// carried words that line's own. A print still to come is waited for up to
/// [`JOIN_WAIT`]. A link of the chain no verdict judged that fails breaks it
/// there, the pieces before it let go; past [`CHAIN_MOST_WORDS`] or
/// [`CHAIN_MOST_FOR`] the oldest pieces are let go too. Otherwise both as
/// they came, and the journal says why, in counts. The words asked, the
/// words let go if not joined, and the parts to keep in flight.
async fn joined_with_cut(
    shared: &Shared,
    (request, let_out, bound): (u64, bool, tokio::time::Instant),
    marks: &Marks,
    carried: Carried,
    text: String,
) -> (String, Option<Carried>, Vec<Part>) {
    let own = own_part(request, &text, marks);
    let Some(same) = shared.same_speaker.as_ref() else {
        return (text, Some(carried), vec![own]);
    };
    let not_joined = |why: &str| {
        eprintln!(
            "lamp-voice second: request {request}: not joined with request {}'s words, cut short by it: {why} (LAMP_JOIN_CUT_WORDS)",
            carried.request
        );
    };
    // Words carried through a held line are another line's: whose they are
    // is not what the held line's voice or face tells (the review of #2620).
    if carried.said_by != carried.request {
        not_joined(&format!(
            "its words were said in request {}, answered in a held line's place",
            carried.said_by
        ));
        return (text, Some(carried), vec![own]);
    }
    let Some((_, gap)) = marks.after_end.filter(|(of, _)| *of == carried.request) else {
        not_joined("its sound or words began this line already, or it was not the line before");
        return (text, Some(carried), vec![own]);
    };
    if gap > JOIN_WITHIN {
        not_joined(&format!(
            "begun {} ms after its end, past {} ms",
            gap.as_millis(),
            JOIN_WITHIN.as_millis()
        ));
        return (text, Some(carried), vec![own]);
    }
    let said = said_by_one(same, (carried.request, request), bound).await;
    let verdict = said.clone();
    let why = match said {
        Joined::Same(why) => Ok(why.to_owned()),
        Joined::NoSecond(why) if let_out => Ok(why),
        Joined::NoSecond(why) => Err(format!(
            "{why}, but this line was held: a held line's words are joined only by the voices or one face"
        )),
        Joined::Not(why) => Err(why),
        Joined::Coming(_) => Err("a voice print still to come past the wait".to_owned()),
    };
    let why = match why {
        Ok(why) => why,
        Err(why) => {
            not_joined(&why);
            return (text, Some(carried), vec![own]);
        }
    };
    // The chain's links no verdict judged: one that fails breaks it there.
    let mut parts = settled(
        Some(same),
        request,
        carried.clone(),
        (let_out, request, bound),
        Some(((carried.request, request), verdict)),
    )
    .await
    .parts;
    parts.push(own);
    // At most so many words, and so long since its oldest piece.
    let now = (shared.clock)();
    let mut dropped = 0;
    while parts.len() > 1
        && (words_of(&parts).split_whitespace().count() > CHAIN_MOST_WORDS
            || now.saturating_duration_since(parts[0].at) > CHAIN_MOST_FOR)
    {
        parts.remove(0);
        dropped += 1;
    }
    let lines: u16 = parts.iter().map(|part| part.lines).sum();
    eprintln!(
        "lamp-voice second: request {request} goes on from request {}'s words, cut short {} ms before it began: {why}; {lines} lines are asked as one{} (LAMP_JOIN_CUT_WORDS)",
        carried.request,
        gap.as_millis(),
        if dropped > 0 {
            format!(
                ", its {dropped} oldest pieces let go past {CHAIN_MOST_WORDS} words or {} s",
                CHAIN_MOST_FOR.as_secs()
            )
        } else {
            String::new()
        }
    );
    (words_of(&parts), None, parts)
}

/// Words cut short and answered in held line `request`'s place, not joined
/// (#1712's carry): they are carried only while none of their answer was
/// heard, so any of it the thread kept was never played. With
/// [`Engine::unsaying_joined`] (`LAMP_JOIN_CUT_WORDS`) those answers leave
/// the thread as a joined line's do, newest first so the oldest question
/// leads: the request that last answered them (a held line's, when carried
/// through one) and each piece's own (reviewer-three's P3 on #3129: else
/// "Twenty past eight in…" stays the lamp's words and London is asked again
/// after it).
fn unsay_carried(shared: &Shared, request: u64, carried: &Carried) {
    let Some(unsay) = shared.unsay_joined.as_ref() else {
        return;
    };
    let mut unsaid: Vec<u64> = carried
        .parts
        .iter()
        .map(|part| part.request)
        .chain([carried.request])
        .filter(|piece| *piece != request)
        .collect();
    unsaid.sort_unstable_by(|a, b| b.cmp(a));
    unsaid.dedup();
    for piece in &unsaid {
        unsay(*piece);
    }
    eprintln!(
        "lamp-voice second: request {request}: the words answered in its place had none of their answers played; those answers are not kept as the lamp's words ({} requests, LAMP_JOIN_CUT_WORDS)",
        unsaid.len()
    );
}

/// Opens a listener for the next line under privacy `generation` and keeps
/// it, in place of any kept before, at most [`KEPT_FOR`]: a lamp nobody
/// talks to keeps nothing open ([`Engine::keeping_a_listener_ready`]).
fn keep_a_listener(shared: &Arc<Shared>, generation: u64) {
    let number = shared.spares.fetch_add(1, Ordering::Relaxed) + 1;
    if let Ok(mut spare) = shared.spare.lock()
        && let Some(old) = spare.replace(Kept {
            number,
            generation,
            opening: tokio::spawn(shared.services.listen()),
        })
    {
        old.opening.abort();
    }
    let expiring = Arc::downgrade(shared);
    tokio::spawn(async move {
        tokio::time::sleep(KEPT_FOR).await;
        if let Some(shared) = expiring.upgrade()
            && let Ok(mut spare) = shared.spare.lock()
            && spare.as_ref().is_some_and(|kept| kept.number == number)
            && let Some(kept) = spare.take()
        {
            kept.opening.abort();
        }
    });
}

/// A listener opening, or open, for the next line
/// ([`Engine::keeping_a_listener_ready`]): which one it is, the privacy
/// generation it was opened under, and its connection.
struct Kept {
    number: u64,
    generation: u64,
    opening: JoinHandle<net::Result<Box<dyn Listener>>>,
}

/// Where the worker reads [`Engine::preparing`]: `on`, and nothing else.
pub const PREPARED_ENVIRONMENT: &str = "LAMP_SECOND_PREPARED";

/// Where the worker reads [`Engine::waiting_for_a_name`], and the
/// coordinator whether a sound after such a line's end goes on from it:
/// `on`, and nothing else.
pub const OPEN_NAME_ENVIRONMENT: &str = "LAMP_SECOND_OPEN_NAME";

/// Where the worker reads [`Engine::carrying_the_name`]: `on`, and nothing
/// else.
pub const NAME_CARRIES_ENVIRONMENT: &str = "LAMP_SECOND_NAME_CARRIES";

/// Where the worker reads whether, right after a start, a line's room not
/// yet known only for want of history is taken as quiet for a join when the
/// ears show it is ([`worker::Heeding::showing_the_first_seconds_quiet`]):
/// `on`, and nothing else.
pub const FIRST_SECONDS_QUIET_ENVIRONMENT: &str = "LAMP_FIRST_SECONDS_QUIET";

/// Where the worker reads [`Engine::holding_own_echo`] (#2997): `on`, and
/// nothing else.
pub const OWN_ECHO_ENVIRONMENT: &str = "LAMP_SECOND_OWN_ECHO";

/// The known person's name told to the model each turn, as the Live route
/// tells it (#2539, the forgotten name): `on`, and nothing else.
pub const FACE_NAME_ENVIRONMENT: &str = "LAMP_SECOND_FACE_NAME";

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
    /// The notes beside the words known when it began (a cut-in's, and
    /// memory's for those words).
    notes: Vec<String>,
    /// What memory told for its words ([`Engine::remembering`]), asked as it
    /// began, so the final words, when they are its own, aren't asked again
    /// and its round still stands (Fourth's P2 on #2229).
    remembered: Option<String>,
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
    /// system message and history, the `notes` known so far, then the
    /// words.
    fn begin(
        shared: &Shared,
        scope: Option<&crate::memory_scope::ScopeLease>,
        words: String,
        notes: &[String],
        remembered: Option<String>,
    ) -> Self {
        let notes: Vec<String> = notes.iter().cloned().chain(remembered.clone()).collect();
        let mut messages = told(shared, scope).messages();
        messages.extend(notes.iter().cloned().map(Message::system));
        messages.push(Message::user(words.clone()));
        let mut chat = chat_for(shared, scope, messages.clone());
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
            notes,
            remembered,
            messages,
            events,
            at: Instant::now(),
            task: Some(task),
        }
    }

    /// Why it can't stand for round 1 asked with `messages`: `base`, its
    /// system message and history; `notes` beside the words, then `said`.
    /// `None` when it can: the same words (as letters, digits and spaces,
    /// whatever the case), the same conversation, and the notes it was
    /// begun with, but for the name note on words that name the lamp,
    /// which tells nothing new.
    fn misfit(&self, base: &[Message], notes: &[String], said: &str) -> Option<&'static str> {
        let asked = &self.messages[..self.messages.len().saturating_sub(1)];
        let asked = asked.get(..asked.len().saturating_sub(self.notes.len()));
        let named = crate::attention::called_by_name_on(said, true);
        let told = notes.iter().filter(|note| {
            !(named && [crate::called::NOTE, crate::called::NOTE_HEARD_AS].contains(&note.as_str()))
        });
        if plain_words(&self.words) != plain_words(said) {
            Some("the final words are not the early ones")
        } else if asked != Some(base) {
            Some("the conversation changed")
        } else if !told.eq(self.notes.iter()) {
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
/// How many of `heard`'s words are among `other`'s, each word as its letters
/// and digits in lower case ("It's" is "its"), so the count is never more
/// than the line's own words.
fn shared_words(heard: &str, other: &str) -> usize {
    let others: std::collections::HashSet<String> =
        other.split_whitespace().map(word_key).collect();
    heard
        .split_whitespace()
        .map(word_key)
        .filter(|word| !word.is_empty() && others.contains(word))
        .count()
}

/// A word as [`shared_words`] matches it: its letters and digits, lower case.
fn word_key(word: &str) -> String {
    word.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The journal's count, never the words, of how many of a line's words were
/// in the line it cut short and in the lamp's latest answer so far (#1712,
/// the first builder at 03:48: on lamp-4ace one line said once became two
/// requests of 6 words each, the second begun with nothing played; the same
/// words again read as a replay, the lamp's own as an echo).
fn tell_whose_words(shared: &Shared, request: u64, heard: &str) {
    let cut = shared
        .cut
        .lock()
        .ok()
        .and_then(|cut| {
            cut.as_ref()
                .map(|carried| (carried.request, carried.text()))
        })
        .filter(|(of, _)| *of < request);
    let said = shared
        .lamp_said
        .lock()
        .ok()
        .map(|said| said.clone())
        .filter(|(of, said)| *of != 0 && *of < request && !said.trim().is_empty());
    if let Some(line) = whose_words(request, heard, cut, said) {
        eprintln!("{line}");
    }
}

/// Why a line held as the lamp's own echo is held ([`own_echo`]).
const OWN_ECHO_HELD: &str = "the lamp's own echo (LAMP_SECOND_OWN_ECHO)";

/// [`own_echo::judge`] for `request`'s words `heard`: when its line began
/// (`said`, the microphone's read time of its first packet), over the lamp's
/// voice or not, and the lamp's latest answer so far, as
/// [`tell_whose_words`] counts it, with when its words sent would have
/// played to their end.
fn own_echo_of(
    shared: &Shared,
    request: u64,
    heard: &str,
    said: &Mutex<(u64, u64)>,
    marks: &Marks,
) -> own_echo::Echo {
    let began_us = said.lock().map_or(0, |said| said.0);
    let answer = shared.lamp_said.lock().ok().map(|said| said.clone());
    let until_us = shared.said_until_us.load(Ordering::Relaxed);
    own_echo::judge(
        &own_echo::Line {
            request,
            heard,
            began_us,
            over_the_lamps_voice: marks.over_the_lamps_voice,
            called: marks.called.load(Ordering::Acquire),
        },
        answer
            .as_ref()
            .map(|(of, words)| own_echo::Said {
                request: *of,
                words,
                until_us,
            })
            .as_ref(),
    )
}

/// The journal's line for [`tell_whose_words`]: `request`'s words counted
/// against the line it cut short (`cut`) and the lamp's answer so far
/// (`said`), each with its request, counts only. None with no words, or
/// nothing to compare them with.
fn whose_words(
    request: u64,
    heard: &str,
    cut: Option<(u64, String)>,
    said: Option<(u64, String)>,
) -> Option<String> {
    let words = heard.split_whitespace().count();
    if words == 0 || cut.is_none() && said.is_none() {
        return None;
    }
    let cut = cut.map_or(String::new(), |(of, text)| {
        format!(
            ", {} in request {of}'s words it cut short",
            shared_words(heard, &text)
        )
    });
    let said = said.map_or(String::new(), |(of, text)| {
        format!(
            ", {} in the lamp's answer to request {of} so far",
            shared_words(heard, &text)
        )
    });
    Some(format!(
        "lamp-voice second: request {request}: of its {words} words{cut}{said}"
    ))
}

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
/// The next result's words while the line is heard; pending once there are
/// none to come.
async fn next_said(said: &mut Option<mpsc::UnboundedReceiver<String>>) -> Option<String> {
    let Some(receiver) = said.as_mut() else {
        return std::future::pending().await;
    };
    match receiver.recv().await {
        Some(words) => Some(words),
        None => {
            *said = None;
            std::future::pending().await
        }
    }
}

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

/// What a turn's chat is told ([`told_whole`]), with a dated follow-up's
/// section cut when it is withheld ([`cut_event`]).
fn told(shared: &Shared, scope: Option<&crate::memory_scope::ScopeLease>) -> Told {
    cut_event(shared, told_whole(shared, scope))
}

/// What a turn's chat is told, as the system gives it: for its admitted
/// scope when this route scopes context ([`Engine::with_memory_scope`]),
/// as before otherwise.
fn told_whole(shared: &Shared, scope: Option<&crate::memory_scope::ScopeLease>) -> Told {
    match (&shared.scoped_system, &shared.lease_system, scope) {
        (Some(system), _, Some(lease)) | (None, Some(system), Some(lease)) => system(lease),
        _ => (shared.system)(),
    }
}

/// `told` without the note's section for a dated follow-up that is
/// withheld now ([`Engine::asking_after_events`], `remember::event_withheld`):
/// before the person's first line decided it, and after a first line that
/// asked something (reviewer-two's P2 on #3059).
fn cut_event(shared: &Shared, told: Told) -> Told {
    match shared
        .event_withheld
        .as_ref()
        .and_then(|withheld| withheld())
    {
        Some(section) => told.without(&section),
        None => told,
    }
}

/// The scope [`told`] tells a turn in: its lease when this route scopes
/// context, none otherwise.
fn told_scope<'a>(
    shared: &Shared,
    scope: Option<&'a crate::memory_scope::ScopeLease>,
) -> Option<&'a crate::memory_scope::ScopeLease> {
    scope.filter(|_| shared.scoped_system.is_some())
}

/// The memory's note for the turn about to be told, waited for here, off
/// the worker's loop, at most what is left of its 300 ms from its ask
/// ([`NoteAhead::ready`]). Nothing without it.
async fn note_ready(shared: &Shared, scope: Option<&crate::memory_scope::ScopeLease>) {
    if let Some(ahead) = &shared.note_ahead {
        (ahead.ready)(told_scope(shared, scope).cloned()).await;
    }
}

/// What a turn's prepared chat is once `said` comes: begun on new early
/// words, kept on the same, dropped when the person went on.
fn prepared_on(
    shared: &Shared,
    (request, scope): (u64, Option<&crate::memory_scope::ScopeLease>),
    prepared: Option<Prepared>,
    said: net::Early,
    (notes, marks): (&[String], &Marks),
) -> Option<Prepared> {
    // Onboarding's sentence is never asked of the model, early or late.
    if marks.count_only.load(Ordering::Acquire) {
        return None;
    }
    // With `LAMP_PERSON_SCOPE_SEEN`, a person's line is not theirs until
    // their face is seen speaking it, which is read only at its end: no
    // memory, context or chat for them before that (Second's P2 on #3437).
    // The final words are asked as ever; a room's line may still prepare.
    if shared.scope_seen
        && scope
            .and_then(|lease| lease.scope())
            .is_some_and(|scope| scope.person().is_some())
    {
        return None;
    }
    match said {
        net::Early::Words(words) => {
            // Without the fillers, as the final words (#3126).
            let words = if shared.unfinished_words {
                crate::unfinished_words::without_fillers(&words)
            } else {
                words
            };
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
            // Memory is asked here, on the early words, off the final
            // words' path; the final words reuse it when they are these.
            let remembered = shared
                .remembered
                .as_ref()
                .filter(|_| !held_by_gap(scope))
                .and_then(|note| note(request, &words, scope));
            Some(Prepared::begin(shared, scope, words, notes, remembered))
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
    /// with `looking`; with `search`, a skill's search's own turn
    /// (`ProviderInput::Written`'s `search`); with `quoted`, an add-on's
    /// line, asked with no functions and running none (#2530 part 2).
    Written {
        looking: bool,
        search: bool,
        quoted: bool,
    },
    /// A due reminder, said as the Live route says it: a note to say it, and
    /// no tool runs (`provider_worker::reminder_note`).
    Reminder,
}

/// How long a turn's first round may take to begin, when it has no wait's
/// sound and is no look: [`WRITTEN_BEGIN_WITHIN`] for one the lamp wrote
/// itself with `written_wait`, else [`BEGIN_WITHIN`].
fn begin_within(heard: bool, written_wait: bool) -> Duration {
    if !heard && written_wait {
        WRITTEN_BEGIN_WITHIN
    } else {
        BEGIN_WITHIN
    }
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

impl Shared {
    /// A turn's tools line said ([`Engine::noting_turn_tools`]), and kept
    /// for a test that asked for it.
    fn turn_tools_said(&self, line: String) {
        #[cfg(test)]
        if let Some(journal) = &self.tools_journal {
            journal.lock().unwrap().push(line.clone());
        }
        eprintln!("{line}");
    }

    /// A named-ask journal line said (`LAMP_TASKS_NAMED_ASK`), and kept
    /// for a test that asked for it.
    fn named_ask_said(&self, line: String) {
        #[cfg(test)]
        if let Some(journal) = &self.named_ask_journal {
            journal.lock().unwrap().push(line.clone());
        }
        eprintln!("{line}");
    }
}

/// What a turn's journal line says of its tools
/// ([`Engine::noting_turn_tools`]): names and counts, never words.
#[derive(Default)]
struct TurnTools {
    /// Whether `remember` was given to any round's chat; `None` while no
    /// round has been asked, or from services that cannot say.
    remember_offered: Option<bool>,
    /// Each function called in any round, by name, and how many times.
    calls: std::collections::BTreeMap<String, usize>,
    /// The chat begun on the early words stood for round 1.
    prepared: bool,
}

impl TurnTools {
    /// One round, asked on `messages` without `without` (or, `quoted`, with
    /// no function at all), and its `calls`.
    fn round(
        &mut self,
        services: &dyn Services,
        messages: &[Message],
        without: &[&str],
        calls: &[net::ToolCall],
        quoted: bool,
    ) {
        // A quoted line's round declares no function at all (#2530).
        let declared = if quoted {
            Some(false)
        } else {
            services.declares(messages, without, crate::remember::REMEMBER)
        };
        if let Some(offered) = declared {
            self.remember_offered = Some(self.remember_offered.unwrap_or(false) || offered);
        }
        for call in calls {
            *self.calls.entry(function_name(&call.name)).or_default() += 1;
        }
    }

    fn line(&self, request: u64) -> String {
        let yes = |on: bool| if on { "y" } else { "n" };
        let calls = if self.calls.is_empty() {
            "none".to_owned()
        } else {
            self.calls
                .iter()
                .map(|(name, count)| format!("{name}:{count}"))
                .collect::<Vec<_>>()
                .join(",")
        };
        format!(
            "lamp-voice second: request {request}: tools: remember_offered={} calls={calls} prepared={}",
            self.remember_offered.map_or("?", yes),
            yes(self.prepared),
        )
    }
}

/// A called function's name as the journal may say it: the model writes
/// it, so one that is no function name (lower-case letters, digits and `_`,
/// at most 64) is `other`, never its words.
fn function_name(name: &str) -> String {
    if !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        name.to_owned()
    } else {
        "other".to_owned()
    }
}

/// The journal's line for what became of a line's named ask
/// (`LAMP_TASKS_NAMED_ASK`, #1722), once its round 1 has answered: the
/// agents its words name, in order, or none and how many words they were;
/// whether round 1 called `start_agent_task`, as it was told to, or what it
/// did instead; and, for each agent a call was given to
/// ([`given_to_the_named`]), whether one such call named another agent
/// (`true`) or each named nobody (`false`). No words.
fn named_ask_line(
    request: u64,
    named: &[&str],
    words: usize,
    called: &[&str],
    set: &[(bool, &str)],
) -> String {
    if named.is_empty() {
        return format!(
            "lamp-voice second: request {request}: named ask: no match ({words} words)"
        );
    }
    let agent = named.join(", ");
    let start = crate::agent_tasks::START;
    if !called.contains(&start) {
        let did = if called.is_empty() {
            "answered in words".to_owned()
        } else {
            format!("called {}", called.join(", "))
        };
        return format!(
            "lamp-voice second: request {request}: named ask: matched {agent}, not applied: round 1 {did}, not {start}"
        );
    }
    // Grouped by the agent given to, in the order first given.
    let mut given: Vec<(bool, &str)> = Vec::new();
    for &(another, to) in set {
        match given.iter_mut().find(|(_, agent)| *agent == to) {
            Some(group) => group.0 |= another,
            None => given.push((another, to)),
        }
    }
    let set: String = given
        .iter()
        .map(|(another, to)| {
            let named = if *another { "another agent" } else { "nobody" };
            format!("; the call named {named}, given to {to}")
        })
        .collect();
    format!("lamp-voice second: request {request}: named ask: matched {agent}{set}")
}

/// [`named_ask_line`] for a line held back, which no round answers.
fn named_ask_held_line(request: u64, named: &[&str], words: usize) -> String {
    if named.is_empty() {
        return named_ask_line(request, named, words, &[], &[]);
    }
    format!(
        "lamp-voice second: request {request}: named ask: matched {}, not applied: the line is held",
        named.join(", ")
    )
}

/// The round 1 calls of `start_agent_task` given to one of `asked`, the
/// agents a named ask's words name, in order: each given call, whether it
/// named another agent (`true`) or nobody (`false`), and to whom. A call
/// that already names one of `asked` is kept. With one asked, every other
/// call goes to it. With several (the reviewer on #2410: "ask Codex to
/// check the build and ask Claude Code to write a poem"), a call naming
/// nobody, or an agent the words don't ask, goes to the first asked, in
/// word order, that no round 1 call names yet, or else the first asked:
/// so a line's two tasks, one naming Codex and one nobody, go one to each.
fn given_to_the_named(
    calls: &mut [net::ToolCall],
    asked: &[&'static str],
) -> Vec<(bool, &'static str)> {
    let Some(&first) = asked.first() else {
        return Vec::new();
    };
    let who = |call: &net::ToolCall| {
        call.args
            .get(crate::agent_tasks::WHO)
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let mut named: Vec<&'static str> = calls
        .iter()
        .filter(|call| call.name == crate::agent_tasks::START)
        .filter_map(|call| {
            let who = who(call)?;
            asked.iter().copied().find(|agent| *agent == who)
        })
        .collect();
    let mut given = Vec::new();
    for call in calls
        .iter_mut()
        .filter(|call| call.name == crate::agent_tasks::START)
    {
        let who = who(call);
        if who.as_deref().is_some_and(|who| asked.contains(&who)) {
            continue;
        }
        let agent = asked
            .iter()
            .copied()
            .find(|agent| !named.contains(agent))
            .unwrap_or(first);
        named.push(agent);
        match call.args.as_object_mut() {
            Some(args) => {
                args.insert(crate::agent_tasks::WHO.to_owned(), Value::from(agent));
            }
            None => call.args = serde_json::json!({ crate::agent_tasks::WHO: agent }),
        }
        given.push((who.is_some(), agent));
    }
    given
}

/// The journal's line for a held line asked its one round: no words.
fn held_asked_line(request: u64) -> String {
    format!("lamp-voice second: request {request}: a held line names reports: asked once")
}

/// A held line's calls that withdraw consent, done though its answer stays
/// held, as the Live route does them ([`crate::attention::DONE_THOUGH_HELD`]):
/// one round told only those functions, its words never said, and only
/// those calls run, with no round after. Bounded by [`HELD_WITHIN`].
async fn done_though_held(
    shared: &Shared,
    context: (u64, Option<&crate::memory_scope::ScopeLease>),
    said: &str,
) {
    let (request, scope) = context;
    let gone = || scope.is_some_and(|scope| !scope.is_current());
    let began = Instant::now();
    let asking = async {
        if gone() {
            return None;
        }
        note_ready(shared, scope).await;
        if gone() {
            return None;
        }
        let mut messages = told(shared, scope).messages();
        if gone() {
            return None;
        }
        messages.push(Message::system(HELD_NOTE));
        messages.push(Message::user(said.to_owned()));
        let mut stream = shared
            .services
            .chat_only(messages, crate::attention::DONE_THOUGH_HELD);
        let mut calls = Vec::new();
        while let Some(event) = stream.next().await {
            match event {
                Ok(ChatEvent::ToolCalls(asked)) => calls = asked,
                Ok(ChatEvent::Done) => break,
                Ok(_) => {}
                Err(error) => {
                    eprintln!(
                        "lamp-voice second: request {request} held line's round failed: {error}"
                    );
                    return None;
                }
            }
        }
        let mut done = Vec::new();
        for call in calls
            .iter()
            .filter(|call| crate::attention::done_though_held(&call.name))
        {
            if gone() {
                return None;
            }
            let told = run_tool(shared, request, true, call, scope).await;
            done.push((call.name.clone(), told));
        }
        Some(done)
    };
    match tokio::time::timeout(HELD_WITHIN, asking).await {
        Ok(Some(done)) => {
            for (name, told) in done {
                eprintln!(
                    "lamp-voice second: request {request}: {name} {} though the answer was held (consent withdrawn), {} ms after the line",
                    if told.get("ok").is_some() {
                        "done"
                    } else {
                        "asked, not shown done,"
                    },
                    began.elapsed().as_millis(),
                );
            }
        }
        Ok(None) => {}
        Err(_) => eprintln!(
            "lamp-voice second: request {request} held line's round had not ended in {} ms; given up",
            HELD_WITHIN.as_millis()
        ),
    }
}

/// Where a skill's answer that asks for a look says what to do with the
/// picture (`lamp_skills::told`): its `next` goes on after this.
const PICTURE_COMES: &str = "When the picture comes:";
/// At most this many bytes of a skill's note for its picture.
const PICTURE_NOTE_MAX: usize = 1024;
/// Around a skill's note for its picture, after the look turn's question.
pub(crate) const PICTURE_NOTE_BEFORE: &str =
    "\n\n(The skill you called for this look said what to do with the picture: ";
pub(crate) const PICTURE_NOTE_AFTER: &str = ")";

/// What a skill's answer with a look says to do with its picture: the
/// words of its `next` after [`PICTURE_COMES`], at most
/// [`PICTURE_NOTE_MAX`] bytes, cut at a character's end. `None` without
/// them, or with nothing after them.
pub(crate) fn picture_note(answer: &Value) -> Option<String> {
    let (_, note) = answer["next"].as_str()?.split_once(PICTURE_COMES)?;
    let note = note.trim();
    let mut end = note.len().min(PICTURE_NOTE_MAX);
    while !note.is_char_boundary(end) {
        end -= 1;
    }
    let note = note[..end].trim_end();
    (!note.is_empty()).then(|| note.to_owned())
}

/// Asks the coordinator for `request`'s full look (`LookAsked`), made by a
/// call of `name`'s: dropped, and why, in a turn the lamp wrote itself
/// (unless `heard` says it may look all the same: a skill's search's own
/// turn) or one that is no longer the turn in hand. True when the look was
/// asked; then `note`, a skill's for the picture, is kept for the look turn.
/// A skill's call, `args` with it, goes with the ask, so the coordinator can
/// walk its search on.
async fn ask_look(
    shared: &Shared,
    request: u64,
    heard: bool,
    (name, args): (&str, Value),
    note: Option<String>,
) -> Result<bool, Stop> {
    // A skill's look is a search's, which may go on in the lamp's own
    // turns; the route's own look call is not.
    let search = name != LOOK_CALL;
    let session = shared
        .begun
        .lock()
        .ok()
        .and_then(|begun| *begun)
        .filter(|(of, _)| *of == request)
        .map(|(_, session)| session);
    let now = monotonic_us();
    let dropped = if !heard {
        Some("the turn is one the lamp wrote itself")
    } else if session.is_none() {
        Some("its turn is not the one in hand")
    } else if shared
        .search_pushed(|pushed| pushed.holds(request, heard, name, &args, now))
        .unwrap_or(false)
    {
        Some(crate::search_pushed::WHY)
    } else {
        None
    };
    // A skill's look asked is its search's newest, which a push may end.
    if search && dropped.is_none() {
        shared.search_pushed(|pushed| pushed.looked(request, name, &args, now));
    }
    eprintln!(
        "lamp-voice second: request {request} called {name}: {}",
        dropped.unwrap_or("the look is asked of the coordinator")
    );
    // Kept before the coordinator hears of the look, so its turn finds it.
    if dropped.is_none()
        && let Some(session) = session
        && let Ok(mut kept) = shared.look_note.lock()
    {
        *kept = note.map(|note| (session, note));
    }
    let asked_look = ProviderOutput::LookAsked {
        request,
        session: session.unwrap_or_default(),
        dropped: dropped.map(str::to_owned),
        search,
        call: search.then(|| crate::provider_worker::SearchCall {
            name: name.to_owned(),
            args,
        }),
    };
    if !shared.emit(asked_look).await {
        return Err(Stop::Gone);
    }
    Ok(dropped.is_none())
}

/// A forget the model called on a spoken turn whose own words don't ask for
/// one (`remember::asks_to_forget`), held in the engine for that person's
/// plain yes within `remember::HELD_FORGET_FOR`, and run as it was called
/// then. #2324, #2326 and #2331 let a yes confirm a forget by reading words,
/// and each was fooled, since the model writes the question; a held call
/// ties the yes to the forget the model tried to run (the first builder,
/// #1722 00:35).
struct HeldForget {
    call: net::ToolCall,
    request: u64,
    /// Whom it waits for: the admitted request's session and known person.
    asker: (u64, String),
    at: Instant,
    /// Whether the held turn's answer, said whole, asked them about a
    /// forget ([`settle_held_forget`]): until then no yes runs it (Fourth's
    /// P2 on #2340: a yes to "Want me to set a reminder?" ran the forget).
    asked: bool,
}

/// A person's spoken turn, for the turn after it: the forgets its own
/// words asked for by name (`remember::asks_to_forget` and
/// `remember::names_forget`), never their words.
struct SpokenBefore {
    request: u64,
    asker: Option<(u64, String)>,
    at: Instant,
    asked: Vec<&'static str>,
}

/// The forgets that `said` asks for and names: `forget_everything` and
/// `forget_my_face`, never a plain `forget`, which is always held.
fn forgets_asked(said: &str) -> Vec<&'static str> {
    if !crate::remember::asks_to_forget(said) {
        return Vec::new();
    }
    [
        crate::remember::FORGET_EVERYTHING,
        crate::known_faces::FORGET,
    ]
    .into_iter()
    .filter(|name| match *name {
        crate::remember::FORGET_EVERYTHING => crate::remember::asks_to_forget_everything(said),
        _ => crate::remember::names_forget(said, name),
    })
    .collect()
}

#[derive(Default)]
struct HeldForgets {
    /// The newest request that has looked at the hold: an older answer
    /// still running holds nothing past it.
    latest: u64,
    /// One at a time.
    held: Option<HeldForget>,
    /// The last spoken turn's words' ask, never the lamp's words: a turn
    /// the lamp wrote clears it ([`asked_just_before`]).
    before: Option<SpokenBefore>,
    /// Who the eyes saw saying the last spoken line read while a forget
    /// was held: its request, the faces seen speaking it and the known
    /// person with their face (`LAMP_EYES_SPEAKING=on`, #2377).
    sight: Option<SightRead>,
}

/// A line's request, the faces seen speaking it, and the known person's
/// name with their face's id ([`HeldForgets::sight`]).
type SightRead = (
    u64,
    crate::speaking_faces::Speakers,
    Option<(String, String)>,
);

/// Every function that reads or writes someone's memory, their rules or
/// their face, or grants something as them, never declared to or run for a
/// line held only by a known face's gap (the reviewer's P2s on #2449 and
/// #2616): it may not be theirs. That is every function the memory runs
/// ([`crate::remember::is_tool_with`]), and every one of the eyes' that
/// keeps, forgets or says yes for someone: their face, a yes to being named
/// in Who was here (run as the line's cached person, so a held line would
/// save the absent person's consent), and a yes to a face added from a photo.
/// Only the eyes' two refusals stay, `dont_tell_others_i_was_here` and
/// `photo_face_no`: a no grants nothing, whoever says it.
const MEMORY_CALLS: &[&str] = &[
    crate::remember::RECALL,
    crate::remember::REMEMBER,
    crate::remember::FORGET,
    crate::remember::FORGET_EVERYTHING,
    crate::rules::KEEP_RULE,
    crate::rules::FORGET_RULE,
    crate::rules::LIST_RULES,
    crate::known_faces::REMEMBER,
    crate::known_faces::FORGET,
    crate::known_faces::TELL,
    crate::photo_faces::YES,
];

/// The eyes' functions a line held only by a gap may still call: refusals,
/// which grant nothing ([`MEMORY_CALLS`]).
#[cfg(test)]
const HELD_REFUSALS: &[&str] = &[crate::known_faces::DONT_TELL, crate::photo_faces::NO];

/// What a memory call on a gap-held line is told instead of running.
const HELD_MEMORY_CALL: &str = "I can't see who is talking just now, so I can't look up, keep or forget anything, or take a yes, for anyone. Say so, and ask them to come back where I can see them.";

/// Whether this turn's scope is a known face's kept only by its held gap
/// (`LAMP_FACE_GAP_HOLD`): someone who left within 12 s of the lamp's last
/// answer to them may not be the one speaking, so the line is answered from
/// the conversation as it stood: no recall or search, no memory hits, and no
/// held forget run on it (the first builder's 22:10 and 01:43 on #2265).
fn held_by_gap(scope: Option<&crate::memory_scope::ScopeLease>) -> bool {
    scope
        .and_then(crate::memory_scope::ScopeLease::scope)
        .is_some_and(crate::memory_scope::ScopeSnapshot::held)
}

/// With `LAMP_MEMORY_EVENTS` ([`Engine::asking_after_events`], the first
/// builder on #2805, 22 Oct 20:28, and on #2602, 22 Oct 22:17): the line a
/// spoken request's own instructions get for the person's dated follow-up,
/// asked after the session's note is read, off the runtime's thread (whose
/// turn it is may read the face file). Never for a line held only by a
/// gap, which may be someone else's.
async fn event_line(
    shared: &Shared,
    request: u64,
    words: &str,
    scope: Option<&crate::memory_scope::ScopeLease>,
) -> Option<String> {
    let note = shared.event_turn.clone().filter(|_| !held_by_gap(scope))?;
    let (words, lease) = (words.to_owned(), scope.cloned());
    let line = tokio::task::spawn_blocking(move || note(request, &words, lease.as_ref()))
        .await
        .ok()
        .flatten()?;
    eprintln!(
        "lamp-voice second: request {request}: they only greeted, and the model is told to ask now how their dated follow-up went"
    );
    Some(line)
}

/// The turn's chat: every function, or none of memory's for a line held
/// only by a gap ([`held_by_gap`]).
fn chat_for(
    shared: &Shared,
    scope: Option<&crate::memory_scope::ScopeLease>,
    messages: Vec<Message>,
) -> futures_util::stream::BoxStream<'static, net::Result<ChatEvent>> {
    if held_by_gap(scope) {
        shared.services.chat_without(messages, MEMORY_CALLS)
    } else {
        shared.services.chat(messages)
    }
}

/// The functions [`chat_for`] leaves out for this scope.
fn withheld(scope: Option<&crate::memory_scope::ScopeLease>) -> &'static [&'static str] {
    if held_by_gap(scope) {
        MEMORY_CALLS
    } else {
        &[]
    }
}

/// Whom a turn's forget is held for, or whose yes runs one: the admitted
/// request's session and known person. None with nobody known (the room)
/// or no admitted scope: a forget isn't undone, so it takes no anonymous
/// yes, and such a turn is told to ask in words, as without the hold.
fn forget_asker(scope: Option<&crate::memory_scope::ScopeLease>) -> Option<(u64, String)> {
    // A scope held only by a known face's gap runs no one's forget: a yes
    // from out of the picture may not be theirs (the first builder's 01:43
    // on #2265).
    if held_by_gap(scope) {
        return None;
    }
    let scope = scope?;
    let person = scope.scope()?.person()?.to_owned();
    Some((scope.key().session, person))
}

/// What became of a held forget on a turn ([`take_held_forget`]).
enum Taken {
    /// Nothing held, or dropped: nothing runs and nothing is told.
    Nothing,
    /// Their yes runs exactly this call.
    Runs(net::ToolCall),
    /// A yes that would have run it, but the eyes did not see the held
    /// person's face say it (`LAMP_EYES_SPEAKING=on`, #2377): dropped, and
    /// the model is told the yes did not count.
    Unseen(String),
}

/// The held forget request `request` runs, if any. Every turn takes the
/// hold: it runs only on a later spoken turn that is a plain yes and
/// nothing else (`known_faces::plain_agreement`), from the same session's
/// person, within `remember::HELD_FORGET_FOR`; where the eyes read speaking
/// ([`Engine::confirming_forgets_by_sight`]), only with that person's face
/// seen saying it and no other face (`speaking_faces::seen_saying`). Any
/// other turn drops it, running nothing. The journal never has the words.
fn take_held_forget(
    shared: &Shared,
    request: u64,
    heard: bool,
    said: &str,
    asker: Option<&(u64, String)>,
    now: Instant,
) -> Taken {
    let Ok(mut hold) = shared.held_forget.lock() else {
        return Taken::Nothing;
    };
    hold.latest = hold.latest.max(request);
    let sight = hold.sight.take();
    let Some(held) = hold.held.take() else {
        return Taken::Nothing;
    };
    let dropped = if !heard {
        Some("a turn the lamp wrote")
    } else if !held.asked {
        Some("the lamp did not ask about it")
    } else if held.request >= request {
        Some("not a later turn")
    } else if !crate::known_faces::plain_agreement(said) {
        Some("not a plain yes")
    } else if asker != Some(&held.asker) {
        Some("not the same person")
    } else if now.saturating_duration_since(held.at) > crate::remember::HELD_FORGET_FOR {
        Some("too late")
    } else if shared.forget_sight.is_some()
        && !sight.is_some_and(|(seen, speakers, known)| {
            seen == request && crate::speaking_faces::seen_saying(&speakers, &held.asker.1, known)
        })
    {
        eprintln!(
            "lamp-voice second: request {request}: dropped a held forget ({}, held in request {}): their face was not seen saying the yes",
            held.call.name, held.request
        );
        return Taken::Unseen(held.call.name);
    } else {
        None
    };
    match dropped {
        Some(why) => {
            eprintln!(
                "lamp-voice second: request {request}: dropped a held forget ({}, held in request {}): {why}",
                held.call.name, held.request
            );
            Taken::Nothing
        }
        None => Taken::Runs(held.call),
    }
}

/// What came of holding a forget.
enum Holding {
    Held,
    /// One this turn already waits.
    OneWaits,
    /// A newer request has begun: nothing is held for this one.
    Stale,
}

/// Holds `call` for `asker`'s yes, in place of one an earlier turn held.
fn hold_forget(
    shared: &Shared,
    request: u64,
    call: &net::ToolCall,
    asker: (u64, String),
    now: Instant,
) -> Holding {
    let Ok(mut hold) = shared.held_forget.lock() else {
        return Holding::Stale;
    };
    if hold.latest != request {
        return Holding::Stale;
    }
    if let Some(held) = &hold.held {
        if held.request == request {
            return Holding::OneWaits;
        }
        eprintln!(
            "lamp-voice second: request {request}: dropped a held forget ({}, held in request {}): a new held call",
            held.call.name, held.request
        );
    }
    eprintln!(
        "lamp-voice second: request {request}: held a forget ({}) for their yes",
        call.name
    );
    hold.held = Some(HeldForget {
        call: call.clone(),
        request,
        asker,
        at: now,
        asked: false,
    });
    Holding::Held
}

/// The held turn `request` is over: its hold stands only if `answer`, the
/// lamp's words said whole, asks them about a forget
/// (`remember::asks_about_forgetting`: a question that names one). An
/// answer cut, failed or held (`None`) or one that asks anything else drops
/// it. On top of the call, so what the model writes never widens it.
fn settle_held_forget(shared: &Shared, request: u64, answer: Option<&str>) {
    let Ok(mut hold) = shared.held_forget.lock() else {
        return;
    };
    let Some(held) = hold
        .held
        .as_mut()
        .filter(|held| held.request == request && !held.asked)
    else {
        return;
    };
    let why = match answer {
        Some(answer) if crate::remember::asks_about_forgetting(answer, &held.call.name) => {
            held.asked = true;
            return;
        }
        Some(_) => "the lamp's answer asked nothing about it",
        None => "its answer did not finish",
    };
    eprintln!(
        "lamp-voice second: request {request}: dropped a held forget ({}, held in request {request}): {why}",
        held.call.name
    );
    hold.held = None;
}

/// The forgets the same person's own spoken turn just before `request`, in
/// the same session and within `remember::HELD_FORGET_FOR`, asked for in
/// words and named (`asked`, [`forgets_asked`] of their words alone): one
/// of those the model calls on this turn runs at once, never asked twice
/// (Fourth's P3 on #2340: "Forget everything about me." "Everything,
/// really?" "Yes."). Records this turn for the next; a turn the lamp wrote
/// records nothing of its words and clears the one before. Nobody known,
/// nothing counts.
fn asked_just_before(
    shared: &Shared,
    request: u64,
    heard: bool,
    asked: Vec<&'static str>,
    asker: Option<&(u64, String)>,
    now: Instant,
) -> Vec<&'static str> {
    let Ok(mut hold) = shared.held_forget.lock() else {
        return Vec::new();
    };
    if hold
        .before
        .as_ref()
        .is_some_and(|before| before.request >= request)
    {
        return Vec::new();
    }
    let before = hold.before.take();
    hold.before = heard.then(|| SpokenBefore {
        request,
        asker: asker.cloned(),
        at: now,
        asked,
    });
    match before {
        Some(before)
            if heard
                && asker.is_some()
                && before.asker.as_ref() == asker
                && now.saturating_duration_since(before.at) <= crate::remember::HELD_FORGET_FOR =>
        {
            before.asked
        }
        _ => Vec::new(),
    }
}

/// One call run by the route's runner: the admitted request's scoped one
/// when there is one.
async fn run_tool(
    shared: &Shared,
    request: u64,
    heard: bool,
    call: &net::ToolCall,
    scope: Option<&crate::memory_scope::ScopeLease>,
) -> Value {
    // A memory call on a line held only by a gap runs for no one, however
    // it was asked: not declared to it, and refused if written anyway.
    if held_by_gap(scope) && MEMORY_CALLS.contains(&call.name.as_str()) {
        eprintln!(
            "lamp-voice second: request {request}: a memory call on a line held only by a face's gap runs for no one"
        );
        return serde_json::json!({ "error": HELD_MEMORY_CALL });
    }
    match (&shared.scoped_tools, scope) {
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
        (None, _) => (shared.tools)(request, heard, call.name.clone(), call.args.clone()).await,
    }
}

/// How an answer stopped short.
enum Stop {
    /// The first round had not begun within its bound ([`BEGIN_WITHIN`]):
    /// the route gave the turn up, not the model. It ends as a silent turn,
    /// but what was said is no talk the lamp left alone.
    GaveUp,
    /// `not_for_me`: nothing more is said, and the model judged the line
    /// not said to the lamp.
    Declined,
    /// The Policy held one of the turn's calls (#1984): nothing more is
    /// said. It ends as a silent turn; the Policy, which held it, is told
    /// no other verdict.
    Held,
    Failed(Fault),
    /// The engine is gone.
    Gone,
}

/// One wait sound for `request`, as audio only: no words, so nothing of it
/// is kept in the thread, and it does not count as an answer for the
/// announcement's spell.
async fn cue(shared: &Shared, request: u64, clip: Vec<i16>) -> Result<(), Stop> {
    // Its time counts in what was heard of the answer, with no words.
    shared.said_piece(request, None);
    if let Ok(mut only) = shared.wait_sound_only.lock() {
        *only = Some(request);
    }
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
        shared.said_samples(request, part.len());
    }
    Ok(())
}

/// A round's held sentences ([`crate::remember::OfferHold`]) at its end:
/// an offer to keep beside a `remember` call is left out, and logged.
fn offered(
    hold: &mut crate::remember::OfferHold,
    remembers: bool,
    request: u64,
    round: usize,
) -> Vec<String> {
    let (said, left_out) = hold.release(remembers);
    if left_out > 0 {
        eprintln!(
            "lamp-voice second: request {request} chat round {round}: {left_out} offer(s) to keep what its remember call keeps: not said (K2)"
        );
    }
    said
}

/// Chat, speech and the turn's last events, for the person's `said`, with
/// the picture of a look (`looking`). The picture goes in this request only,
/// never into the thread or a later turn's history.
// Provenance and the prepared round remain separate from the turn's admission
// kind; carried answers can be spoken without receiving audio authority.
#[allow(clippy::too_many_arguments)]
async fn answer(
    shared: &Shared,
    context: (u64, Option<crate::memory_scope::ScopeLease>),
    said: String,
    asked: Asker,
    spoken: Option<SpokenTurn>,
    picture: Option<Vec<u8>>,
    notes: Vec<String>,
    times: Arc<Mutex<Times>>,
    prepared: Option<Prepared>,
    memory_write_owner: Option<WriteOwner>,
) {
    let (request, scope) = context;
    let heard = asked == Asker::Spoken;
    let looking = matches!(asked, Asker::Written { looking: true, .. });
    let searching = matches!(asked, Asker::Written { search: true, .. });
    let quoted = matches!(asked, Asker::Written { quoted: true, .. });
    let heard_at = Instant::now();
    // The request's own words, for a forget it calls (#3113): never the
    // next line, which may be someone else's.
    if heard && let Ok(mut thread) = crate::thread::THREAD.lock() {
        thread.line_of(request, &said);
    }
    // Its speech socket opened now, while the chat thinks, with the switch
    // (`LAMP_SECOND_SPEECH_SOCKET`).
    shared.services.open_speech_ahead(request);
    // Started now, with the chat request, so it is ready first.
    let mut speech = JoinSet::new();
    let announcement = (shared.announce && shared.announcement_due()).then(|| {
        start_speech(
            shared,
            &mut speech,
            ANNOUNCEMENT.to_owned(),
            net::Turn { request, heard_at },
        )
    });
    let (sentences, mut queued) = mpsc::channel(SENTENCES_AHEAD);
    let spoke = AtomicBool::new(false);
    // The answer's own first audio went (not the announcement's).
    let answered = AtomicBool::new(false);
    // Whether a tool ran for this line: one declined after is the lamp's.
    let ran = AtomicBool::new(false);
    let ran_tool = &ran;
    // The words of the answer's sentences begun so far, for a call the
    // Policy holds after them.
    let words_begun = AtomicUsize::new(0);
    let words_out = &words_begun;
    // Accepted private audio or a confirmed held forget keeps the turn acted
    // after a later decline.
    let acted = AtomicBool::new(false);
    let dispatched = &acted;
    let mut queue = Queue {
        shared,
        speech: &mut speech,
        announcement,
        sentences,
        started: false,
        request,
        heard_at,
        tags: shared.speech_tags.map(speech_tags::Tags::new),
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
    // A forget runs only on a spoken turn whose words ask for it (lamp-4ace,
    // 20 Oct 21:31: a question was answered with two forgets). One the model
    // calls unasked is held for that person's plain yes ([`HeldForget`]);
    // a yes with nothing held forgets nothing.
    // One the same person asked for in words on their turn just before
    // runs at once ([`asked_just_before`]).
    let asks_now = crate::remember::asks_to_forget(&said);
    // What these words, the request's own, say of forgetting everything
    // ([`crate::remember::everything_asked`]): `Some(false)`, a refusal or
    // a smaller ask such as "forget that", outweighs an ask just before and
    // a held yes; `Some(true)` asks for it now.
    let everything_now = heard
        .then(|| crate::remember::everything_asked(&said))
        .flatten();
    let asker = forget_asker(scope.as_ref());
    let asked_before = asked_just_before(
        shared,
        request,
        heard,
        forgets_asked(&said),
        asker.as_ref(),
        (shared.clock)(),
    );
    let forget_may_run = crate::remember::forget_may_run(heard, asks_now);
    let (mut held_forget_runs, unseen_yes) = match take_held_forget(
        shared,
        request,
        heard,
        &said,
        asker.as_ref(),
        (shared.clock)(),
    ) {
        Taken::Nothing => (None, None),
        // Their yes to a held `forget_everything` does not run it when the
        // same words say not to ("Yes. No, don't forget everything.").
        Taken::Runs(call)
            if call.name == crate::remember::FORGET_EVERYTHING
                && everything_now == Some(false)
                && !crate::remember::forget_unguarded() =>
        {
            eprintln!(
                "lamp-voice second: request {request}: dropped a held forget ({}): their words say not to",
                call.name
            );
            (None, None)
        }
        Taken::Runs(call) => (Some(call), None),
        Taken::Unseen(name) => (None, Some(crate::remember::unseen_yes_told(&name))),
    };
    // Whom the line's own words ask by name (`LAMP_TASKS_NAMED_ASK`): told
    // once, after round 1 answers.
    let mut named_ask = shared
        .named_ask
        .as_ref()
        .map(|named| (named(&said), said.split_whitespace().count()));
    // Whether the line began with the lamp's name and asks for no silence
    // to be broken (`LAMP_NO_WORDS_AGAIN`): "Lamp, stop." or "Lamp, never
    // mind." is rightly answered with nothing (the Reviewer-lead's P2 on
    // #3712).
    let by_name = crate::attention::begins_with_the_name(&said) && !says_stop(&said);
    let words = Message::user(said.clone());
    let event_words = said.clone();
    // One of the onboarding walk's fixed lines, written by the lamp: if its
    // chat has not begun in time, its exact words are said as they are
    // rather than the walk's line left unsaid (lamp-52e6, 28 Oct 16:42).
    // Never a person's line, a reminder, a look or an add-on's quoted line.
    let mut fixed = matches!(
        asked,
        Asker::Written {
            looking: false,
            search: false,
            quoted: false
        }
    )
    .then(|| crate::onboarding::fixed_words(&said).map(str::to_owned))
    .flatten();
    // A chat begun on the early words was asked without the held forget's
    // result, or that its yes did not count, so it does not stand for
    // round 1.
    let prepared_for = prepared
        .filter(|_| held_forget_runs.is_none() && unseen_yes.is_none())
        .map(|prepared| (prepared, said.clone()));
    let question = match picture {
        Some(jpeg) => Message::user_seeing(said, &jpeg),
        None => Message::user(said),
    };
    let writing_times = times.clone();
    // Whether the model called anything in the turn, any round.
    let called_any = AtomicBool::new(false);
    let called = &called_any;
    // The turn's tools, for its journal line (`LAMP_MEMORY_REPLACES`).
    let turn_tools = Mutex::new(TurnTools::default());
    let tools_noted = shared.note_tools.then_some(&turn_tools);
    // `LAMP_NOT_FOR_ME_WORDS`: on a line told it may be people's talk, the
    // reply's text is held until its opening is known: "not for me" written
    // as words is the decline itself (lamp-4ace, r06, request 131). On any
    // other line, or with the switch off, nothing is held.
    let mut opening = (shared.declines_in_words
        && notes.iter().any(|note| note == worker::CONVERSATION_ALONE))
    .then(String::new);
    let writing = async move {
        if scope.as_ref().is_some_and(|scope| !scope.is_current()) {
            return Err(Stop::Gone);
        }
        // The memory's note asked as the line began, waited for here, never
        // on the loop: building the context below reads nothing.
        note_ready(shared, scope.as_ref()).await;
        if scope.as_ref().is_some_and(|scope| !scope.is_current()) {
            return Err(Stop::Gone);
        }
        let told = told_whole(shared, scope.as_ref());
        // Building it can wait on the memory service: a request revoked
        // meanwhile is not asked with what was read for it.
        if scope.as_ref().is_some_and(|scope| !scope.is_current()) {
            return Err(Stop::Gone);
        }
        // A dated follow-up's line, last among the notes: asked only now,
        // once the session's note is in (read as the line began or as the
        // turn is told, its follow-up read for this answered turn, and
        // taken just above), as a session's first line is the one that
        // opens it and the offer is made as it opens. Then the note's
        // section for it is cut unless that line only greeted
        // (reviewer-two's P2 on #3059).
        let mut notes = notes;
        if heard && let Some(line) = event_line(shared, request, &event_words, scope.as_ref()).await
        {
            notes.push(line);
        }
        let mut messages = cut_event(shared, told).messages();
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
        // The held forget their yes runs, before round 1, as any call of a
        // spoken turn: never on a line the rule holds, and with the Policy's
        // decision.
        let mut held_forget_ran = false;
        if let Some(call) = held_forget_runs.take() {
            let by_rule = shared
                .hold
                .as_ref()
                .is_some_and(|hold| hold(request, &call.name));
            let policed = match &shared.policy {
                Some(gate) if !by_rule => gate(request, call.name.clone()).await,
                _ => Policed::Run,
            };
            if scope.as_ref().is_some_and(|scope| !scope.is_current()) {
                return Err(Stop::Gone);
            }
            if by_rule || policed == Policed::Held {
                eprintln!(
                    "lamp-voice second: request {request}: dropped a held forget ({}): the line is held",
                    call.name
                );
                if policed == Policed::Held {
                    return Err(Stop::Held);
                }
            } else if writes_memory(&call.name)
                && reaches_memory(shared, &call.name, scope.as_ref())
                && !claims_write(memory_write_owner.as_ref(), request)
            {
                // Their yes is these words' memory write too ([`WriteOwner`]).
                eprintln!(
                    "lamp-voice second: request {request}: dropped a held forget ({}): a memory write was already tried for these words",
                    call.name
                );
            } else {
                ran_tool.store(true, Ordering::Relaxed);
                dispatched.store(true, Ordering::Relaxed);
                // Their yes to a held `forget_everything`, not refused by
                // these words above: the request's own ask for everything,
                // so it takes the face too (#3564). Not on a line held only
                // by a gap, whose memory call `run_tool` refuses.
                if call.name == crate::remember::FORGET_EVERYTHING
                    && heard
                    && !crate::remember::forget_unguarded()
                    && !held_by_gap(scope.as_ref())
                    && let Some(scope) = scope.as_ref()
                {
                    scope.mark_everything_asked(&EverythingShown(()));
                }
                let result = run_tool(shared, request, heard, &call, scope.as_ref()).await;
                if scope.as_ref().is_some_and(|scope| !scope.is_current()) {
                    return Err(Stop::Gone);
                }
                eprintln!(
                    "lamp-voice second: request {request}: ran a held forget ({})",
                    call.name
                );
                messages.push(Message::system(format!(
                    "(For you to know: they said yes, so the {} you asked them about has run, with this result: {result}. Say in a few words that it is done, and do not call it again.)",
                    call.name
                )));
                held_forget_ran = true;
            }
        }
        // Their yes did not count (#2377): told once, beside the request.
        if let Some(told) = unseen_yes {
            messages.push(Message::system(told));
        }
        messages.push(question);
        // What round 1 was asked, kept for the rounds after it.
        if let Some(prepared) = &prepared {
            messages.clone_from(&prepared.messages);
        }
        let asked_at = messages.len() - 1;
        // Round 1's bounds run from here, or from its one ask again after a
        // line lost on the network (`LAMP_RESEND_LOST_LINE`).
        let mut from = begun;
        let mut resent = false;
        // A `not_for_me` not taken, once (#2191).
        let mut named_asked_again = false;
        // A line said to the lamp by name answered with no words, asked once
        // more (`LAMP_NO_WORDS_AGAIN`, #3704).
        let mut no_words_asked_again = false;
        // Whether an earlier round of the turn called anything: a turn
        // answered with its calls' own line is one whose first calls are
        // all short (`LAMP_SHORT_DONE`, #2877).
        let mut called_earlier = false;
        // The calls' own line, kept while the round after them is asked
        // whether the request held more (the Reviewer-lead's P2 on #2930):
        // said only if that round adds no words and no calls.
        let mut short_line: Option<String> = None;
        // Whether a `remember` of this request kept something, and whether
        // a reply that said it kept something with none was asked again
        // once ([`crate::remember::claims_a_keep`], finding K on #3504).
        let mut kept_now = false;
        let mut keep_claim_asked = false;
        // Told before the round after one whose claim went unsaid beside
        // other calls, once those calls are answered.
        let mut keep_claim_note = false;
        // A reply's offer to keep something, held to its round's end and
        // not said beside a `remember` call that keeps it (finding K2 on
        // #3504), and whether a `remember` of this request ran.
        let mut offer_hold = crate::remember::OfferHold::default();
        let mut remember_ran = false;
        let mut round = 0;
        while round < MAX_ROUNDS {
            if std::mem::take(&mut keep_claim_note) {
                messages.push(Message::system(crate::remember::KEEP_CLAIMED_UNKEPT));
            }
            if scope.as_ref().is_some_and(|scope| !scope.is_current()) {
                return Err(Stop::Gone);
            }
            round += 1;
            let mut refused = false;
            let mut stream = match prepared.take() {
                // A quoted line's every round declares no function at all.
                _ if quoted => shared.services.chat_only(messages.clone(), &[]),
                Some(prepared) if round == 1 => {
                    if let Some(noted) = tools_noted
                        && let Ok(mut noted) = noted.lock()
                    {
                        noted.prepared = true;
                    }
                    prepared.into_events()
                }
                _ => chat_for(shared, scope.as_ref(), messages.clone()),
            };
            let mut first = true;
            let mut split = Sentences::default();
            let mut text = String::new();
            let mut calls = Vec::new();
            let mut reasoning = None;
            let mut called_us = 0;
            // A reply's opening, held until it is known whether it says
            // something was kept; one that does is held to the round's end,
            // when its calls say whether it was. Not while something of
            // this request was kept already, nor on a turn offered no
            // function (a quoted add-on's or a reminder's).
            let mut claim_held =
                (!kept_now && !quoted && asked != Asker::Reminder).then(String::new);
            loop {
                let next = if first && round == 1 {
                    let at = |after: Duration| from + after;
                    let cues = shared.cues.as_ref().filter(|_| heard);
                    let first_wait = if cues.is_some() {
                        shared.cue_after
                    } else if looking {
                        LOOK_BEGIN_WITHIN
                    } else {
                        begin_within(heard, shared.written_wait)
                    };
                    let mut next = tokio::time::timeout_at(at(first_wait), stream.next()).await;
                    if next.is_err()
                        && let Some(cues) = cues
                    {
                        // Both bounds are measured from the words. A ready
                        // sound leaves three seconds after its scheduled
                        // time; without one, never set a deadline earlier
                        // than the wait already spent checking for it.
                        let bound = match cues.next() {
                            Some(clip) => {
                                eprintln!(
                                    "lamp-voice second: request {request} made a sound while its chat had not begun in {} ms",
                                    began.elapsed().as_millis()
                                );
                                reached(&writing_times, |times| &mut times.cue);
                                cue(shared, request, clip).await?;
                                shared.cue_after + BEGIN_WITHIN_AFTER_CUE
                            }
                            // No sound ready yet (the first turn after a
                            // start, its clips still rendering): the same
                            // time a ready sound gives, not less, so a cold
                            // chat that begins late is answered (#3755: the
                            // first turn after a restart had 3 s, the rest
                            // 4.5 s, while its chat was coldest).
                            None => shared.cue_after + BEGIN_WITHIN_AFTER_CUE,
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
                        Err(_) if fixed.is_some() => {
                            eprintln!(
                                "lamp-voice second: request {request} chat round {round} had not begun in {} ms; the onboarding line's own words are said",
                                began.elapsed().as_millis()
                            );
                            shared.emit(ProviderOutput::OwnWordsSaid { request }).await;
                            queue.push(fixed.take().unwrap_or_default()).await?;
                            return Ok(());
                        }
                        Err(_) => {
                            eprintln!(
                                "lamp-voice second: request {request} chat round {round} had not begun in {} ms; the turn ends unanswered",
                                began.elapsed().as_millis()
                            );
                            say_down(shared, request, heard).await?;
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
                    Ok(event) => event,
                    Err(error) => {
                        eprintln!(
                            "lamp-voice second: request {request} chat round {round} failed after {} ms: {error}",
                            began.elapsed().as_millis()
                        );
                        // Lost on the network before any word or call: asked
                        // once more if the link is back in time (#1722).
                        if first
                            && round == 1
                            && !resent
                            && heard
                            && let Some(back) = &shared.resend
                            && link::lost(&fault(&error))
                        {
                            resent = true;
                            if link_back_in_time(shared, request, back).await {
                                if scope.as_ref().is_some_and(|scope| !scope.is_current()) {
                                    return Err(Stop::Gone);
                                }
                                eprintln!(
                                    "lamp-voice second: request {request} lost on the network ({error}): the link is back, asked once more"
                                );
                                from = tokio::time::Instant::now();
                                stream = chat_for(shared, scope.as_ref(), messages.clone());
                                continue;
                            }
                        }
                        // An onboarding fixed line whose chat failed before
                        // any word (a 402 in 0.9 s, lamp-52e6 29 Oct 07:35)
                        // is said in its own words, as one not begun in time.
                        if first
                            && round == 1
                            && let Some(words) = fixed.take()
                        {
                            eprintln!(
                                "lamp-voice second: request {request} chat round {round} failed; the onboarding line's own words are said"
                            );
                            shared.emit(ProviderOutput::OwnWordsSaid { request }).await;
                            queue.push(words).await?;
                            return Ok(());
                        }
                        // A chat that answers with a refusal before any word
                        // is as down as one that never begins: a line, not
                        // silence (#1844's B). The slow line only for one
                        // that passes (429, a 5xx); a key, credit or request
                        // refused says it can't answer, with no promise
                        // (the Reviewer-lead's P3 on #3771).
                        if first
                            && round == 1
                            && let net::Error::Status(status) = error
                        {
                            if passing(status) {
                                say_down(shared, request, heard).await?;
                            } else {
                                say_cannot(shared, request, heard).await?;
                            }
                        }
                        return Err(Stop::Failed(fault(&error)));
                    }
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
                        // `LAMP_NOT_FOR_ME_WORDS`: nothing goes to the voice
                        // until the reply's opening is known; then what was
                        // held goes on as one piece.
                        let piece = match opening.as_mut() {
                            None => piece,
                            Some(held) => {
                                held.push_str(&piece);
                                match crate::not_speech::opens_not_for_me(held, false) {
                                    None => continue,
                                    Some(true) => {
                                        eprintln!(
                                            "lamp-voice second: request {request}: its reply opens with \"not for me\" on a line let out only as the conversation going on: taken as not_for_me, nothing said (LAMP_NOT_FOR_ME_WORDS)"
                                        );
                                        return Err(Stop::Declined);
                                    }
                                    Some(false) => opening.take().unwrap_or_default(),
                                }
                            }
                        };
                        let piece = match claim_held.as_mut() {
                            None => piece,
                            Some(held) => {
                                held.push_str(&piece);
                                match crate::remember::claims_a_keep(held, false) {
                                    Some(false) => claim_held.take().unwrap_or_default(),
                                    // Undecided, or a claim: held.
                                    _ => continue,
                                }
                            }
                        };
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
                        // An opening that may yet offer to keep waits for
                        // its sentence's end (K2): "Want me to keep" alone
                        // would be said before its offer is known.
                        if sentences.is_empty()
                            && !queue.started
                            && !crate::remember::may_offer_a_keep(split.pending())
                        {
                            sentences.extend(split.clause().or_else(|| {
                                shared.first_words.and_then(|words| split.words(words))
                            }));
                        }
                        for sentence in sentences {
                            if let Some(sentence) = offer_hold.say(sentence, remember_ran) {
                                queue.push(sentence).await?;
                            }
                        }
                    }
                    ChatEvent::ToolCalls(asked) => {
                        called_us = monotonic_us();
                        calls = asked;
                    }
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
            // The round ended with its opening still not known ("Not for"):
            // whole now, and said unless it is the decline.
            if let Some(held) = opening.take_if(|held| !held.is_empty()) {
                if crate::not_speech::opens_not_for_me(&held, true) == Some(true) {
                    eprintln!(
                        "lamp-voice second: request {request}: its reply is \"not for me\" on a line let out only as the conversation going on: taken as not_for_me, nothing said (LAMP_NOT_FOR_ME_WORDS)"
                    );
                    return Err(Stop::Declined);
                }
                for sentence in split.push(&held) {
                    if let Some(sentence) = offer_hold.say(sentence, remember_ran) {
                        queue.push(sentence).await?;
                    }
                }
            }
            // A reply that says it kept something (finding K on #3504:
            // lamp-06c1, 27 Oct, "Kept: the meeting room is called Orange."
            // with no call, 2 of 4): said only beside a `remember` call. With
            // none, its words are not said. Beside other calls, those run
            // and the next round is told nothing was kept (the reviewer-
            // lead's P2 on #3628). Alone, the model is told once and asked
            // again; a second such reply, or one in the last round, is said
            // as the plain line that it was not kept.
            if let Some(held) = claim_held.take().filter(|held| !held.is_empty()) {
                let remembers = calls
                    .iter()
                    .any(|call| call.name == crate::remember::REMEMBER);
                // A soft claim ("Noted — timer set for ten minutes.") beside
                // any call confirms that call (Reviewer-three's and
                // Reviewer-two's P2 on #4137): a claim only with none.
                let claimed = match crate::remember::keep_claim(&held) {
                    Some(crate::remember::KeepClaim::Hard) => true,
                    Some(crate::remember::KeepClaim::Soft) => calls.is_empty(),
                    None => false,
                };
                if claimed && !remembers {
                    if !calls.is_empty() {
                        eprintln!(
                            "lamp-voice second: request {request} chat round {round}: its reply says it kept something, with no remember call: not said, its other calls run"
                        );
                        keep_claim_asked = true;
                        keep_claim_note = true;
                        text.clear();
                    } else if round < MAX_ROUNDS && !std::mem::replace(&mut keep_claim_asked, true)
                    {
                        eprintln!(
                            "lamp-voice second: request {request} chat round {round}: its reply says it kept something, with no remember call: not said, asked again"
                        );
                        messages.push(Message::system(crate::remember::KEEP_CLAIMED_UNKEPT));
                        continue;
                    } else {
                        eprintln!(
                            "lamp-voice second: request {request} chat round {round}: its reply says again it kept something, with no remember call: said it was not kept"
                        );
                        // What an earlier round held is said first: nothing
                        // was kept (Reviewer-two's P3 on #4076).
                        for sentence in offered(&mut offer_hold, false, request, round) {
                            queue.push(sentence).await?;
                        }
                        queue
                            .push(crate::remember::NOT_KEPT_LINE.to_owned())
                            .await?;
                        return Ok(());
                    }
                } else {
                    for sentence in split.push(&held) {
                        if let Some(sentence) = offer_hold.say(sentence, remember_ran) {
                            queue.push(sentence).await?;
                        }
                    }
                }
            }
            // Asked on `messages` as they still are: what the round's chat
            // was given, and what it called.
            if let Some(noted) = tools_noted
                && let Ok(mut noted) = noted.lock()
            {
                noted.round(
                    shared.services.as_ref(),
                    &messages,
                    withheld(scope.as_ref()),
                    &calls,
                    quoted,
                );
            }
            // Round 1 of a named ask: its hand-over goes to the agent named,
            // and the journal says what became of it (#1722).
            if round == 1
                && let Some((named, count)) = named_ask.take()
            {
                let set = given_to_the_named(&mut calls, &named);
                let names: Vec<&str> = calls.iter().map(|call| call.name.as_str()).collect();
                shared.named_ask_said(named_ask_line(request, &named, count, &names, &set));
            }
            // Before the unfinished last words are said.
            if !calls.is_empty() {
                let names: Vec<&str> = calls.iter().map(|call| call.name.as_str()).collect();
                eprintln!(
                    "lamp-voice second: request {request} chat round {round} called {}",
                    names.join(", ")
                );
            }
            // A turn that called anything, a look by its call included, is no
            // holding answer (#1712; the peer read on #1986).
            if !calls.is_empty() {
                called.store(true, Ordering::Relaxed);
            }
            let first_calls = !called_earlier;
            called_earlier |= !calls.is_empty();
            if calls.iter().any(|call| call.name == NOT_FOR_ME) {
                let named = heard
                    && shared
                        .named_to_a_face
                        .as_ref()
                        .is_some_and(|named| named(request));
                if !named || std::mem::replace(&mut named_asked_again, true) {
                    return Err(Stop::Declined);
                }
                // Called by name while faced (#2191): the decline is not
                // taken, and the model is asked once more. Every call of
                // the round is answered so, none run: the next round calls
                // again what it needs.
                eprintln!(
                    "lamp-voice second: request {request} chat round {round}: not_for_me not taken: it began with the lamp's name said to a face turned to the lamp (LAMP_NAMED_ALWAYS); asked once more"
                );
                messages.push(Message::assistant_calls_with(
                    Some(std::mem::take(&mut text)),
                    &calls,
                    reasoning.take(),
                ));
                for call in &calls {
                    messages.push(Message::tool(
                        call.id.clone(),
                        NAMED_TO_A_FACE_ANSWER.to_owned(),
                    ));
                }
                continue;
            }
            // A look asked by its own call (#1712): the coordinator takes the
            // turn's full look, and the turn ends here; its picture comes in
            // the look turn after it. Only in a turn someone spoke.
            let remembers = remember_ran
                || calls
                    .iter()
                    .any(|call| call.name == crate::remember::REMEMBER);
            if shared.look_by_call && calls.iter().any(|call| call.name == LOOK_CALL) {
                if let Some(rest) = split.finish()
                    && let Some(rest) = offer_hold.say(rest, remember_ran)
                {
                    queue.push(rest).await?;
                }
                for sentence in offered(&mut offer_hold, remembers, request, round) {
                    queue.push(sentence).await?;
                }
                if ask_look(shared, request, heard, (LOOK_CALL, Value::Null), None).await?
                    && !queue.started
                {
                    queue.push(LOOKING.to_owned()).await?;
                }
                return Ok(());
            }
            if let Some(rest) = split.finish()
                && let Some(rest) = offer_hold.say(rest, remember_ran)
            {
                queue.push(rest).await?;
            }
            for sentence in offered(&mut offer_hold, remembers, request, round) {
                queue.push(sentence).await?;
            }
            // The round after a turn's calls were all answered short
            // (`LAMP_SHORT_DONE`, #2877): with no words and no calls, the
            // request held nothing more, and the calls' own line is the
            // answer. Words ("dim the light and tell me a joke") or calls
            // go on as without the switch, the line unsaid.
            if let Some(line) = short_line.take()
                && calls.is_empty()
                && !text.chars().any(char::is_alphanumeric)
            {
                eprintln!(
                    "lamp-voice second: request {request} chat round {round} added nothing to the calls answered short; said \"{line}\" (LAMP_SHORT_DONE)"
                );
                queue.push(line).await?;
                return Ok(());
            }
            // No words and no call, after nothing said or called in the
            // turn, to a line that began with the lamp's name: never silence
            // (`LAMP_NO_WORDS_AGAIN`, #3704). Asked once more; with no words
            // again, the lamp says it lost them.
            if calls.is_empty()
                && shared.no_words_again
                && asked == Asker::Spoken
                && !called_earlier
                && !queue.started
                && !text.chars().any(char::is_alphanumeric)
                && by_name
            {
                if !std::mem::replace(&mut no_words_asked_again, true) {
                    eprintln!(
                        "lamp-voice second: request {request} chat round {round}: no words and no call for a line begun with the lamp's name; asked once more (LAMP_NO_WORDS_AGAIN)"
                    );
                    messages.push(Message::system(NO_WORDS_AGAIN_NOTE.to_owned()));
                    continue;
                }
                eprintln!(
                    "lamp-voice second: request {request} chat round {round}: no words again for a line begun with the lamp's name; said \"{NO_WORDS_LINE}\" (LAMP_NO_WORDS_AGAIN)"
                );
                queue.push(NO_WORDS_LINE.to_owned()).await?;
                return Ok(());
            }
            if calls.is_empty() {
                return Ok(());
            }
            // An add-on's quoted line calls nothing, though none was
            // declared: what it said is said, and the turn ends.
            if quoted {
                eprintln!(
                    "lamp-voice second: request {request} is an add-on's quoted line: its {} call(s) are refused, a quoted add-on turn calls nothing",
                    calls.len()
                );
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
            // Words said before the calls, for the next round's note
            // (`LAMP_SECOND_SAID_ONCE`): never a call written as words, which
            // is not said.
            let said_once = (shared.said_once
                && text.chars().any(char::is_alphanumeric)
                && !crate::not_speech::names_a_function(&text))
            .then(|| said_once_note(&text));
            // A turn that may be answered with its calls' own line
            // (`LAMP_SHORT_DONE`, #2877): these are its first calls, no held
            // forget ran before them, and the model said no words beside
            // them. Any word is beyond the ask (a call written as words
            // too), and the turn goes on as without the switch.
            let may_answer_short = shared.short_done
                && first_calls
                && !held_forget_ran
                && !text.chars().any(char::is_alphanumeric);
            // What each call was told, while every one may be answered short.
            let mut short_told: Vec<Value> = Vec::new();
            // Its own words doubt what was heard: a keep in it is not run
            // (#3504 finding N5).
            let doubted = crate::remember::doubted::doubts_what_was_heard(&text);
            messages.push(Message::assistant_calls_with(
                Some(text),
                &calls,
                reasoning.take(),
            ));
            // A call the Policy held ends the turn after this round.
            let mut policy_held = false;
            // A skill's call answered with `"look": true`, by its name, with
            // its arguments and its note for the picture
            // (`LAMP_SECOND_SKILL_LOOK`).
            let mut skill_looked: Option<(String, Value, Option<String>)> = None;
            let held_by_rule: Vec<bool> = calls
                .iter()
                .map(|call| {
                    shared
                        .hold
                        .as_ref()
                        .is_some_and(|hold| hold(request, &call.name))
                })
                .collect();
            // The Policy's decisions about a spoken turn's calls (#800,
            // #1712), waited for together: one `policy_holds::WAIT` for the
            // round, as the Live route parks a round's calls at once, never
            // one wait after another (the peer read on #1984: three actions
            // with the Policy down took 6 s). The lamp's own turns, and calls
            // the rule holds, are never put to it.
            let policed: Vec<Policed> = match &shared.policy {
                Some(gate) if heard => {
                    let waits = calls.iter().zip(&held_by_rule).map(|(call, held)| {
                        if *held {
                            Box::pin(async { Policed::Run }) as BoxFuture<'static, Policed>
                        } else {
                            gate(request, call.name.clone())
                        }
                    });
                    let policed = futures_util::future::join_all(waits).await;
                    if scope.as_ref().is_some_and(|scope| !scope.is_current()) {
                        return Err(Stop::Gone);
                    }
                    policed
                }
                _ => vec![Policed::Run; calls.len()],
            };
            for ((call, held), policed) in calls.iter().zip(held_by_rule).zip(policed) {
                if held {
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
                if policed == Policed::Held {
                    policy_held = true;
                    messages.push(Message::tool(
                        call.id.clone(),
                        crate::policy_holds::held().to_string(),
                    ));
                    continue;
                }
                // A push on the body ended the search: its look is not
                // called again, so the base does not turn again, unless
                // the person asks in words (#2904).
                if shared
                    .search_pushed(|pushed| {
                        pushed.holds(request, heard, &call.name, &call.args, monotonic_us())
                    })
                    .unwrap_or(false)
                {
                    eprintln!(
                        "lamp-voice second: request {request} chat round {round}: {} not run: {}",
                        call.name,
                        crate::search_pushed::WHY
                    );
                    messages.push(Message::tool(
                        call.id.clone(),
                        serde_json::json!({"error": crate::search_pushed::HELD}).to_string(),
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
                // Or one their turn just before asked for by name.
                let asked_just_before = heard && asked_before.contains(&call.name.as_str());
                // `forget_everything` runs on a spoken turn only for its own
                // request's words asking for everything, their own ask just
                // before, or their yes to one held (run before round 1),
                // and never when these words refuse it or ask for something
                // less, whatever came before (lamp-06c1, 26 Oct; the HOLDs
                // on #3608). Called with no ask at all, it is held below.
                if call.name == crate::remember::FORGET_EVERYTHING
                    && heard
                    && !crate::remember::forget_unguarded()
                    && (everything_now == Some(false)
                        || (everything_now.is_none() && asks_now && !asked_just_before))
                {
                    eprintln!(
                        "lamp-voice second: request {request} chat round {round}: {} not run: their words ask for something else, or not to",
                        call.name
                    );
                    messages.push(Message::tool(
                        call.id.clone(),
                        serde_json::json!({"error": crate::remember::NOT_ASKED_TO_FORGET_EVERYTHING})
                            .to_string(),
                    ));
                    continue;
                }
                // A spoken `forget_everything` from someone known never runs
                // on the turn that asks for it, however plainly (#3504, N3,
                // the person's decision, 27 Oct: "except my name" heard as
                // "accept my name" wiped one person). It is held and asked
                // about once, and only their yes to the hold runs it
                // ([`take_held_forget`], with the eyes' check that their face
                // said it), never an ask just before: a yes there is read
                // with no such check (the reviewer-lead's P2 on #3706).
                // Nobody known: as before, the memory refuses it.
                let everything_waits = call.name == crate::remember::FORGET_EVERYTHING
                    && heard
                    && !crate::remember::forget_unguarded()
                    && forget_asker(scope.as_ref()).is_some();
                if crate::remember::is_forget(&call.name)
                    && ((!forget_may_run && !asked_just_before) || everything_waits)
                {
                    let holding = match forget_asker(scope.as_ref()) {
                        _ if held_forget_ran => None,
                        Some(asker) if heard => {
                            Some(hold_forget(shared, request, call, asker, (shared.clock)()))
                        }
                        _ => Some(Holding::Stale),
                    };
                    let told = match holding {
                        None => serde_json::json!({"error": crate::remember::HELD_FORGET_RAN}),
                        Some(Holding::Held) => crate::remember::held_forget_told(&call.name),
                        Some(Holding::OneWaits) => {
                            serde_json::json!({"error": crate::remember::ONE_FORGET_HELD})
                        }
                        Some(Holding::Stale) => {
                            eprintln!(
                                "lamp-voice second: request {request} chat round {round}: {} not run: nothing they said asks to forget ({})",
                                call.name,
                                crate::remember::FORGET_ASKED_ENVIRONMENT
                            );
                            serde_json::json!({"error": crate::remember::NOT_ASKED_TO_FORGET})
                        }
                    };
                    messages.push(Message::tool(call.id.clone(), told.to_string()));
                    continue;
                }
                if crate::remember::is_forget(&call.name) && asked_just_before && !forget_may_run {
                    eprintln!(
                        "lamp-voice second: request {request} chat round {round}: {} runs: their turn just before asked to forget",
                        call.name
                    );
                }
                // A memory call on a line held only by a gap runs for no one,
                // by either runner: refused here, before the requested-audio
                // runner too, which never reaches `run_tool`'s own refusal
                // (Reviewer-two, 03:38 on #3614).
                if held_by_gap(scope.as_ref()) && MEMORY_CALLS.contains(&call.name.as_str()) {
                    eprintln!(
                        "lamp-voice second: request {request}: a memory call on a line held only by a face's gap runs for no one"
                    );
                    messages.push(Message::tool(
                        call.id.clone(),
                        serde_json::json!({ "error": HELD_MEMORY_CALL }).to_string(),
                    ));
                    continue;
                }
                // A keep its own round's words doubt is not run (#3504
                // finding N5): asked again, kept once they say it.
                if call.name == crate::remember::REMEMBER && doubted {
                    eprintln!(
                        "lamp-voice second: request {request} chat round {round}: {}",
                        crate::remember::doubted::SKIPPED
                    );
                    messages.push(Message::tool(
                        call.id.clone(),
                        serde_json::json!({
                            "remembered": false,
                            "note": crate::remember::doubted::NOT_KEPT
                        })
                        .to_string(),
                    ));
                    continue;
                }
                // A memory write for words another request already tried one
                // for ([`WriteOwner`]): claimed by the first to send one, the
                // rest refused ([`claims_write`]).
                if writes_memory(&call.name)
                    && reaches_memory(shared, &call.name, scope.as_ref())
                    && !claims_write(memory_write_owner.as_ref(), request)
                {
                    eprintln!(
                        "lamp-voice second: request {request} chat round {round}: {} not run: a memory write was already tried for these words",
                        call.name
                    );
                    messages.push(Message::tool(
                        call.id.clone(),
                        serde_json::json!({"error": CARRIED_MEMORY_WRITE}).to_string(),
                    ));
                    continue;
                }
                // The request's own ask for everything, as the guard above
                // read it (its words, or their ask just before), goes with
                // this call on its lease: only then does `forget_everything`
                // take the face too (#3564; Reviewer-two's P2s at 01:12 and
                // 02:20). Never on the tool's name alone.
                if call.name == crate::remember::FORGET_EVERYTHING
                    && heard
                    && !crate::remember::forget_unguarded()
                    && (everything_now == Some(true) || asked_just_before)
                    && let Some(scope) = scope.as_ref()
                {
                    scope.mark_everything_asked(&EverythingShown(()));
                }
                ran_tool.store(true, Ordering::Relaxed);
                let mut answer = match (&shared.requested_audio, spoken) {
                    (Some((worker_boot, runner)), Some(turn)) if turn.captured => {
                        let context = lamp_msgs::RequestedAudioContext {
                            worker_boot: worker_boot.clone(),
                            provider_session: turn.session,
                            request,
                            privacy_generation: turn.privacy_generation,
                            call_id: call.id.clone(),
                            issued_us: called_us,
                        };
                        let mut answer = match scope
                            .as_ref()
                            .filter(|scope| audio_scope_current(scope, &context))
                        {
                            Some(scope) => {
                                runner(
                                    context.clone(),
                                    lamp_gemini::ToolCall {
                                        id: call.id.clone(),
                                        name: call.name.clone(),
                                        args: call.args.clone(),
                                    },
                                    scope.clone(),
                                )
                                .await
                            }
                            None => crate::tools::ToolAnswer::plain(crate::tools::unavailable()),
                        };
                        let now = monotonic_us();
                        answer.requested_audio = answer.requested_audio.filter(|offer| {
                            offer.check().is_ok()
                                && offer.context == context
                                && now >= offer.issued_us
                                && now < offer.start_before_us
                        });
                        answer
                    }
                    _ => crate::tools::ToolAnswer::plain(
                        run_tool(shared, request, heard, call, scope.as_ref()).await,
                    ),
                };
                if scope.as_ref().is_some_and(|scope| !scope.is_current()) {
                    return Err(Stop::Gone);
                }
                // A finished task's news asked about (`check_agent_task`):
                // its key is never the model's, and the coordinator puts it
                // on this turn, so the tasks part is told it was heard once
                // the turn is heard whole, as on the Live route (#1390).
                if crate::agent_tasks::is_tool(&call.name)
                    && let Some(key) = crate::agent_tasks::take_news_key(&mut answer.told)
                {
                    eprintln!(
                        "lamp-voice second: request {request} was answered the news ({key}); it rides on the turn"
                    );
                    if !shared
                        .emit(ProviderOutput::NewsAnswered { request, key })
                        .await
                    {
                        return Err(Stop::Gone);
                    }
                }
                // Asked to sleep, and told the line to say (#3748): the
                // coordinator asks the sleep once this turn has played, as on
                // the Live route. Not now during onboarding: told otherwise.
                if (answer.sleep
                    || (crate::sleep_asked::is_tool(&call.name)
                        && answer.told == crate::sleep_asked::told(false)))
                    && !shared.emit(ProviderOutput::SleepAsked { request }).await
                {
                    return Err(Stop::Gone);
                }
                if skill_looked.is_none()
                    && answer.told["look"] == true
                    && shared
                        .skill_look
                        .as_ref()
                        .is_some_and(|skill| skill(&call.name))
                {
                    skill_looked = Some((
                        call.name.clone(),
                        call.args.clone(),
                        picture_note(&answer.told),
                    ));
                }
                if may_answer_short {
                    short_told.push(answer.told.clone());
                }
                // Any call of this request that ran and was not refused (a
                // remember, a reminder set, a skill's add): its confirmation
                // in a later round, "I've saved that for five", is no false
                // keep (reviewer-three and the reviewer-lead on #3628). A
                // recall reads, and keeps nothing.
                if call.name != crate::remember::RECALL
                    && answer.told["remembered"] != false
                    && answer.told.get("error").is_none()
                {
                    kept_now = true;
                    remember_ran |= call.name == crate::remember::REMEMBER;
                }
                messages.push(Message::tool(call.id.clone(), answer.told.to_string()));
                if let Some(offer) = answer.requested_audio {
                    if !shared
                        .emit(ProviderOutput::RequestedAudio {
                            request,
                            session: offer.context.provider_session,
                            offer,
                        })
                        .await
                    {
                        return Err(Stop::Gone);
                    }
                    dispatched.store(true, Ordering::Relaxed);
                }
            }
            // Held: nothing more of the turn is said, as on the Live route,
            // where none of a held turn's sound is let out. What the model
            // said before its call may have gone out already: counted, for
            // the battery (the first builder on #1712, 20:46).
            if policy_held {
                eprintln!(
                    "lamp-voice second: request {request}: {} words were let out before a held call; the turn ends here, and nothing more is said",
                    words_out.load(Ordering::Relaxed)
                );
                return Err(Stop::Held);
            }
            // A skill asked for a look (#941): as a look by its own call, the
            // coordinator takes the turn's full look once the lamp's words
            // play, and the turn ends here; its picture comes in the look
            // turn after it, with the skill's note for it. The lamp says it
            // will look itself rather than wait a round for the model to.
            // A look dropped (a turn the lamp wrote itself) ends nothing:
            // the next round answers with the skill's answer, as with the
            // switch off. In a search's own turn (its look's or its note's)
            // the skill's next look is taken though the lamp wrote the turn:
            // the person asked for the search, and lamp-skills ends it after
            // its last look. Every other turn the lamp wrote takes none
            // (#1146: pictures never chain with nobody speaking).
            if let Some((name, args, note)) = skill_looked
                && ask_look(shared, request, heard || searching, (&name, args), note).await?
            {
                if !queue.started {
                    queue.push(LOOKING.to_owned()).await?;
                }
                return Ok(());
            }
            // Every call a light or volume change answered short: the calls'
            // own line is kept, and the next round is told the change is
            // answered, to add only what else was asked, or nothing
            // (`LAMP_SHORT_DONE`, #2877). The request may hold more than its
            // calls ("dim the light and tell me a joke": round 1 is the call
            // alone), so the round is asked, never assumed (the
            // Reviewer-lead's P2 on #2930). A call held, refused or run by
            // anyone else told nothing short, so the count falls short.
            if may_answer_short
                && short_told.len() == calls.len()
                && let Some(line) = short_done_line(&short_told)
            {
                eprintln!(
                    "lamp-voice second: request {request} chat round {round}: every call answered short; \"{line}\" kept while the next round says whether more was asked (LAMP_SHORT_DONE)"
                );
                messages.push(Message::system(short_done_note(line)));
                short_line = Some(line.to_owned());
            }
            if let Some(note) = said_once {
                eprintln!(
                    "lamp-voice second: request {request} chat round {round} said words before its calls: the next round is told they were heard"
                );
                messages.push(Message::system(note));
            }
        }
        // ponytail: a model still calling tools after MAX_ROUNDS ends the
        // turn with what it has said; a spoken "I could not finish" if seen.
        Ok(())
    };
    let saying = async {
        let mut after = false;
        // The answer's words, for a holding answer's check.
        let mut whole = String::new();
        // A piece the voice could not start twice (`services::UNSAID`,
        // #4338): said with the next piece, as one, so no sentence of the
        // answer is silently dropped.
        let mut carried: Option<String> = None;
        while let Some((text, packets)) = queued.recv().await {
            // Its words, kept for a piece said with the next (#4338).
            let own = text.clone();
            // The announcement is heard, not written: the coordinator reads
            // an answer's opening words for its look ("Let me look.") and
            // for the body's gesture, and they are the model's (#1075 on
            // lamp-52e6: "My voice sounds… Let me look." took no look).
            // Sentences come trimmed: each after the first is said after a
            // space, so the lamp's words read "just now. Hmm," (#1014).
            let answering = text != ANNOUNCEMENT;
            // The announcement's time counts in what was heard, its words
            // don't: they are never kept as the lamp's.
            shared.said_piece(request, answering.then_some(text.as_str()));
            if answering {
                words_out.fetch_add(text.split_whitespace().count(), Ordering::Relaxed);
                whole.push(' ');
                whole.push_str(&text);
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
            // After a piece not said, this one is said with it, in order
            // (#4338): its own speech, already started, is let go. The two
            // go to the voice as the lamp's words, without the speech tags
            // (`LAMP_SECOND_SPEECH_TAGS`) the pieces were given apart.
            let (piece, mut packets) = match carried.take().filter(|_| answering) {
                Some(unsaid) => {
                    let joined = format!("{unsaid} {own}");
                    eprintln!(
                        "lamp-voice second: request {request}: the piece not said goes with the next one, as one piece ({} words)",
                        joined.split_whitespace().count()
                    );
                    let speech = shared
                        .services
                        .speak_for(joined.clone(), net::Turn { request, heard_at });
                    (joined, speech)
                }
                None => (
                    own,
                    futures_util::stream::unfold(packets, |mut packets| async move {
                        packets.recv().await.map(|packet| (packet, packets))
                    })
                    .boxed(),
                ),
            };
            let mut played = false;
            while let Some(packet) = packets.next().await {
                let samples = match packet {
                    // Not started twice, and none of it played: kept for the
                    // next piece (#4338). The announcement is never carried.
                    Err(error) if services::unsaid(&error) && !played => {
                        if answering {
                            eprintln!(
                                "lamp-voice second: request {request}: a piece of the answer could not be started twice; it is said with the next piece"
                            );
                            carried = Some(piece);
                        }
                        break;
                    }
                    packet => packet.map_err(|error| Stop::Failed(fault(&error)))?,
                };
                played = true;
                for part in samples.chunks(PACKET_SAMPLES) {
                    let audio = ProviderOutput::Audio {
                        request,
                        sequence: shared.sequence.fetch_add(1, Ordering::Relaxed) + 1,
                        provider_event_at_us: monotonic_us(),
                        source_frames: part.len(),
                        source_offset: 0,
                        samples: part.to_vec(),
                    };
                    // Its answer's first sound is handed on now: its words
                    // are no longer carried, before the await a newer line's
                    // cut could take them in, so words carried, joined and
                    // unsaid never had a sample at the speaker (El's and
                    // Fourteen's P3 on #3129).
                    if answering
                        && !answered.load(Ordering::Relaxed)
                        && let Ok(mut in_flight) = shared.in_flight.lock()
                    {
                        in_flight.take_if(|carried| carried.request == request);
                    }
                    if !shared.emit(audio).await {
                        return Err(Stop::Gone);
                    }
                    shared.said_samples(request, part.len());
                    spoke.store(true, Ordering::Relaxed);
                    // The answer's, never the announcement's or a cue's.
                    if answering && !answered.swap(true, Ordering::Relaxed) {
                        if let Ok(mut last) = shared.answered_request.lock() {
                            *last = Some(request);
                        }
                        // The air's note, if this request was given it, is
                        // spent now its answer is heard.
                        if let Some((_, heard)) = &shared.air {
                            heard(request);
                        }
                        if let Some((_, heard)) = &shared.seems {
                            heard(request);
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
                        if asked == Asker::Spoken {
                            worker::name_sounded(request);
                        }
                    }
                    if let Ok(mut at) = shared.answered_at.lock() {
                        *at = Some((shared.clock)());
                    }
                }
                // Each packet's words, as long as it plays, after the one
                // before it or from now.
                // Also kept for the echo's tail (`LAMP_SECOND_OWN_ECHO`).
                if answering && (shared.did.is_some() || shared.own_echo) {
                    let until = shared.said_through(samples.len(), monotonic_us());
                    if let Some(did) = &shared.did {
                        did(Did::Said(request, until));
                    }
                }
            }
        }
        // The answer's last piece could not be started twice: nothing came
        // after it to say it with (#4338).
        if carried.is_some() {
            eprintln!(
                "lamp-voice second: request {request}: the answer's last piece could not be started twice and was not said"
            );
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
        Ok(whole)
    };
    let ended = tokio::try_join!(writing, saying);
    if shared.note_tools
        && let Ok(noted) = turn_tools.lock()
    {
        shared.turn_tools_said(noted.line(request));
    }
    // A forget held in this turn stands only on its answer said whole.
    settle_held_forget(
        shared,
        request,
        ended.as_ref().ok().map(|(_, whole)| whole.as_str()),
    );
    // Only asking the person to wait ("One moment."), with nothing called:
    // the coordinator asks this route to go on, as it asks Gemini
    // (`LAMP_HOLDING_ANSWER`, `crate::holding_answer`; the first builder's
    // 22:54 on #1722). Only for someone who spoke.
    let holding = shared.holding_answer
        && asked == Asker::Spoken
        && !called.load(Ordering::Relaxed)
        && matches!(&ended, Ok((_, whole)) if crate::holding_answer::holding_only(whole));
    // A look's turn answered with nothing called: neither the next place
    // nor anything else (`LAMP_SECOND_LOOK_NOTE`). The coordinator knows
    // whether the look was a skill's search. With the walk
    // (`LAMP_SECOND_LOOK_WALK`), a search's note turn left so is told too.
    let look_left = shared.note_looks
        && (looking || (shared.walk_looks && searching))
        && ended.is_ok()
        && !called.load(Ordering::Relaxed);
    // The voice's line on the turn's pieces (#1712).
    shared.services.spoken(request);
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
        Err(Stop::GaveUp | Stop::Held) => ProviderOutput::TurnComplete {
            request,
            idle: true,
        },
        Err(Stop::Declined) => {
            // A line the model declines is told held back, as a line the
            // rule holds is, unless a tool ran for it first (#1155, the
            // first builder on #1817).
            if let Some(told) = shared.verdicts.as_ref()
                && !ran.load(Ordering::Relaxed)
            {
                told(request, lamp_msgs::TurnVerdict::HeldBack);
            }
            ProviderOutput::TurnComplete {
                request,
                idle: true,
            }
        }
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
        && !acted.load(Ordering::Relaxed)
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
    let session = shared
        .begun
        .lock()
        .ok()
        .and_then(|begun| *begun)
        .filter(|(of, _)| *of == request)
        .map(|(_, session)| session);
    // Before its idle `TurnComplete`, as the Live worker sends it, for the
    // session the turn was begun in.
    if holding
        && matches!(last, ProviderOutput::TurnComplete { idle: true, .. })
        && let Some(session) = session
    {
        eprintln!(
            "lamp-voice second: request {request} was answered only with a holding phrase: the coordinator may ask it to go on"
        );
        if !shared
            .emit(ProviderOutput::HoldingAnswer { request, session })
            .await
        {
            return;
        }
    }
    if look_left
        && matches!(last, ProviderOutput::TurnComplete { idle: true, .. })
        && let Some(session) = session
    {
        eprintln!(
            "lamp-voice second: request {request} answered a look with nothing called: the coordinator may note a search to go on"
        );
        if !shared
            .emit(ProviderOutput::LookNoCall { request, session })
            .await
        {
            return;
        }
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
fn start_speech(
    shared: &Shared,
    speech: &mut JoinSet<()>,
    text: String,
    turn: net::Turn,
) -> Packets {
    let (packets, queued) = mpsc::channel(SENTENCE_PACKETS);
    let mut stream = shared.services.speak_for(text, turn);
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
    /// The reply's tags (`LAMP_SECOND_SPEECH_TAGS`); `None`, pieces as
    /// written.
    tags: Option<speech_tags::Tags>,
}
impl Drop for Queue<'_> {
    /// The reply's tags in the journal, counts only, once it said something.
    fn drop(&mut self) {
        if let Some(tags) = &mut self.tags
            && self.started
        {
            tags.finish();
            eprintln!(
                "lamp-voice second: request {} speech tags: {} given to the voice, {} dropped (LAMP_SECOND_SPEECH_TAGS)",
                self.request, tags.kept, tags.dropped
            );
        }
    }
}
impl Queue<'_> {
    async fn announce(&mut self) -> Result<(), Stop> {
        match self.announcement.take() {
            Some(packets) => self.send(ANNOUNCEMENT.to_owned(), packets).await,
            None => Ok(()),
        }
    }

    async fn push(&mut self, text: String) -> Result<(), Stop> {
        // With tags (`LAMP_SECOND_SPEECH_TAGS`), the voice is given `spoken`
        // and everything else the lamp's words, which carry none.
        let (spoken, text) = match &mut self.tags {
            Some(tags) => tags.piece(&text),
            None => (text.clone(), text),
        };
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
                return Err(Stop::Declined);
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
        if let Ok(mut said) = self.shared.lamp_said.lock() {
            if said.0 != self.request {
                *said = (self.request, String::new());
            }
            said.1.push(' ');
            said.1.push_str(&text);
        }
        let turn = net::Turn {
            request: self.request,
            heard_at: self.heard_at,
        };
        let packets = start_speech(self.shared, self.speech, spoken, turn);
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

#[cfg(test)]
mod requested_audio_tests {
    use super::*;
    use futures_util::stream::{self, BoxStream};
    use serde_json::json;

    pub(super) struct RecordingScript {
        pub(super) chats: Mutex<VecDeque<Vec<net::Result<ChatEvent>>>>,
        pub(super) messages: Mutex<Vec<Vec<Message>>>,
    }

    struct RecordedWords;
    impl Listener for RecordedWords {
        fn send(&self, _samples: &[i16]) -> net::Result<()> {
            Ok(())
        }
        fn finish(self: Box<Self>, _request: u64) -> BoxFuture<'static, net::Result<String>> {
            Box::pin(async { Ok("Please play relaxing music.".into()) })
        }
    }

    impl Services for RecordingScript {
        fn listen(&self) -> BoxFuture<'static, net::Result<Box<dyn Listener>>> {
            Box::pin(async { Ok(Box::new(RecordedWords) as Box<dyn Listener>) })
        }
        fn chat(&self, messages: Vec<Message>) -> BoxStream<'static, net::Result<ChatEvent>> {
            self.messages.lock().unwrap().push(messages);
            stream::iter(self.chats.lock().unwrap().pop_front().unwrap()).boxed()
        }
        fn speak(&self, _text: String) -> BoxStream<'static, net::Result<Vec<i16>>> {
            stream::iter([Ok(vec![1; PACKET_SAMPLES])]).boxed()
        }
    }

    pub(super) fn offered(
        context: lamp_msgs::RequestedAudioContext,
    ) -> lamp_msgs::RequestedAudioOffer {
        lamp_msgs::RequestedAudioOffer {
            skills_boot: "skills-test".into(),
            definition_generation: 1,
            offer_id: 1,
            skill: "relaxing-music".into(),
            asset: "/local/relaxing.wav".into(),
            sha256: "a".repeat(64),
            frames: 24_000,
            issued_us: context.issued_us,
            start_before_us: context.issued_us + lamp_msgs::REQUESTED_AUDIO_START_US,
            finish_before_us: context.issued_us + lamp_msgs::REQUESTED_AUDIO_START_US + 1_000_000,
            context,
        }
    }

    #[tokio::test]
    async fn requested_audio_switch_keeps_the_admitted_identity() {
        for (requested_audio, written, scoped_context) in [false, true]
            .into_iter()
            .flat_map(|audio| [false, true].map(move |written| (audio, written)))
            .flat_map(|(audio, written)| [false, true].map(move |scoped| (audio, written, scoped)))
        {
            let directory = crate::process::SessionDirectory::create().unwrap();
            let face = directory.path.join("face");
            let cache = Arc::new(crate::memory_scope::FaceCache::default());
            let observe = |name: &str| {
                std::fs::write(&face, name).unwrap();
                assert!(cache.replace(
                    cache.begin_refresh(),
                    crate::known_faces::FaceObservation::read(&face)
                ));
            };
            let expected_person = (!written).then(|| crate::remember::person_id("admitted-face"));
            let script = Arc::new(RecordingScript {
                chats: Mutex::new(
                    [
                        vec![
                            Ok(ChatEvent::ToolCalls(vec![net::ToolCall {
                                extra: None,
                                id: "original-scoped-call".into(),
                                name: "play_relaxing_music".into(),
                                args: json!({}),
                            }])),
                            Ok(ChatEvent::Done),
                        ],
                        vec![Ok(ChatEvent::Done)],
                    ]
                    .into(),
                ),
                messages: Mutex::default(),
            });
            let ordinary_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let scoped_runner: ScopedToolRunner = {
                let calls = ordinary_calls.clone();
                let expected_person = expected_person.clone();
                Arc::new(move |request, heard, name, _, scope| {
                    assert_eq!(request, 7);
                    assert_eq!(heard, !written);
                    assert_eq!(name, "play_relaxing_music");
                    assert_eq!(scope.scope().unwrap().person(), expected_person.as_deref());
                    calls.fetch_add(1, Ordering::Relaxed);
                    Box::pin(async { json!({"next":"Confirm."}) })
                })
            };
            let contexts: Arc<Mutex<Vec<lamp_msgs::RequestedAudioContext>>> = Arc::default();
            let audio_runner: RequestedAudioRunner = {
                let contexts = contexts.clone();
                let expected_person = expected_person.clone();
                Arc::new(move |context, call, scope| {
                    assert_eq!(call.id, "original-scoped-call");
                    assert_eq!(call.name, "play_relaxing_music");
                    assert_eq!(call.args, json!({}));
                    assert!(audio_scope_current(&scope, &context));
                    assert_eq!(scope.scope().unwrap().person(), expected_person.as_deref());
                    contexts.lock().unwrap().push(context.clone());
                    Box::pin(async move {
                        crate::tools::ToolAnswer {
                            sleep: false,
                            told: json!({"next":"Confirm."}),
                            requested_audio: Some(offered(context)),
                        }
                    })
                })
            };
            let boot = lamp_turn_owner::BootId::new([9; 16]).unwrap();
            let mut engine = Engine::new(
                script.clone(),
                || Told::from("legacy context".to_owned()),
                Arc::new(|_, _, _, _| panic!("scope remains mandatory with either option off")),
            )
            .with_memory_scope(
                boot,
                cache.clone(),
                scoped_runner,
                scoped_context.then(|| {
                    Arc::new(|lease: &crate::memory_scope::ScopeLease| {
                        let scope = lease.scope().unwrap();
                        Told {
                            text: format!("context {:?}", scope.person()),
                            history: vec![Message::assistant(format!(
                                "history {:?}",
                                scope.person()
                            ))],
                            after: None,
                        }
                    }) as ScopedSystem
                }),
            )
            .announcing(false);
            if requested_audio {
                engine = engine.requesting_audio(boot, audio_runner);
            }
            engine.memory_camera(true, 1);
            observe("admitted-face");
            if written {
                engine.submit(ProviderInput::Written {
                    request: 7,
                    session: 3,
                    privacy_generation: 5,
                    text: "Please play relaxing music.".into(),
                    look: false,
                    search: false,
                    quoted: false,
                });
            } else {
                engine.submit(ProviderInput::Start {
                    request: 7,
                    session: 3,
                    privacy_generation: 5,
                    continues: false,
                    continues_cut_off: false,
                    over_the_lamp: false,
                    after_stop: false,
                    follows_on: false,
                    over_the_lamps_voice: false,
                    heard_of_cut: None,
                    soft_lead_us: 0,
                });
                engine.submit(ProviderInput::Audio {
                    request: 7,
                    privacy_generation: 5,
                    sequence: 1,
                    read_completed_at_us: monotonic_us(),
                    samples: vec![1; 160],
                });
                // Context and the later tool must retain the identity admitted
                // at Start, even though another face is present before End.
                observe("later-face");
                engine.submit(ProviderInput::End {
                    request: 7,
                    privacy_generation: 5,
                    sound: false,
                    cut_in: false,
                });
            }
            let mut offers = Vec::new();
            loop {
                match tokio::time::timeout(Duration::from_secs(2), engine.next())
                    .await
                    .unwrap()
                {
                    ProviderOutput::RequestedAudio {
                        request,
                        session,
                        offer,
                    } => {
                        assert_eq!((request, session), (7, 3));
                        offers.push(offer);
                    }
                    ProviderOutput::TurnComplete { .. } => break,
                    ProviderOutput::TurnFailed { .. } => panic!("the scripted turn failed"),
                    _ => (),
                }
            }
            let has_audio = requested_audio && !written;
            assert_eq!(offers.len(), usize::from(has_audio));
            assert_eq!(
                ordinary_calls.load(Ordering::Relaxed),
                usize::from(!has_audio)
            );
            let contexts = contexts.lock().unwrap();
            assert_eq!(contexts.len(), usize::from(has_audio));
            if let Some(context) = contexts.first() {
                assert_eq!(context.worker_boot, crate::process::boot_text(boot));
                assert_eq!(
                    (
                        context.request,
                        context.provider_session,
                        context.privacy_generation
                    ),
                    (7, 3, 5)
                );
                assert_eq!(context.call_id, "original-scoped-call");
                assert!(context.issued_us > 0);
                assert_eq!(offers[0].context, *context);
            }
            let messages = script.messages.lock().unwrap();
            assert_eq!(messages.len(), 2);
            if scoped_context {
                let system = Message::system(format!("context {:?}", expected_person.as_deref()));
                let history =
                    Message::assistant(format!("history {:?}", expected_person.as_deref()));
                assert!(
                    messages
                        .iter()
                        .all(|round| round[0] == system && round[1] == history)
                );
                assert_eq!(messages[0].len(), 3);
            } else {
                assert_eq!(messages[0][0], Message::system("legacy context"));
                assert_eq!(messages[0].len(), 2);
            }
            let sent = format!("{messages:?}");
            assert!(!sent.contains(&crate::remember::person_id("later-face")));
            assert!(!sent.contains("requested_audio"));
            assert!(!sent.contains("/local/relaxing.wav"));
            let lease = engine
                .turn
                .as_ref()
                .unwrap()
                .memory_scope
                .as_ref()
                .unwrap()
                .lease();
            assert!(
                lease.is_current(),
                "generation completion retains the admitted owner"
            );
            engine.retire_through(7);
            assert!(!lease.is_current());
        }
    }

    #[tokio::test]
    async fn requested_audio_uses_only_the_spoken_runner_and_preserves_the_call_binding() {
        for mode in [
            "spoken",
            "written",
            "empty-input",
            "off",
            "changed-binding",
            "missing-scope",
            "wrong-worker",
        ] {
            let script = Arc::new(RecordingScript {
                chats: Mutex::new(
                    [
                        vec![
                            Ok(ChatEvent::ToolCalls(vec![net::ToolCall {
                                extra: None,
                                id: "call-original".into(),
                                name: "play_relaxing_music".into(),
                                args: json!({}),
                            }])),
                            Ok(ChatEvent::Done),
                        ],
                        vec![
                            Ok(ChatEvent::Text("A quiet piece is ready.".into())),
                            Ok(ChatEvent::Done),
                        ],
                    ]
                    .into(),
                ),
                messages: Mutex::default(),
            });
            let runner: ToolRunner = Arc::new(|_, _, _, _| {
                panic!("a configured request never falls back to unscoped tools")
            });
            let scoped: Arc<Mutex<Vec<(u64, bool)>>> = Arc::default();
            let seen_scoped = scoped.clone();
            let scoped_runner: ScopedToolRunner = Arc::new(move |request, heard, _, _, scope| {
                assert_eq!(scope.key().request, request);
                assert!(scope.is_current());
                seen_scoped.lock().unwrap().push((request, heard));
                Box::pin(async { json!({"next":"Confirm."}) })
            });
            let seen: Arc<Mutex<Vec<lamp_msgs::RequestedAudioContext>>> = Arc::default();
            let captured = seen.clone();
            let typed: RequestedAudioRunner = Arc::new(move |context, call, scope| {
                assert_eq!(context.call_id, call.id);
                assert_eq!(scope.key().request, context.request);
                assert_eq!(scope.key().session, context.provider_session);
                assert_eq!(
                    crate::process::boot_text(scope.key().worker),
                    context.worker_boot
                );
                assert!(scope.is_current());
                captured.lock().unwrap().push(context.clone());
                Box::pin(async move {
                    let mut offer = offered(context);
                    if mode == "changed-binding" {
                        offer.context.privacy_generation += 1;
                    }
                    crate::tools::ToolAnswer {
                        sleep: false,
                        told: json!({"next":"Confirm."}),
                        requested_audio: Some(offer),
                    }
                })
            });
            let boot = lamp_turn_owner::BootId::new([9; 16]).unwrap();
            let memory_boot = if mode == "wrong-worker" {
                lamp_turn_owner::BootId::new([8; 16]).unwrap()
            } else {
                boot
            };
            let mut engine = Engine::new(script.clone(), Told::default, runner)
                .with_memory_scope(memory_boot, Arc::default(), scoped_runner, None)
                .announcing(false);
            if mode == "missing-scope" {
                engine.memory = None;
            }
            if mode != "off" {
                engine = engine.requesting_audio(boot, typed);
            }
            if mode == "written" {
                engine.submit(ProviderInput::Written {
                    request: 7,
                    session: 3,
                    privacy_generation: 5,
                    text: "Please play relaxing music.".into(),
                    look: false,
                    search: false,
                    quoted: false,
                });
            } else {
                engine.submit(ProviderInput::Start {
                    request: 7,
                    session: 3,
                    privacy_generation: 5,
                    continues: false,
                    continues_cut_off: false,
                    over_the_lamp: false,
                    after_stop: false,
                    follows_on: false,
                    over_the_lamps_voice: false,
                    heard_of_cut: None,
                    soft_lead_us: 0,
                });
                if mode != "empty-input" {
                    engine.submit(ProviderInput::Audio {
                        request: 7,
                        privacy_generation: 5,
                        sequence: 1,
                        read_completed_at_us: monotonic_us(),
                        samples: vec![1; 160],
                    });
                }
                engine.submit(ProviderInput::End {
                    request: 7,
                    privacy_generation: 5,
                    sound: false,
                    cut_in: false,
                });
            }
            let mut offers = Vec::new();
            loop {
                let output = tokio::time::timeout(Duration::from_secs(2), engine.next())
                    .await
                    .unwrap();
                match output {
                    ProviderOutput::RequestedAudio {
                        request: 7,
                        session: 3,
                        offer,
                    } => offers.push(offer),
                    ProviderOutput::TurnComplete { .. } => break,
                    ProviderOutput::TurnFailed { .. } => panic!("the scripted turn failed"),
                    _ => (),
                }
            }
            assert_eq!(offers.len(), usize::from(mode == "spoken"), "{mode}");
            if matches!(mode, "spoken" | "changed-binding") {
                assert!(scoped.lock().unwrap().is_empty());
                let contexts = seen.lock().unwrap();
                assert_eq!(contexts.len(), 1);
                let context = &contexts[0];
                assert_eq!(context.worker_boot, crate::process::boot_text(boot));
                assert_eq!(
                    (
                        context.request,
                        context.provider_session,
                        context.privacy_generation
                    ),
                    (7, 3, 5)
                );
                assert!(context.issued_us > 0);
            } else {
                assert!(seen.lock().unwrap().is_empty(), "{mode}");
                if matches!(mode, "missing-scope" | "wrong-worker") {
                    assert!(scoped.lock().unwrap().is_empty());
                } else {
                    assert_eq!(*scoped.lock().unwrap(), [(7, mode != "written")]);
                }
            }
            let messages = script.messages.lock().unwrap();
            let told = serde_json::to_string(&messages[1]).unwrap();
            assert!(!told.contains("requested_audio") && !told.contains("relaxing.wav"));
            if matches!(mode, "missing-scope" | "wrong-worker") {
                let tool = Message::tool("call-original", crate::tools::unavailable().to_string());
                assert_eq!(
                    serde_json::to_value(messages[1].last().unwrap()).unwrap(),
                    serde_json::to_value(tool).unwrap(),
                    "{mode}",
                );
            }
        }
    }

    #[tokio::test]
    async fn requested_audio_scope_is_checked_after_the_runner_and_before_queued_delivery() {
        for revoked in [false, true] {
            let script = Arc::new(RecordingScript {
                chats: Mutex::new(
                    [
                        vec![
                            Ok(ChatEvent::ToolCalls(vec![net::ToolCall {
                                extra: None,
                                id: "call-original".into(),
                                name: "play_relaxing_music".into(),
                                args: json!({}),
                            }])),
                            Ok(ChatEvent::Done),
                        ],
                        vec![Ok(ChatEvent::Done)],
                    ]
                    .into(),
                ),
                messages: Mutex::default(),
            });
            let entered = Arc::new(tokio::sync::Notify::new());
            let release = Arc::new(tokio::sync::Notify::new());
            let typed: RequestedAudioRunner = {
                let entered = entered.clone();
                let release = release.clone();
                Arc::new(move |context, call, scope| {
                    let entered = entered.clone();
                    let release = release.clone();
                    assert_eq!(call.id, "call-original");
                    assert!(audio_scope_current(&scope, &context));
                    Box::pin(async move {
                        entered.notify_one();
                        release.notified().await;
                        // Deliberately return even after revocation: the provider's
                        // final fence, not a cooperative runner, must suppress it.
                        crate::tools::ToolAnswer {
                            sleep: false,
                            told: json!({"next":"Confirm."}),
                            requested_audio: Some(offered(context)),
                        }
                    })
                })
            };
            let legacy: ToolRunner = Arc::new(|_, _, _, _| panic!("unscoped fallback"));
            let scoped: ScopedToolRunner = Arc::new(|_, _, _, _, _| panic!("lost audio context"));
            let boot = lamp_turn_owner::BootId::new([9; 16]).unwrap();
            let mut engine = Engine::new(script.clone(), Told::default, legacy)
                .with_memory_scope(boot, Arc::default(), scoped, None)
                .requesting_audio(boot, typed)
                .announcing(false);
            engine.submit(ProviderInput::Start {
                request: 7,
                session: 3,
                privacy_generation: 5,
                continues: false,
                continues_cut_off: false,
                over_the_lamp: false,
                after_stop: false,
                follows_on: false,
                over_the_lamps_voice: false,
                heard_of_cut: None,
                soft_lead_us: 0,
            });
            engine.submit(ProviderInput::Audio {
                request: 7,
                privacy_generation: 5,
                sequence: 1,
                read_completed_at_us: monotonic_us(),
                samples: vec![1; 160],
            });
            engine.submit(ProviderInput::End {
                request: 7,
                privacy_generation: 5,
                sound: false,
                cut_in: false,
            });
            tokio::time::timeout(Duration::from_secs(2), entered.notified())
                .await
                .unwrap();
            if revoked {
                engine
                    .turn
                    .as_ref()
                    .unwrap()
                    .memory_scope
                    .as_ref()
                    .unwrap()
                    .revoke();
            }
            release.notify_one();
            if revoked {
                tokio::time::timeout(
                    Duration::from_secs(2),
                    &mut engine.turn.as_mut().unwrap().task,
                )
                .await
                .unwrap()
                .unwrap();
                assert_eq!(script.messages.lock().unwrap().len(), 1);
                while let Some(event) = engine.try_output() {
                    assert!(!matches!(event, ProviderOutput::RequestedAudio { .. }));
                }
            } else {
                let mut offer = None;
                loop {
                    match tokio::time::timeout(Duration::from_secs(2), engine.next())
                        .await
                        .unwrap()
                    {
                        ProviderOutput::RequestedAudio { offer: got, .. } => {
                            assert!(offer.replace(got).is_none(), "one private offer only");
                        }
                        ProviderOutput::TurnComplete { .. } => break,
                        ProviderOutput::TurnFailed { .. } => panic!("the scripted turn failed"),
                        _ => (),
                    }
                }
                let offer = offer.expect("the valid scoped reply is handed on");
                assert_eq!(offer.context.call_id, "call-original");
                assert_eq!(script.messages.lock().unwrap().len(), 2);
                let event = || ProviderOutput::RequestedAudio {
                    request: 7,
                    session: 3,
                    offer: offer.clone(),
                };
                // Generation completion does not drop the request's owner. A
                // still-queued offer can be delivered until that owner is revoked.
                engine.shared.out.try_send(event()).unwrap();
                assert!(matches!(
                    engine.try_output(),
                    Some(ProviderOutput::RequestedAudio { .. })
                ));
                engine.shared.out.try_send(event()).unwrap();
                engine
                    .turn
                    .as_ref()
                    .unwrap()
                    .memory_scope
                    .as_ref()
                    .unwrap()
                    .revoke();
                assert!(engine.try_output().is_none());
            }
        }
    }

    #[tokio::test]
    async fn requested_audio_queued_for_a_retired_or_replaced_turn_is_dropped() {
        let script = Arc::new(RecordingScript {
            chats: Mutex::default(),
            messages: Mutex::default(),
        });
        let runner: ToolRunner = Arc::new(|_, _, _, _| Box::pin(async { json!({}) }));
        let mut engine = Engine::new(script, Told::default, runner);
        let event = || ProviderOutput::RequestedAudio {
            request: 7,
            session: 3,
            offer: offered(lamp_msgs::RequestedAudioContext {
                worker_boot: "a".repeat(32),
                provider_session: 3,
                request: 7,
                privacy_generation: 5,
                call_id: "call-original".into(),
                issued_us: monotonic_us(),
            }),
        };
        engine.begin(7, 3);
        engine.urgent.clear();
        engine.shared.out.try_send(event()).unwrap();
        assert!(matches!(
            engine.try_output(),
            Some(ProviderOutput::RequestedAudio { .. })
        ));
        engine.shared.out.try_send(event()).unwrap();
        engine.begin(8, 3);
        engine.urgent.clear();
        assert!(engine.try_output().is_none());
        engine.shared.out.try_send(event()).unwrap();
        engine.begin(7, 4);
        engine.urgent.clear();
        assert!(engine.try_output().is_none());
        engine.begin(7, 3);
        engine.urgent.clear();
        engine.shared.out.try_send(event()).unwrap();
        engine.retire_through(7);
        assert!(engine.try_output().is_none());
    }
}
