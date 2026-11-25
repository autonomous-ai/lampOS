//! The lamp's agent switch, said in a conversation (`docs/tasks.md`, "The
//! agent switch, said aloud").
//!
//! The lamp runs one agent, its default (OpenClaw or Hermes). When the
//! default changes, lamp-tasks ends the old agent's work and the new one's
//! gateway takes a few seconds to answer. The owner's rule: the lamp says
//! one fixed line when a switch starts, "Switching to Hermes.", and one
//! when the new agent is ready, "Hermes is ready.", only to someone it is
//! talking with. A switch made in the app while the room is quiet is never
//! spoken: it shows in the app and on the light alone, and the lamp never
//! talks to an empty room.
//!
//! "In a conversation" is the voice's own sign, read by the coordinator:
//! a turn open (a reply in hand or a stream the person owns), or the
//! thread's conversation going on (`thread::Thread::going_on`, the lamp's
//! words within `thread::GOING_ON_FOR`, 45 s, and not closed since). A
//! switch asked by voice is in one by its nature. Out of a conversation
//! nothing is read and nothing is owed; a conversation that opens later
//! starts with what lamp-tasks says then, so a switch made before it is
//! never said.
//!
//! What is read: lamp-tasks' `targets` on its request socket, the voice's
//! existing link to it, every [`READ_EVERY`] on a thread of its own and
//! only while a conversation is open. The lamp's target (`"lamp"`) names
//! the default as its engine (#6492); a change of it within one
//! conversation is a switch. Its readiness is the answer's `gateway`
//! (`"switching"` or `"ready"`, lamp-tasks' `direct::gateway_said`): with
//! no `gateway` in the answer (a lamp-tasks that does not give it on
//! `targets`), the ready line is never said, since nothing says the new
//! agent answers yet.
//!
//! Said once each: one "switching" line a switch, and one "ready" line
//! after it, never a "ready" line for a switch whose "switching" line was
//! not said. A `ready` read right after the engine changed may be the old
//! gateway's, read before lamp-tasks looked again ([`SETTLE_US`]): it
//! counts only once `switching` was seen, or that long after the change.
//! Only OpenClaw and Hermes have lines; a switch to any other engine is
//! not said.
use crate::language::Language;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::Duration,
};

/// How often lamp-tasks' `targets` is read while a conversation is open:
/// the "ready" line begins within about this long of the gateway's answer.
pub const READ_EVERY: Duration = Duration::from_secs(1);
/// Each read, connecting included: a busy lamp-tasks is read next time.
const ASK_DEADLINE: Duration = Duration::from_millis(800);
/// A `ready` read this soon after the engine changed may be from before
/// lamp-tasks looked at the new gateway (its own look comes every
/// `direct::SWITCH_LOOK_EVERY`, 1 s): not taken unless `switching` was
/// seen first.
pub const SETTLE_US: u64 = 2_500_000;
/// A conversation ends after this long with no sign of it
/// ([`AgentSwitchTold::conversation`]).
pub const ENDS_AFTER_US: u64 = 2_000_000;
/// lamp-tasks' alias for the lamp's own target (`runtime::LAMP_TARGET`).
const LAMP_TARGET: &str = "lamp";

/// An agent the lamp says by name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Agent {
    OpenClaw,
    Hermes,
}

impl Agent {
    /// The agent for lamp-tasks' engine name; none for any other engine.
    pub fn of(engine: &str) -> Option<Self> {
        match engine {
            "openclaw" => Some(Self::OpenClaw),
            "hermes" => Some(Self::Hermes),
            _ => None,
        }
    }
}

/// Which of the two lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Moment {
    /// "Switching to <Agent>."
    Switching,
    /// "<Agent> is ready."
    Ready,
}

/// The lines in English, then Vietnamese, Japanese, Chinese, Korean and
/// French (`Language::column`'s order). Said through the model's route as
/// written (`onboarding::said_as_is`), never from the speech cache.
const LINES: [(Agent, Moment, &str, [&str; 5]); 4] = [
    (
        Agent::OpenClaw,
        Moment::Switching,
        "Switching to OpenClaw.",
        [
            "Mình chuyển sang OpenClaw nhé.",
            "OpenClawに切り替えるね。",
            "正在切换到 OpenClaw。",
            "OpenClaw로 바꿀게요.",
            "Je passe à OpenClaw.",
        ],
    ),
    (
        Agent::OpenClaw,
        Moment::Ready,
        "OpenClaw is ready.",
        [
            "OpenClaw sẵn sàng rồi nhé.",
            "OpenClawの準備ができたよ。",
            "OpenClaw 准备好了。",
            "OpenClaw 준비됐어요.",
            "OpenClaw est prêt.",
        ],
    ),
    (
        Agent::Hermes,
        Moment::Switching,
        "Switching to Hermes.",
        [
            "Mình chuyển sang Hermes nhé.",
            "Hermesに切り替えるね。",
            "正在切换到 Hermes。",
            "Hermes로 바꿀게요.",
            "Je passe à Hermes.",
        ],
    ),
    (
        Agent::Hermes,
        Moment::Ready,
        "Hermes is ready.",
        [
            "Hermes sẵn sàng rồi nhé.",
            "Hermesの準備ができたよ。",
            "Hermes 准备好了。",
            "Hermes 준비됐어요.",
            "Hermes est prêt.",
        ],
    ),
];

/// The line for `agent` at `moment` in `language`, English for none.
pub fn line(language: Option<Language>, agent: Agent, moment: Moment) -> &'static str {
    let (_, _, english, words) = LINES
        .iter()
        .find(|(of, at, _, _)| *of == agent && *at == moment)
        .expect("every agent has both lines");
    language.map_or(english, |language| words[column(language)])
}

/// The task news for work the switch ended (lamp-tasks' `direct::SWITCHED`
/// as its last words), in English, then [`LINES`]' languages.
const ENDED_LINE: (&str, [&str; 5]) = (
    crate::skill_words::SWITCHED_LINE,
    [
        "Việc đó đã dừng vì mình chuyển sang agent khác. Bạn hỏi lại mình nhé.",
        "エージェントを切り替えたので、それは止まったよ。もう一度聞いてね。",
        "因为我切换到了另一个智能体，那件事停下了。请再问我一次。",
        "다른 에이전트로 바꿔서 그 일은 멈췄어요. 다시 물어봐 주세요.",
        "Ça s'est arrêté parce que je suis passé à un autre agent. Redemande-moi, s'il te plaît.",
    ],
);

/// The task news for work the switch ended, in `language`, English for
/// none, as a written turn the route says as written
/// (`onboarding::said_as_is`). The lamp's own fixed line, never worded by
/// the chat model: given the line inside a prompt, the model answered a
/// sound heard just before with its own small talk and the person never
/// heard it (lamp, Hermes to OpenClaw).
pub fn ended_note(language: Option<Language>) -> Option<String> {
    let (english, words) = ENDED_LINE;
    crate::onboarding::said_as_is(language.map_or(english, |language| words[column(language)]))
}

/// The language's place in each row of [`LINES`].
fn column(language: Language) -> usize {
    match language {
        Language::Vietnamese => 0,
        Language::Japanese => 1,
        Language::Chinese => 2,
        Language::Korean => 3,
        Language::French => 4,
    }
}

/// The gateway's state as lamp-tasks says it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gateway {
    Switching,
    Ready,
}

/// One read of lamp-tasks' `targets`: the lamp's default engine, and its
/// gateway's state where the answer gives it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Read {
    pub engine: Option<String>,
    pub gateway: Option<Gateway>,
}

impl Read {
    /// From a `targets` answer; none for a refusal.
    pub fn of(answer: &Value) -> Option<Self> {
        if answer.get("ok") != Some(&json!(true)) {
            return None;
        }
        let engine = answer["targets"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|target| target["alias"] == LAMP_TARGET)
            .and_then(|target| target["engine"].as_str())
            .map(str::to_owned);
        let gateway = match answer["gateway"].as_str() {
            Some("switching") => Some(Gateway::Switching),
            Some("ready") => Some(Gateway::Ready),
            _ => None,
        };
        Some(Self { engine, gateway })
    }
}

/// A switch this conversation saw begin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Switch {
    to: Agent,
    /// When the change was read (the coordinator's clock, µs).
    began_us: u64,
    /// `switching` read since.
    saw_switching: bool,
    /// `ready` read, and taken ([`SETTLE_US`]).
    ready: bool,
    said_switching: bool,
    said_ready: bool,
}

/// What this conversation knows of the switch: no lamp or thread of its
/// own, so a test drives it with reads and a clock.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SwitchTold {
    /// The conversation reads belong to; 0 with none open.
    conversation: u64,
    /// The default engine last read in this conversation.
    seen: Option<String>,
    switch: Option<Switch>,
}

impl SwitchTold {
    /// A conversation opened (`Some`, its number) or ended (`None`): ended,
    /// all is forgotten, and nothing is owed.
    pub fn conversation(&mut self, open: Option<u64>) {
        let open = open.unwrap_or(0);
        if open != self.conversation {
            *self = Self {
                conversation: open,
                ..Self::default()
            };
        }
    }

    /// A read taken in `conversation` at `clock_us`: a read of another
    /// conversation, or with none open, is passed over.
    pub fn take(&mut self, conversation: u64, read: &Read, clock_us: u64) {
        if conversation == 0 || conversation != self.conversation {
            return;
        }
        if let Some(engine) = read.engine.as_deref() {
            if self.seen.as_deref().is_some_and(|seen| seen != engine) {
                // A new switch; one not yet said gives way to it.
                self.switch = Agent::of(engine).map(|to| Switch {
                    to,
                    began_us: clock_us,
                    saw_switching: false,
                    ready: false,
                    said_switching: false,
                    said_ready: false,
                });
            }
            self.seen = Some(engine.to_owned());
        }
        if let Some(switch) = self.switch.as_mut() {
            match read.gateway {
                Some(Gateway::Switching) => switch.saw_switching = true,
                Some(Gateway::Ready)
                    if switch.saw_switching
                        || clock_us.saturating_sub(switch.began_us) >= SETTLE_US =>
                {
                    switch.ready = true;
                }
                _ => {}
            }
        }
    }

    /// The line owed now, if any: "switching" first, then "ready" once the
    /// gateway answered and "switching" was said.
    pub fn owed(&self) -> Option<(Agent, Moment)> {
        let switch = self.switch?;
        if !switch.said_switching {
            return Some((switch.to, Moment::Switching));
        }
        (switch.ready && !switch.said_ready).then_some((switch.to, Moment::Ready))
    }

    /// The line for `moment` was said.
    pub fn said(&mut self, moment: Moment) {
        if let Some(switch) = self.switch.as_mut() {
            match moment {
                Moment::Switching => switch.said_switching = true,
                Moment::Ready => switch.said_ready = true,
            }
        }
    }
}

/// What the coordinator and the reading thread share.
#[derive(Default)]
struct Shared {
    /// The conversation open now, 0 with none: the thread reads only then.
    conversation: AtomicU64,
    told: Mutex<SwitchTold>,
    stopping: AtomicBool,
}

/// The coordinator's handle: it says whether a conversation is open, and
/// takes the line owed; a thread of its own reads lamp-tasks, so a turn
/// never waits on it.
pub struct AgentSwitchTold {
    shared: Arc<Shared>,
    /// Conversations opened so far, to number the next.
    opened: u64,
    /// Since when no sign of the open conversation was seen.
    closed_since: Option<u64>,
}

impl AgentSwitchTold {
    /// On whenever the tasks part is (`LAMP_TASKS_DIR`).
    pub fn from_environment() -> Option<Self> {
        let directory = std::env::var_os(crate::agent_tasks::ENVIRONMENT)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())?;
        Some(Self::start(&directory.join("request.sock")))
    }

    /// Reads `targets` at `socket` while a conversation is open.
    pub fn start(socket: &Path) -> Self {
        let shared = Arc::new(Shared::default());
        let reader = Arc::clone(&shared);
        let socket = socket.to_path_buf();
        let started = std::time::Instant::now();
        let spawned = thread::Builder::new()
            .name("agent-switch".into())
            .spawn(move || {
                while !reader.stopping.load(Ordering::Relaxed) {
                    thread::sleep(READ_EVERY);
                    let conversation = reader.conversation.load(Ordering::Relaxed);
                    if conversation == 0 {
                        continue;
                    }
                    let Some(read) = crate::tools::exchange_up_to(
                        &socket,
                        &json!({"op":"targets"}),
                        ASK_DEADLINE,
                        crate::tools::MAX_TASKS_ANSWER_BYTES,
                    )
                    .as_ref()
                    .and_then(Read::of) else {
                        continue;
                    };
                    let clock_us = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
                    reader
                        .told
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .take(conversation, &read, clock_us);
                }
            });
        if let Err(error) = spawned {
            eprintln!("lamp-voice: the agent switch is not read: {error}");
        }
        Self {
            shared,
            opened: 0,
            closed_since: None,
        }
    }

    /// Whether a conversation is open at `clock_us` ([`in_conversation`]):
    /// one ends only after [`ENDS_AFTER_US`] with no sign of it, so the
    /// moment between a reply's end and its exchange in the thread does
    /// not end it.
    pub fn conversation(&mut self, open: bool, clock_us: u64) {
        let now = self.shared.conversation.load(Ordering::Relaxed);
        if open {
            self.closed_since = None;
        } else if now != 0 {
            let since = *self.closed_since.get_or_insert(clock_us);
            if clock_us.saturating_sub(since) < ENDS_AFTER_US {
                return;
            }
        }
        let next = match (open, now) {
            (false, 0) | (true, 1..) => return,
            (false, _) => 0,
            (true, 0) => {
                self.opened += 1;
                self.opened
            }
        };
        self.closed_since = None;
        let mut told = self
            .shared
            .told
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        told.conversation(Some(next).filter(|open| *open != 0));
        self.shared.conversation.store(next, Ordering::Relaxed);
    }

    /// The line owed now, in the owner's language, as the route says it
    /// as written; none when nothing is owed.
    pub fn owed(&self) -> Option<(Moment, String)> {
        let (agent, moment) = self
            .shared
            .told
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .owed()?;
        let words = line(crate::language::at_start(), agent, moment);
        Some((moment, crate::onboarding::said_as_is(words)?))
    }

    /// The line for `moment` was said.
    pub fn said(&self, moment: Moment) {
        self.shared
            .told
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .said(moment);
    }
}

impl Drop for AgentSwitchTold {
    fn drop(&mut self) {
        self.shared.stopping.store(true, Ordering::Relaxed);
    }
}

/// Whether the lamp is in a conversation: a turn open (`turn_open`), or the
/// thread's conversation going on (`thread_going_on`, the lamp's words
/// within 45 s, not closed since). Nothing else: a face in view or the
/// room's talk is no conversation with the lamp.
pub fn in_conversation(turn_open: bool, thread_going_on: impl FnOnce() -> bool) -> bool {
    turn_open || thread_going_on()
}

#[cfg(test)]
mod tests;
