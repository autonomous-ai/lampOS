//! The policy as it runs: it listens on the bus, judges each thing that is
//! heard, and says what it would do about it.
//!
//! This version works in shadow mode. It decides, publishes the decision
//! on `policy/decisions` and writes it down, and nothing acts on it: what
//! it would have done can then be laid beside what the lamp did.
//!
//! The decision model is asked on a thread of its own, because it can take
//! seconds and the bus has to be read meanwhile: what the lamp says and
//! does while a judgement is awaited belongs to the next one. One question
//! is out at a time, and what is heard meanwhile waits its turn in a short
//! queue.
//!
//! Nothing that goes wrong around it stops it. A voice that is not there
//! yet, a model that cannot be reached, a log that cannot be written and a
//! bus that cannot be published to are each counted, said once when they
//! begin and once when they end ([`Note`]), and the loop goes on.
use crate::{
    attention::{self, Answers, Choice, Context, Judgement, Room},
    decide::{Asked, Connection, Decider, Failed, Usage},
    gather::{Gather, Lamp, Utterance},
    hello::{self, Hello},
    outcome::{Begun, Watch, sent_at},
    pictures::{self, Pictures},
    progress::{self, Progress},
    record::Record,
};
use lamp_bus::{Bus, Notice, Publisher, Subscriber};
use lamp_msgs::{
    AddonAsk, Decision, Diagnostic, Faces, Level, NameHeard, RoomState, Rules, SkillAsk, Speech,
    TaskStatus, Voice, VoiceArrival, VoiceState, VoiceTurn,
};
use serde::{Serialize, de::DeserializeOwned};
use std::{
    collections::{BTreeMap, VecDeque},
    fmt, io,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub mod baseline;
pub mod fit;
mod judge_log;
pub mod judges;
pub mod local;

/// The name the policy subscribes and publishes under.
pub const NAME: &str = "policy";
/// Nothing acts on what this version decides.
pub const SHADOW: bool = true;
/// No more utterances than this wait to be judged. When one more ends, the
/// one that has waited longest is let go: a judgement of something said
/// half a minute ago is of no use to anyone.
pub const MOST_WAITING: usize = 8;
/// Who decided, when no model did.
pub const RULE: &str = "rule";

/// What the voice heard, and what the lamp said.
const HEARD: &str = Speech::TOPICS[0];
const SAID: &str = Speech::TOPICS[1];
/// How long the loop waits for something heard before it looks at
/// everything else. An utterance is found over at most this long after its
/// pause.
const TICK: Duration = Duration::from_millis(20);
/// The same wait while a question is out: the answer is taken up at most
/// this long after it came.
const TICK_WHILE_ASKING: Duration = Duration::from_millis(5);
/// How often the lamp's Policy says how it is ([`Options::report_every`]).
pub const REPORT_EVERY: Duration = Duration::from_secs(1);
/// No more messages than this are taken from one topic in one turn of the
/// loop, so that a flood on one cannot keep the others from being read.
const MOST_AT_ONCE: usize = 64;

/// How the policy is to run.
#[derive(Clone, Debug)]
pub struct Options {
    /// Where the decisions log is kept: a directory of the owner's alone.
    pub log_dir: PathBuf,
    /// How long the decision model has for one asking.
    pub deadline: Duration,
    /// How long after its last piece an utterance is over.
    pub pause: Duration,
    /// The attention decisions act (`lamp-policy --act attention`, #800):
    /// published, and written down, with `shadow: false`, for the voice to
    /// hold the lamp's actions by (`LAMP_POLICY_HOLDS_ACTIONS=on`). Speech
    /// is not held. Off unless set: shadow.
    pub act_attention: bool,
    /// How often the Policy says how it is on the bus, and looks again
    /// for its decisions' publisher and for a part listening for the
    /// lamp's name. The lamp's is [`REPORT_EVERY`]. Tests give less, so
    /// that a rig that waits for a report after its cues does not wait a
    /// second (#1128).
    pub report_every: Duration,
}

/// What was counted from start to stop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    /// Utterances heard to their end.
    pub utterances: u64,
    /// Decisions made, and how many of them by the rule.
    pub decided: u64,
    pub by_rule: u64,
    /// Utterances let go unjudged because eight later ones were waiting.
    pub dropped: u64,
    /// Decisions that could not be published, and failed decision-log writes.
    /// In the opt-in path `unlogged` counts final-row queue rejections;
    /// `JudgesFinished` separately accounts for asynchronous write results.
    pub unpublished: u64,
    pub unlogged: u64,
    /// Messages on the bus that could not be read as their type.
    pub unreadable: u64,
    /// Skills' asks taken (`--gate`, #939).
    pub skill_asks: u64,
    /// Add-ons' says asked about (`addon/ask`, `--addons`).
    pub addon_asks: u64,
    /// Decisions whose second judgement (`--attention-next`, #1040) came to
    /// another choice.
    pub attention_next_differs: u64,
}

/// One line the policy says about itself.
///
/// A note is counts, times, choices and kinds, and nothing else. It cannot
/// hold what was heard or said: it is `Copy`, which no text that was put
/// together while running can be, and the only words in one are the names
/// of topics and of kinds of failure that this program was built with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Note {
    /// The opt-in path retains its local judge after losing the slow worker.
    SlowGone,
    /// Numeric answers from the opt-in judges; never speech or a key.
    Judge(judges::Notice),
    /// Accepted log records and completed writes are separate facts.
    JudgesFinished {
        queued: u64,
        written: u64,
        failed: u64,
        dropped: u64,
        abandoned: u64,
        pending: u64,
        drained: bool,
    },
    /// A progress decision was made (`lamp-policy --progress`): for the
    /// task and change of its progress line it was about.
    ProgressDecided {
        number: u64,
        task: u64,
        change: u32,
        choice: progress::Choice,
        probability: f32,
        by_rule: bool,
        took_ms: u32,
        /// A hard rule that turned the answer to `wait`.
        guard: Option<progress::Hold>,
    },
    /// A hello decision was made (`lamp-policy --hello`), for the arrival
    /// it names.
    HelloDecided {
        number: u64,
        arrival: u64,
        choice: progress::Choice,
        probability: f32,
        by_rule: bool,
        took_ms: u32,
        /// A hard rule that answered `wait`, or turned a yes to it.
        guard: Option<hello::Hold>,
    },
    /// The judge's answer for a hello the hard rules decided
    /// (`--hello-by-rule`), asked in shadow: never acted on.
    HelloJudged {
        arrival: u64,
        decided: progress::Choice,
        /// What the judge would have decided, if it answered.
        judged: Option<(progress::Choice, f32)>,
        took_ms: u32,
        failed: Option<&'static str>,
    },
    /// A decision was made.
    Decided {
        number: u64,
        choice: Choice,
        probability: f32,
        by_rule: bool,
        took_ms: u32,
        waited_ms: u32,
        /// What the asking was sent on, if the model answered over one.
        on: Option<Connection>,
        pieces: u32,
        bytes: u32,
    },
    /// The decision model failed, after answering or at the start.
    ModelFailing {
        kind: &'static str,
    },
    /// It answered, after failing this many times.
    ModelAnswering {
        failures: u64,
    },
    /// Nobody publishes this topic, for this time since the start.
    SourceGone {
        topic: &'static str,
        times: u64,
    },
    SourceBack {
        topic: &'static str,
    },
    /// A message on this topic was not of its type.
    Unreadable {
        topic: &'static str,
    },
    Readable {
        topic: &'static str,
        passed_over: u64,
    },
    /// This topic's socket could not be read.
    NotReceiving {
        topic: &'static str,
        kind: io::ErrorKind,
    },
    Receiving {
        topic: &'static str,
    },
    /// Utterances end faster than they are judged.
    Dropping,
    CaughtUp {
        dropped: u64,
    },
    /// The decisions log could not be written.
    LogFailing {
        kind: io::ErrorKind,
    },
    LogWriting {
        missing: u64,
    },
    /// The bus could not be published to.
    BusFailing {
        kind: io::ErrorKind,
    },
    BusWorking {
        unpublished: u64,
    },
    /// The thread that asks the decision model has ended.
    AskerGone,
    /// What the lamp did after a decision: whether it spoke, how long
    /// after the decision its first sound came (less than nothing if the
    /// sound came first), and how long after the last of what was heard.
    Outcome {
        number: u64,
        choice: Choice,
        spoke: bool,
        spoke_after_ms: Option<i64>,
        heard_to_sound_ms: Option<i64>,
        overtaken: bool,
        /// Original path: append completed. Opt-in path: queue accepted;
        /// consult `JudgesFinished` for actual append results.
        logged: bool,
    },
}

/// A time that may not be there, for a line of the journal.
struct Perhaps(Option<i64>);

impl fmt::Display for Perhaps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(time) => write!(f, "{time}"),
            None => f.write_str("none"),
        }
    }
}

impl fmt::Display for Note {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::SlowGone => {
                f.write_str("the slow judge worker has ended: the local judge continues")
            }
            Self::Judge(notice) => fmt::Display::fmt(&notice, f),
            Self::JudgesFinished {
                queued,
                written,
                failed,
                dropped,
                abandoned,
                pending,
                drained,
            } => write!(
                f,
                "judges log queued={queued} written={written} failed={failed} dropped={dropped} abandoned={abandoned} pending={pending} drained={drained}; written is append completion, not power-loss durability",
            ),
            Self::ProgressDecided {
                number,
                task,
                change,
                choice,
                probability,
                by_rule,
                took_ms,
                guard,
            } => write!(
                f,
                "progress decision {number} task={task} change={change} choice={} probability={probability:.3} by={} took_ms={took_ms} guard={}",
                choice.as_str(),
                if by_rule { RULE } else { "model" },
                guard.map_or("none", progress::Hold::as_str),
            ),
            Self::HelloDecided {
                number,
                arrival,
                choice,
                probability,
                by_rule,
                took_ms,
                guard,
            } => write!(
                f,
                "hello decision {number} arrival={arrival} choice={} probability={probability:.3} by={} took_ms={took_ms} guard={}",
                choice.as_str(),
                if by_rule { RULE } else { "model" },
                guard.map_or("none", hello::Hold::as_str),
            ),
            Self::HelloJudged {
                arrival,
                decided,
                judged,
                took_ms,
                failed,
            } => match judged {
                Some((choice, probability)) => write!(
                    f,
                    "hello judge in shadow arrival={arrival} decided={} judge={} probability={probability:.3} took_ms={took_ms}",
                    decided.as_str(),
                    choice.as_str(),
                ),
                None => write!(
                    f,
                    "hello judge in shadow arrival={arrival} decided={} judge=none failed={} took_ms={took_ms}",
                    decided.as_str(),
                    failed.unwrap_or("unknown"),
                ),
            },
            Self::Decided {
                number,
                choice,
                probability,
                by_rule,
                took_ms,
                waited_ms,
                on,
                pieces,
                bytes,
            } => write!(
                f,
                "decision {number} about={} choice={} probability={probability:.3} by={} took_ms={took_ms} waited_ms={waited_ms} on={} pieces={pieces} bytes={bytes}",
                attention::ABOUT,
                choice.as_str(),
                if by_rule { RULE } else { "model" },
                match on {
                    Some(Connection::New) => "new",
                    Some(Connection::Ahead) => "ahead",
                    Some(Connection::Kept) => "kept",
                    Some(Connection::Replaced) => "replaced",
                    None => "none",
                },
            ),
            Self::ModelFailing { kind } => write!(
                f,
                "the decision model is not answering ({kind}): the rule decides until it does"
            ),
            Self::ModelAnswering { failures } => {
                write!(
                    f,
                    "the decision model answers again after failures={failures}"
                )
            }
            Self::SourceGone { topic, times } => {
                write!(f, "{topic} has no publisher (times={times})")
            }
            Self::SourceBack { topic } => write!(f, "{topic} has a publisher again"),
            Self::Unreadable { topic } => write!(
                f,
                "a message on {topic} is not of its type: such messages are passed over"
            ),
            Self::Readable { topic, passed_over } => {
                write!(f, "{topic} is read again after passed_over={passed_over}")
            }
            Self::NotReceiving { topic, kind } => {
                write!(f, "{topic} cannot be received from ({kind})")
            }
            Self::Receiving { topic } => write!(f, "{topic} is received from again"),
            Self::Dropping => write!(
                f,
                "more than {MOST_WAITING} utterances wait to be judged: the oldest are let go"
            ),
            Self::CaughtUp { dropped } => {
                write!(f, "no utterance waits any more; dropped={dropped}")
            }
            Self::LogFailing { kind } => write!(
                f,
                "the decisions log cannot be written ({kind}): decisions go on without it"
            ),
            Self::LogWriting { missing } => {
                write!(f, "the decisions log is written again; missing={missing}")
            }
            Self::BusFailing { kind } => write!(
                f,
                "decisions cannot be published on the bus ({kind}): they go on without it"
            ),
            Self::BusWorking { unpublished } => write!(
                f,
                "decisions are published again; unpublished={unpublished}"
            ),
            Self::AskerGone => f.write_str(
                "the thread that asks the decision model has ended: the rule decides from now on",
            ),
            Self::Outcome {
                number,
                choice,
                spoke,
                spoke_after_ms,
                heard_to_sound_ms,
                overtaken,
                logged,
            } => write!(
                f,
                "outcome of decision {number} choice={} lamp={} spoke_after_ms={} heard_to_sound_ms={} overtaken={overtaken} logged={logged}",
                choice.as_str(),
                if spoke { "spoke" } else { "silent" },
                Perhaps(spoke_after_ms),
                Perhaps(heard_to_sound_ms),
            ),
        }
    }
}

/// One kind of trouble: counted while it lasts, and known to have begun
/// and to have ended, so that each can be said once.
#[derive(Default)]
struct Trouble {
    on: bool,
    lost: u64,
}

impl Trouble {
    /// It went wrong, and `lost` things with it. True where it begins.
    fn wrong(&mut self, lost: u64) -> bool {
        self.lost += lost;
        !std::mem::replace(&mut self.on, true)
    }

    /// It went right. What was lost while it lasted, where it ends.
    fn right(&mut self) -> Option<u64> {
        std::mem::take(&mut self.on).then(|| std::mem::take(&mut self.lost))
    }
}

/// One topic the policy listens to.
struct Ear {
    topic: &'static str,
    subscriber: Subscriber,
    unreadable: Trouble,
    failing: Trouble,
    gone: u64,
}

impl Ear {
    fn new(bus: &Bus, topic: &'static str, type_name: &str) -> io::Result<Self> {
        Ok(Self {
            topic,
            subscriber: bus.subscribe_typed(topic, NAME, type_name)?,
            unreadable: Trouble::default(),
            failing: Trouble::default(),
            gone: 0,
        })
    }

    /// What is waiting, read as `T`, after waiting at most `wait` for the
    /// first. A message that is not a `T` is counted and passed over, and
    /// nothing of it is said: what it holds may be what was heard.
    fn take<T: DeserializeOwned>(
        &mut self,
        wait: Duration,
        unreadable: &mut u64,
        say: &mut dyn FnMut(Note),
    ) -> Vec<T> {
        self.take_sent(wait, unreadable, say)
            .into_iter()
            .map(|(read, _)| read)
            .collect()
    }

    /// [`Self::take`], each with the stamp its publisher sent it with
    /// (`at_us`, the monotonic clock; 0 if it gave none), for the outcome
    /// (#1652).
    fn take_sent<T: DeserializeOwned>(
        &mut self,
        mut wait: Duration,
        unreadable: &mut u64,
        say: &mut dyn FnMut(Note),
    ) -> Vec<(T, u64)> {
        let topic = self.topic;
        let mut taken = Vec::new();
        while taken.len() < MOST_AT_ONCE {
            match self.subscriber.recv(wait) {
                Ok(Some(message)) => {
                    if self.failing.right().is_some() {
                        say(Note::Receiving { topic });
                    }
                    match message.parse::<T>() {
                        Ok(read) => {
                            if let Some(passed_over) = self.unreadable.right() {
                                say(Note::Readable { topic, passed_over });
                            }
                            taken.push((read, message.header.at_us));
                        }
                        Err(_) => {
                            *unreadable += 1;
                            if self.unreadable.wrong(1) {
                                say(Note::Unreadable { topic });
                            }
                        }
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    if self.failing.wrong(1) {
                        let kind = error.kind();
                        say(Note::NotReceiving { topic, kind });
                    }
                    // A socket that fails at once must not spin the loop.
                    thread::sleep(wait);
                    break;
                }
            }
            wait = Duration::ZERO;
        }
        taken
    }

    /// Its source's comings and goings, passed over unsaid: for a topic
    /// silent between its messages for longer than the bus's lease, whose
    /// source would otherwise be said gone and back around each one.
    fn sources_unsaid(&mut self) {
        let _ = self.subscriber.notices();
    }

    /// Say what has changed about the topic's source. True if it has just
    /// gone.
    fn source(&mut self, say: &mut dyn FnMut(Note)) -> bool {
        let topic = self.topic;
        let mut went = false;
        for notice in self.subscriber.notices() {
            match notice {
                Notice::SourceGone => {
                    self.gone += 1;
                    went = true;
                    say(Note::SourceGone {
                        topic,
                        times: self.gone,
                    });
                }
                Notice::SourceBack => {
                    went = false;
                    say(Note::SourceBack { topic });
                }
                _ => {}
            }
        }
        went
    }
}

/// An utterance that is over, with what was known when it began.
struct Waiting {
    utterance: Utterance,
    context: Context,
    /// When gathering released it, before any judge or queue could delay it.
    eligible: Instant,
    /// What the eyes saw as it ended, if they said anything lately.
    seen: Option<Seen>,
    /// The new judge's separate frozen input; absent in the original mode.
    local: Option<local::Cues>,
}

/// What the eyes saw: how many faces, how many of them near enough to be
/// somebody at the lamp, and how many of those turned to it. Written down
/// with each decision and not yet used to make it: nobody has stood in
/// front of the lamp since the eyes began to publish, so there is nothing
/// to weigh it by. Counts only: no box, no id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Seen {
    pub faces: u8,
    pub near: u8,
    pub facing: u8,
    /// How old the eyes' list was when the utterance ended.
    pub age_ms: u32,
}

/// A face this tall, as a share of the picture's height, is somebody at
/// the lamp: the line the eyes draw (`lamp-eyes`, `sight::LEAST_SIZE`).
const NEAR_HEIGHT: f32 = 0.15;
/// The eyes' list is good for this long (`lamp-eyes`, `bus_out::LASTS`).
const SEEN_FOR: Duration = Duration::from_secs(2);

/// The two waits of the loop's own a test runs short (#1806): the lamp's
/// unless told otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timing {
    /// How long the lamp's words are waited for after its first sound
    /// ([`crate::outcome::WORDS_FOR`]).
    pub words_for: Duration,
    /// How long the eyes' list counts ([`SEEN_FOR`], 2 s).
    pub seen_for: Duration,
}

impl Timing {
    pub const LAMP: Self = Self {
        words_for: crate::outcome::WORDS_FOR,
        seen_for: SEEN_FOR,
    };
}

impl Default for Timing {
    fn default() -> Self {
        Self::LAMP
    }
}

fn counted(faces: &Faces) -> (u8, u8, u8) {
    let near = |face: &&lamp_msgs::Face| face.ymax - face.ymin >= NEAR_HEIGHT;
    let count = |n: usize| u8::try_from(n).unwrap_or(u8::MAX);
    (
        count(faces.faces.len()),
        count(faces.faces.iter().filter(near).count()),
        count(
            faces
                .faces
                .iter()
                .filter(near)
                .filter(|face| face.facing == Some(true))
                .count(),
        ),
    )
}

/// What the asking thread is given to do.
enum Ask {
    /// Someone has begun to speak: a question is likely within seconds.
    Ready,
    /// The state to ask the questions about.
    About(String),
}

/// A hearing of the lamp's name is kept this long.
const NAMES_KEPT: Duration = Duration::from_secs(60);
/// The name counts for what was said if it was heard from this long
/// before the voice began to listen: the name is what makes it listen.
const NAME_BEFORE: Duration = Duration::from_secs(1);
/// When the voice began to listen is taken as the start of what was said
/// if it was no longer ago than this.
const LISTENED_WITHIN: Duration = Duration::from_secs(30);
/// Otherwise what was said is taken to have begun this long before its
/// text came.
const SAID_WITHIN: Duration = Duration::from_secs(8);

/// How long the model may take to get ready. It is not waited for: a
/// question that comes meanwhile is asked as soon as this is over.
const READY_WITHIN: Duration = Duration::from_millis(1_500);

/// The thread that asks the decision model, and the way to it.
struct Asker {
    states: mpsc::SyncSender<Ask>,
    answers: mpsc::Receiver<(Result<Asked, Failed>, Duration, Instant)>,
    thread: thread::JoinHandle<()>,
}

impl Asker {
    fn start<D: Decider + Send + 'static>(mut decider: D, deadline: Duration) -> io::Result<Self> {
        let (states, asked) = mpsc::sync_channel::<Ask>(1);
        let (answered, answers) = mpsc::channel();
        let thread = thread::Builder::new().name("ask".into()).spawn(move || {
            let questions = attention::questions();
            for ask in asked {
                let state = match ask {
                    Ask::Ready => {
                        decider.ready(READY_WITHIN.min(deadline));
                        continue;
                    }
                    Ask::About(state) => state,
                };
                let started = Instant::now();
                let answer = decider.ask(&state, &questions, deadline);
                if answered
                    .send((answer, started.elapsed(), Instant::now()))
                    .is_err()
                {
                    break;
                }
            }
        })?;
        Ok(Self {
            states,
            answers,
            thread,
        })
    }
}

/// One line of the decisions log.
#[derive(Serialize)]
struct Line<'a> {
    /// When the decision was made, in milliseconds since 1970.
    at_unix_ms: u64,
    about: &'static str,
    shadow: bool,
    /// What was known. `heard` is what was said, in words.
    context: &'a Context,
    in_conversation: bool,
    /// What the decision model was shown. Left out on a `certain` line:
    /// the model was shown nothing.
    #[serde(skip_serializing_if = "Option::is_none")]
    state: Option<serde_json::Value>,
    /// The judge that was asked. Left out on a `certain` line: none was.
    #[serde(skip_serializing_if = "Option::is_none")]
    asked: Option<&'a str>,
    /// What it answered to each question. Empty if it did not.
    answers: &'a BTreeMap<String, f32>,
    judgement: Judgement,
    /// The second judgement, in shadow (`--attention-next`, #1040): never
    /// published, never acted on. Left out without the switch, and when
    /// the rule decided.
    #[serde(skip_serializing_if = "Option::is_none")]
    next: Option<Judgement>,
    /// Who decided: the model as its service names it, or the rule.
    by: &'a str,
    /// How long the asking took, and how long the utterance had waited
    /// for it since its last piece.
    took_ms: u32,
    waited_ms: u32,
    /// What the asking was sent on: a connection made for it (`new`),
    /// made ahead of it (`ahead`), kept from one before (`kept`), or made
    /// in place of a kept one that had ended (`replaced`).
    #[serde(skip_serializing_if = "Option::is_none")]
    connection: Option<Connection>,
    /// Why the model's answers were not used, as a kind and in words.
    #[serde(skip_serializing_if = "Option::is_none")]
    failed: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    failed_why: Option<String>,
    /// The rule decided alone, its kind being one `--rule-decides` names:
    /// the model was not asked. Left out otherwise.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    certain: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage: Option<Usage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    request: Option<u64>,
    /// What the eyes saw as the utterance ended. Not used in the decision.
    #[serde(skip_serializing_if = "Option::is_none")]
    seen: Option<Seen>,
    /// How many pieces the utterance came in, and whether it was ended by
    /// its length.
    pieces: u32,
    cut: bool,
}

/// One line of the outcomes log: what the lamp did after a decision.
#[derive(Serialize)]
struct After<'a> {
    at_unix_ms: u64,
    about: &'static str,
    /// The `at_unix_ms` of the decision this is the outcome of.
    decision_at_unix_ms: u64,
    choice: Choice,
    /// `spoke` or `silent`.
    lamp: &'static str,
    /// From the decision to the lamp's first sound and to the first of
    /// its words in writing; from the last of what was heard to the first
    /// sound. Less than nothing if it came before.
    #[serde(skip_serializing_if = "Option::is_none")]
    spoke_after_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    said_after_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    heard_to_sound_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    request: Option<u64>,
    /// The lamp's words, as far as they had come.
    #[serde(skip_serializing_if = "str::is_empty")]
    said: &'a str,
    /// Another utterance was decided before this was known.
    overtaken: bool,
}

fn millis(time: Duration) -> u32 {
    u32::try_from(time.as_millis()).unwrap_or(u32::MAX)
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

/// Everything the loop holds between two turns.
struct Policy<'a> {
    bus: &'a Bus,
    say: &'a mut dyn FnMut(Note),
    started: Instant,
    /// What the judge that is asked is called.
    judge_name: String,
    asker: Asker,
    asker_gone: bool,
    /// The one question that is out, and when it went.
    asking: Option<(Waiting, Instant)>,
    waiting: VecDeque<Waiting>,
    gather: Gather,
    lamp: Lamp,
    /// When the lamp last began to speak, and the newest decision while
    /// what the lamp does about it is waited for, with its number.
    begun: Begun,
    watch: Option<(Watch, u64)>,
    outcomes: Record,
    /// What the room sounds like, as the voice last published it on
    /// `room/state`: not known until it has, or once it has gone.
    room: Room,
    /// The eyes' newest list, counted, and when it came.
    seen: Option<((u8, u8, u8), Instant)>,
    /// Each time the lamp's name was heard in its microphone's sound of
    /// late: when it began, and the score. And whether anything on the
    /// lamp is listening for the name at all, and when the voice last
    /// began to listen to someone.
    names: VecDeque<(Instant, f32)>,
    name_listening: bool,
    spoke_from: Option<Instant>,
    record: Record,
    out: Option<Publisher>,
    /// Where the policy says how it is itself: every `report_every`, once a
    /// second on the lamp.
    reports: Option<Publisher>,
    model: Trouble,
    log: Trouble,
    publishing: Trouble,
    dropping: Trouble,
    totals: Totals,
    /// These exist only through the explicitly enabled second-judge entry point.
    judges: Option<judges::Judges>,
    judge_log: Option<judge_log::Writer>,
    /// The progress question, only with `--progress` (`run_with_progress`).
    progress: Option<Progress>,
    /// Whether the attention decisions are shadow: always, unless
    /// `--act attention` ([`Options::act_attention`], #800).
    attention_shadow: bool,
    /// The second attention judgement, in shadow ([`Beside::attention_next`]).
    attention_next: bool,
    /// The kinds the rule decides alone ([`Beside::rule_decides`]).
    rule_decides: Vec<crate::replay::Kind>,
    /// The hello question, only with `--hello` (`run_with_questions`).
    hello: Option<Hello>,
    /// Skills' asks of an act not judged here yet, to be answered `wait`.
    not_judged: Vec<u64>,
    /// The picture's askings (`--images`, #871), if started.
    pictures: Option<Pictures>,
    /// The loop's own waits ([`Timing`]).
    timing: Timing,
}

impl Policy<'_> {
    /// An utterance is over: it waits to be judged, with what was known
    /// when it began.
    fn heard(&mut self, utterance: Utterance) {
        self.totals.utterances += 1;
        let context = Context {
            heard: utterance.text.clone(),
            room: self.room,
            lamp_is_speaking: utterance.lamp.speaking,
            lamp_last_said: attention::last_words(&utterance.lamp.said),
            seconds_since_lamp_spoke: utterance
                .lamp
                .spoke
                .map(|at| utterance.began.saturating_duration_since(at).as_secs_f32()),
            name_heard: self.name_heard(&utterance),
        };
        if self.waiting.len() == MOST_WAITING {
            self.waiting.pop_front();
            self.totals.dropped += 1;
            if self.dropping.wrong(1) {
                (self.say)(Note::Dropping);
            }
        }
        let seen = self.seen.and_then(|((faces, near, facing), at)| {
            let age = utterance.ended.saturating_duration_since(at);
            (age <= self.timing.seen_for).then_some(Seen {
                faces,
                near,
                facing,
                age_ms: millis(age),
            })
        });
        let local = self
            .judges
            .as_ref()
            .map(|judges| judges.cues(&context, &utterance, self.timing.seen_for));
        self.waiting.push_back(Waiting {
            utterance,
            context,
            eligible: Instant::now(),
            seen,
            local,
        });
    }

    /// Take up an answer if one has come, and ask the next question if
    /// none is out.
    fn judge(&mut self, now: Instant) {
        if let Some(mut judges) = self.judges.take() {
            judges.tick(self, now);
            self.judges = Some(judges);
            return;
        }
        match self.asker.answers.try_recv() {
            Ok((answer, took, _completed)) => {
                if let Some((waiting, sent)) = self.asking.take() {
                    self.decide(waiting, Some(answer), took, sent);
                }
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => self.asker_ended(now),
        }
        while self.asking.is_none() {
            let Some(waiting) = self.waiting.pop_front() else {
                break;
            };
            // A kind the record showed the model agrees with the rule on is
            // decided by the rule at once, and the model is not asked
            // (`--rule-decides`, #705): no wait for its answer.
            if self
                .rule_decides
                .contains(&crate::replay::Kind::of(&waiting.context))
            {
                self.decide(waiting, None, Duration::ZERO, now);
                continue;
            }
            let sent = if self.asker_gone {
                Err(mpsc::TrySendError::Disconnected(Ask::Ready))
            } else {
                let state = attention::state_text(&waiting.context);
                self.asker.states.try_send(Ask::About(state))
            };
            match sent {
                Ok(()) => {
                    // The picture's askings, if they are free: never waited for.
                    if let Some(pictures) = &self.pictures {
                        pictures.offer(pictures::Job {
                            context: waiting.context.clone(),
                            seen: waiting.seen,
                            sent_at_unix_ms: unix_ms(),
                        });
                    }
                    self.asking = Some((waiting, now));
                }
                // The thread has not come back for its next question yet.
                Err(mpsc::TrySendError::Full(_)) => {
                    self.waiting.push_front(waiting);
                    break;
                }
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    self.asker_ended(now);
                    self.decide(waiting, Some(Err(Self::no_asker())), Duration::ZERO, now);
                }
            }
        }
        if self.waiting.is_empty()
            && let Some(dropped) = self.dropping.right()
        {
            (self.say)(Note::CaughtUp { dropped });
        }
    }

    fn no_asker() -> Failed {
        Failed::Unreachable {
            during: "asking",
            kind: io::ErrorKind::BrokenPipe,
        }
    }

    /// The asking thread is no more. What it was asked is decided by the
    /// rule, and so is everything after it.
    fn asker_ended(&mut self, now: Instant) {
        if !std::mem::replace(&mut self.asker_gone, true) {
            (self.say)(Note::AskerGone);
        }
        if let Some((waiting, sent)) = self.asking.take() {
            let took = now.saturating_duration_since(sent);
            self.decide(waiting, Some(Err(Self::no_asker())), took, sent);
        }
    }

    /// Make the decision from the model's answers, or by the rule if there
    /// are none that can be used, and publish it and write it down. `None`:
    /// the model was not asked, its kind being one the rule decides
    /// (`--rule-decides`); that is no failure of the model's.
    fn decide(
        &mut self,
        waiting: Waiting,
        answer: Option<Result<Asked, Failed>>,
        took: Duration,
        sent: Instant,
    ) {
        let certain = answer.is_none();
        let answer = answer.unwrap_or(Err(Failed::Withdrawn));
        let Waiting {
            utterance,
            context,
            seen,
            local: _,
            eligible: _,
        } = waiting;
        let connection = answer.as_ref().ok().and_then(|asked| asked.connection);
        // The second judgement, in shadow, only from a model's answers.
        let mut next = None;
        let (judgement, answers, by, failed, usage) = match answer {
            Ok(asked) => match Answers::of(&asked.answers) {
                Ok(answers) => {
                    if self.attention_next {
                        next = Some(attention::combine_next(&answers, &context));
                    }
                    (
                        attention::combine(&answers, &context),
                        answers.by_name(),
                        asked.model,
                        None,
                        asked.usage,
                    )
                }
                // A judge that returned without all it was asked.
                Err(question) => (
                    attention::fallback(&context),
                    BTreeMap::new(),
                    RULE.to_owned(),
                    Some(Failed::Missing(question.to_owned())),
                    asked.usage,
                ),
            },
            Err(_) if certain => (
                attention::fallback(&context),
                BTreeMap::new(),
                RULE.to_owned(),
                None,
                None,
            ),
            Err(failed) => (
                attention::fallback(&context),
                BTreeMap::new(),
                RULE.to_owned(),
                Some(failed),
                None,
            ),
        };
        // What the lamp did about the one before is closed first: its
        // sound from here on is taken as its answer to this one.
        if let Some((watch, number)) = self.watch.take() {
            self.close(&watch, number, true);
        }
        self.totals.decided += 1;
        self.totals.by_rule += u64::from(judgement.from_rule);
        if next.is_some_and(|next| next.choice != judgement.choice) {
            self.totals.attention_next_differs += 1;
        }
        match &failed {
            // Not asked: says nothing of whether the model answers.
            _ if certain => {}
            Some(failed) => {
                if self.model.wrong(1) {
                    (self.say)(Note::ModelFailing {
                        kind: failed.kind(),
                    });
                }
            }
            None => {
                if let Some(failures) = self.model.right() {
                    (self.say)(Note::ModelAnswering { failures });
                }
            }
        }
        let took_ms = millis(took);
        let waited_ms = millis(sent.saturating_duration_since(utterance.ended));
        // Published before it is written down: whoever comes to act on a
        // decision must not wait for a disk.
        self.publish(&Decision {
            about: attention::ABOUT.to_owned(),
            choice: judgement.choice.as_str().to_owned(),
            probability: judgement.probability,
            answers: answers.clone(),
            by: by.clone(),
            took_ms,
            shadow: self.attention_shadow,
            request: utterance.request,
            task: None,
            arrival: None,
            ask: None,
            proposal: None,
        });
        let at_unix_ms = unix_ms();
        self.watch = Some((
            Watch::new(
                at_unix_ms,
                judgement.choice,
                Instant::now(),
                utterance.sent,
                &self.begun,
            ),
            self.totals.decided,
        ));
        let line = Line {
            at_unix_ms,
            about: attention::ABOUT,
            shadow: self.attention_shadow,
            context: &context,
            in_conversation: context.in_conversation(),
            state: (!certain).then(|| attention::state(&context)),
            asked: (!certain).then_some(self.judge_name.as_str()),
            answers: &answers,
            judgement,
            next,
            by: &by,
            took_ms,
            waited_ms,
            connection,
            failed: failed.as_ref().map(Failed::kind),
            failed_why: failed.as_ref().map(Failed::to_string),
            certain,
            usage,
            request: utterance.request,
            seen,
            pieces: utterance.pieces,
            cut: utterance.cut,
        };
        match self.record.write(at_unix_ms, &line) {
            Ok(()) => {
                if let Some(missing) = self.log.right() {
                    (self.say)(Note::LogWriting { missing });
                }
            }
            Err(error) => {
                self.totals.unlogged += 1;
                if self.log.wrong(1) {
                    (self.say)(Note::LogFailing { kind: error.kind() });
                }
            }
        }
        (self.say)(Note::Decided {
            number: self.totals.decided,
            choice: judgement.choice,
            probability: judgement.probability,
            by_rule: judgement.from_rule,
            took_ms,
            waited_ms,
            on: connection,
            pieces: utterance.pieces,
            bytes: u32::try_from(utterance.text.len()).unwrap_or(u32::MAX),
        });
    }

    /// Close the newest decision's outcome once nothing more of it is
    /// waited for.
    fn after(&mut self, now: Instant) {
        let words_for = self.timing.words_for;
        if let Some((watch, number)) = self.watch.take_if(|(watch, _)| watch.over(now, words_for)) {
            self.close(&watch, number, false);
        }
    }

    /// Write down what the lamp did after a decision, and say so.
    fn close(&mut self, watch: &Watch, number: u64, overtaken: bool) {
        let outcome = watch.outcome(overtaken);
        // Its words, if the answer it holds is the one this is about.
        let (request, words) = self.lamp.answer();
        let said = if outcome.request.is_some() && request == outcome.request {
            words
        } else {
            ""
        };
        let at_unix_ms = unix_ms();
        let line = After {
            at_unix_ms,
            about: attention::ABOUT,
            decision_at_unix_ms: outcome.decision_at_unix_ms,
            choice: outcome.choice,
            lamp: if outcome.spoke { "spoke" } else { "silent" },
            spoke_after_ms: outcome.spoke_after_ms,
            said_after_ms: outcome.said_after_ms,
            heard_to_sound_ms: outcome.heard_to_sound_ms,
            request: outcome.request,
            said,
            overtaken,
        };
        let logged = match self.judge_log.as_mut() {
            Some(writer) => writer.write(judge_log::Kind::Outcomes, at_unix_ms, &line),
            None => self.outcomes.write(at_unix_ms, &line).is_ok(),
        };
        (self.say)(Note::Outcome {
            number,
            choice: outcome.choice,
            spoke: outcome.spoke,
            spoke_after_ms: outcome.spoke_after_ms,
            heard_to_sound_ms: outcome.heard_to_sound_ms,
            overtaken,
            logged,
        });
    }

    /// Take up a progress answer, or put the next progress question, and
    /// publish and write down a decision that comes of it.
    fn progress_turn(&mut self, now: Instant) {
        let room = self.room;
        let seen_for = self.timing.seen_for;
        let someone_here = self.seen.and_then(|((_, near, _), at)| {
            (now.saturating_duration_since(at) <= seen_for).then_some(near > 0)
        });
        let Some(decided) = self
            .progress
            .as_mut()
            .and_then(|progress| progress.turn(now, unix_ms(), room, someone_here))
        else {
            return;
        };
        // Published before it is written down, as every decision is.
        self.publish(&decided.decision);
        let line = &decided.line;
        match self.record.write(line.at_unix_ms, line) {
            Ok(()) => {
                if let Some(missing) = self.log.right() {
                    (self.say)(Note::LogWriting { missing });
                }
            }
            Err(error) => {
                self.totals.unlogged += 1;
                if self.log.wrong(1) {
                    (self.say)(Note::LogFailing { kind: error.kind() });
                }
            }
        }
        let number = self.progress.as_ref().map_or(0, |p| p.totals().decided);
        (self.say)(Note::ProgressDecided {
            number,
            task: line.context.task,
            change: line.context.change,
            choice: line.judgement.choice,
            probability: line.judgement.probability,
            by_rule: line.judgement.from_rule,
            took_ms: line.took_ms,
            guard: line.guard,
        });
    }

    /// A skill's ask of an act not judged here yet (`news`, `unprompted`)
    /// is answered `wait` by the rule at once, so that no asker waits for
    /// nothing (#939: their question is to come).
    fn not_judged_turn(&mut self) {
        for ask in std::mem::take(&mut self.not_judged) {
            self.publish(&Decision {
                about: "moment".to_owned(),
                choice: "wait".to_owned(),
                probability: 0.0,
                answers: BTreeMap::new(),
                by: "rule".to_owned(),
                took_ms: 0,
                shadow: false,
                request: None,
                task: None,
                arrival: None,
                ask: Some(ask),
                proposal: None,
            });
        }
    }

    /// Take up the moment question's answer, or decide a skill's next ask
    /// of news or of anything unprompted, and publish and write down a
    /// decision that comes of it (`--gate`, #939).
    fn moment_turn(&mut self, now: Instant) {
        let room = self.room;
        let seen_for = self.timing.seen_for;
        let someone_here = self.seen.and_then(|((_, near, _), at)| {
            (now.saturating_duration_since(at) <= seen_for).then_some(near > 0)
        });
        let Some(decided) = self
            .hello
            .as_mut()
            .and_then(|hello| hello.moment_turn(now, unix_ms(), room, someone_here))
        else {
            return;
        };
        // Published first: the skill is waiting for it.
        self.publish(&decided.decision);
        let line = &decided.line;
        match self.record.write(line.at_unix_ms, line) {
            Ok(()) => {
                if let Some(missing) = self.log.right() {
                    (self.say)(Note::LogWriting { missing });
                }
            }
            Err(error) => {
                self.totals.unlogged += 1;
                if self.log.wrong(1) {
                    (self.say)(Note::LogFailing { kind: error.kind() });
                }
            }
        }
    }

    /// Take up a hello answer, or decide the next arrival, and publish and
    /// write down a decision that comes of it.
    fn hello_turn(&mut self, now: Instant) {
        let room = self.room;
        // Someone near, and whether such a face is turned to the lamp.
        let seen_for = self.timing.seen_for;
        let seen = self.seen.and_then(|((_, near, facing), at)| {
            (now.saturating_duration_since(at) <= seen_for)
                .then_some((Some(near > 0), (near > 0).then_some(facing > 0)))
        });
        // The judge's answer asked in shadow (`--hello-by-rule`): written
        // down only, after the hello it is about was decided and published.
        if let Some(judged) = self.hello.as_mut().and_then(Hello::judge_turn) {
            self.write_line(judged.at_unix_ms, &judged);
            (self.say)(Note::HelloJudged {
                arrival: judged.context.arrival,
                decided: judged.decided,
                judged: judged
                    .judgement
                    .map(|judgement| (judgement.choice, judgement.probability)),
                took_ms: judged.took_ms,
                failed: judged.failed,
            });
        }
        let Some(decided) = self
            .hello
            .as_mut()
            .and_then(|hello| hello.turn(now, unix_ms(), room, seen.unwrap_or((None, None))))
        else {
            return;
        };
        // Published first: the voice is waiting for it.
        self.publish(&decided.decision);
        let line = &decided.line;
        self.write_line(line.at_unix_ms, line);
        let number = self.hello.as_ref().map_or(0, |h| h.totals().decided);
        (self.say)(Note::HelloDecided {
            number,
            arrival: line.context.arrival,
            choice: line.judgement.choice,
            probability: line.judgement.probability,
            by_rule: line.judgement.from_rule,
            took_ms: line.took_ms,
            guard: line.guard,
        });
    }

    /// Write one line to the decisions log, saying when writing fails and
    /// when it works again.
    fn write_line<T: serde::Serialize>(&mut self, unix_ms: u64, line: &T) {
        match self.record.write(unix_ms, line) {
            Ok(()) => {
                if let Some(missing) = self.log.right() {
                    (self.say)(Note::LogWriting { missing });
                }
            }
            Err(error) => {
                self.totals.unlogged += 1;
                if self.log.wrong(1) {
                    (self.say)(Note::LogFailing { kind: error.kind() });
                }
            }
        }
    }

    fn publish(&mut self, decision: &Decision) {
        let at_us = self.started.elapsed().as_micros() as u64;
        let sent = match self.out.as_mut() {
            Some(out) => out.publish_json(at_us, decision).map(|_| ()),
            None => Err(io::ErrorKind::NotConnected.into()),
        };
        // A publisher says of a root it cannot use once, and then takes
        // each message as before: while that trouble lasts, what it takes
        // goes nowhere.
        if let Err(error) = &sent {
            self.totals.unpublished += 1;
            if self.publishing.wrong(1) {
                (self.say)(Note::BusFailing { kind: error.kind() });
            }
        } else if self.publishing.on {
            self.totals.unpublished += 1;
            self.publishing.wrong(1);
        }
    }

    /// The highest score with which the lamp's name was heard while this
    /// was being said: from a second before the voice began to listen to
    /// whoever said it until its text came. Nothing heard is 0, and with
    /// nothing on the lamp listening for the name there is no answer.
    ///
    /// The name is said at the start and the text comes after the end,
    /// so the two are joined by when the voice began to listen. If that
    /// is not known, or was long before, the last [`SAID_WITHIN`] count.
    fn name_heard(&self, utterance: &Utterance) -> Option<f32> {
        if !self.name_listening {
            return None;
        }
        let back = |from: Instant, by: Duration| from.checked_sub(by).unwrap_or(from);
        let began = self
            .spoke_from
            .filter(|at| {
                *at <= utterance.began && utterance.began.duration_since(*at) <= LISTENED_WITHIN
            })
            .unwrap_or_else(|| back(utterance.began, SAID_WITHIN));
        let from = back(began, NAME_BEFORE);
        Some(
            self.names
                .iter()
                .filter(|(at, _)| *at >= from && *at <= utterance.ended)
                .map(|(_, score)| *score)
                .fold(0.0, f32::max),
        )
    }

    /// How the policy itself is, for the lamp's health (`diagnostics`).
    /// It is well while a model answers and the log is written; with
    /// either gone it still decides, and says which it is without.
    fn report(&self) -> Diagnostic {
        if let Some(judges) = self.judges.as_ref() {
            return judges.report(self);
        }
        let (level, message) = if self.asker_gone {
            (
                Level::Error,
                "it can no longer ask the decision model: the rule decides",
            )
        } else if self.model.on {
            (
                Level::Warn,
                "the decision model is not answering: the rule decides until it does",
            )
        } else if self.log.on {
            (
                Level::Warn,
                "its log cannot be written: decisions go on without it",
            )
        } else {
            (Level::Ok, "")
        };
        let report = Diagnostic::new(level, NAME, message)
            .with("decided", self.totals.decided)
            .with("by_rule", self.totals.by_rule)
            .with("shadow", self.attention_shadow);
        // Only with `--attention-next`.
        let report = if self.attention_next {
            report.with("attention_next_differs", self.totals.attention_next_differs)
        } else {
            report
        };
        // Whether anything on the lamp listens for its name, as found this
        // second; left out when nothing does, so the report is as it was.
        let report = if self.name_listening {
            report.with("name_listening", true)
        } else {
            report
        };
        // Only with `--progress`: without it the report is as it was.
        let report = match self.progress.as_ref() {
            Some(progress) => {
                let totals = progress.totals();
                let report = report
                    .with("progress_decided", totals.decided)
                    .with("progress_by_rule", totals.by_rule)
                    .with("progress_said", totals.said)
                    .with("progress_held_talking", totals.held_talking)
                    .with("progress_held_recent", totals.held_recent);
                // Only with `--rules`.
                let report = if progress.obeys_rules() {
                    report.with("progress_held_rule", totals.held_rule)
                } else {
                    report
                };
                // Only with `--act progress`.
                if progress.acts() {
                    report.with("progress_acts", true)
                } else {
                    report
                }
            }
            None => report,
        };
        // Only with `--hello`, and `hello_acts` only with `--act hello`.
        match self.hello.as_ref() {
            Some(hello) => {
                let totals = hello.totals();
                let report = report
                    .with("hello_decided", totals.decided)
                    .with("hello_by_rule", totals.by_rule)
                    .with("hello_said", totals.said)
                    .with("hello_held_talking", totals.held_talking)
                    .with("hello_held_recent", totals.held_recent)
                    .with("hello_held_person", totals.held_person)
                    .with("hello_late", totals.late);
                // Only with `--rules`.
                let report = if hello.obeys_rules() {
                    report.with("hello_held_rule", totals.held_rule)
                } else {
                    report
                };
                // Only with `--hello-by-rule`: the judge asked in shadow.
                let report = if hello.by_rule() {
                    report
                        .with("hello_judged", totals.judged)
                        .with("hello_judge_differs", totals.judge_differs)
                } else {
                    report
                };
                // Only with `--gate`.
                let report = if hello.answers_skills() {
                    report.with("skill_asks", self.totals.skill_asks)
                } else {
                    report
                };
                // Only with `--addons`.
                let report = if hello.answers_addons() {
                    report.with("addon_asks", self.totals.addon_asks)
                } else {
                    report
                };
                let report = match hello.moment_totals() {
                    Some(moment) => {
                        let report = report
                            .with("moment_decided", moment.decided)
                            .with("moment_said", moment.said)
                            .with("moment_held_talking", moment.held_talking)
                            .with("moment_late", moment.late);
                        if hello.obeys_rules() {
                            report.with("moment_held_rule", moment.held_rule)
                        } else {
                            report
                        }
                    }
                    None => report,
                };
                if hello.acts() {
                    report.with("hello_acts", true)
                } else {
                    report
                }
            }
            None => report,
        }
    }

    /// Every `report_every`, once a second on the lamp: say how the policy
    /// is, and show the bus that it is alive though it has decided nothing, which is also how it learns
    /// that the bus can be published to again.
    fn second(&mut self) {
        // Whether anything on the lamp is listening for its name: a part
        // that publishes hearings shows itself alive there between them.
        self.name_listening = self
            .bus
            .publishers(NameHeard::TOPIC)
            .is_ok_and(|live| !live.is_empty());
        if self.reports.is_none() {
            self.reports = self
                .bus
                .publisher_declared(Diagnostic::TOPIC, &Diagnostic::declaration(), NAME)
                .ok();
        }
        let report = self.report();
        if let Some(reports) = self.reports.as_mut() {
            // A report that cannot be sent is not a trouble of its own:
            // the decisions' publisher below meets the same bus and says.
            let at_us = self.started.elapsed().as_micros() as u64;
            let _ = reports.publish_json(at_us, &report);
        }
        if self.out.is_none() {
            let made = self
                .bus
                .publisher_declared(Decision::TOPIC, &Decision::declaration(), NAME);
            match made {
                Ok(out) => self.out = Some(out),
                Err(error) => {
                    if self.publishing.wrong(0) {
                        (self.say)(Note::BusFailing { kind: error.kind() });
                    }
                    return;
                }
            }
        }
        let Some(out) = self.out.as_mut() else {
            return;
        };
        match out.alive() {
            Ok(()) => {
                if let Some(unpublished) = self.publishing.right() {
                    (self.say)(Note::BusWorking { unpublished });
                }
            }
            Err(error) => {
                if self.publishing.wrong(0) {
                    (self.say)(Note::BusFailing { kind: error.kind() });
                }
            }
        }
    }
}

/// Run the policy on `bus` until `stop` is set: subscribe to what the
/// voice hears, says and is doing, judge each utterance with `decider`,
/// and publish and write down each decision.
///
/// It fails only at the start, if it cannot subscribe or start its thread.
/// After that nothing ends it but `stop`. `say` is given each [`Note`] as
/// it happens.
pub fn run<D: Decider + Send + 'static>(
    bus: &Bus,
    decider: D,
    options: &Options,
    stop: &AtomicBool,
    say: &mut dyn FnMut(Note),
) -> io::Result<Totals> {
    run_inner(bus, decider, options, stop, say, None, Beside::default())
}

/// [`run`], and beside it the progress question ([`crate::progress`]):
/// `tasks/status` is listened to, and `progress_decider`, a judge of its
/// own on a thread of its own, is asked whether a change is worth telling
/// now. Shadow unless `progress.act` (`--act progress`); "is this said to
/// the lamp?" is shadow always. Only `lamp-policy --progress` comes here.
pub fn run_with_progress<D, P>(
    bus: &Bus,
    decider: D,
    options: &Options,
    stop: &AtomicBool,
    say: &mut dyn FnMut(Note),
    progress_decider: P,
    progress: progress::Options,
) -> io::Result<Totals>
where
    D: Decider + Send + 'static,
    P: Decider + Send + 'static,
{
    let progress = Progress::start(progress_decider, progress)?;
    run_beside(
        bus,
        decider,
        options,
        stop,
        say,
        Beside {
            progress: Some(progress),
            ..Beside::default()
        },
    )
}

/// [`run`], and beside it whichever of the questions that may speak first
/// are given, each started with its own judge ([`Progress::start`],
/// [`Hello::start`]): progress listens to `tasks/status`, hello to
/// `voice/arrival`. Each is shadow unless its own options say it acts;
/// "is this said to the lamp?" is shadow always.
pub fn run_with_questions<D: Decider + Send + 'static>(
    bus: &Bus,
    decider: D,
    options: &Options,
    stop: &AtomicBool,
    say: &mut dyn FnMut(Note),
    progress: Option<Progress>,
    hello: Option<Hello>,
) -> io::Result<Totals> {
    run_beside(
        bus,
        decider,
        options,
        stop,
        say,
        Beside {
            progress,
            hello,
            ..Beside::default()
        },
    )
}

/// What may run beside the attention question, each only if given: the
/// questions that may speak first, and the picture's askings
/// ([`crate::pictures`], #871), which each attention question is also
/// handed to as it is sent, if they are free, and which write their own
/// lines. Shadow: the decision never waits for them.
#[derive(Default)]
pub struct Beside {
    pub progress: Option<Progress>,
    pub hello: Option<Hello>,
    pub pictures: Option<Pictures>,
    /// The second attention judgement proposed on #1040
    /// ([`attention::combine_next`], `--attention-next`): written on each
    /// decision's line as `next` and counted, never published and never
    /// acted on.
    pub attention_next: bool,
    /// The kinds of attention decision the rule makes alone, without asking
    /// the model (`--rule-decides`, #705): the record showed the model
    /// agreeing with the rule on them. Empty: every decision is asked.
    pub rule_decides: Vec<crate::replay::Kind>,
    /// The loop's own waits: [`Timing::LAMP`] unless a test runs them short.
    pub timing: Timing,
}

/// [`run`], with whatever [`Beside`] gives. With nothing, it is [`run`].
pub fn run_beside<D: Decider + Send + 'static>(
    bus: &Bus,
    decider: D,
    options: &Options,
    stop: &AtomicBool,
    say: &mut dyn FnMut(Note),
    beside: Beside,
) -> io::Result<Totals> {
    run_inner(bus, decider, options, stop, say, None, beside)
}

/// The separate, explicitly enabled second-judge path. It is always shadow.
pub fn run_with_local<D: Decider + Send + 'static>(
    bus: &Bus,
    decider: D,
    options: &Options,
    stop: &AtomicBool,
    say: &mut dyn FnMut(Note),
    local: judges::Options,
) -> io::Result<Totals> {
    local.validate()?;
    run_inner(
        bus,
        decider,
        options,
        stop,
        say,
        Some(local),
        Beside::default(),
    )
}

fn run_inner<D: Decider + Send + 'static>(
    bus: &Bus,
    decider: D,
    options: &Options,
    stop: &AtomicBool,
    say: &mut dyn FnMut(Note),
    local: Option<judges::Options>,
    beside: Beside,
) -> io::Result<Totals> {
    let Beside {
        progress,
        hello,
        pictures,
        attention_next,
        rule_decides,
        timing,
    } = beside;
    let mut heard = Ear::new(bus, HEARD, Speech::TYPE)?;
    let mut said = Ear::new(bus, SAID, Speech::TYPE)?;
    let mut voice = Ear::new(bus, VoiceState::TOPIC, VoiceState::TYPE)?;
    let mut room = Ear::new(bus, RoomState::TOPIC, RoomState::TYPE)?;
    let mut eyes = Ear::new(bus, Faces::TOPIC, Faces::TYPE)?;
    let mut names = Ear::new(bus, NameHeard::TOPIC, NameHeard::TYPE)?;
    // Without `--progress` the topic is not subscribed to at all.
    let mut tasks = match progress {
        Some(_) => Some(Ear::new(bus, TaskStatus::TOPIC, TaskStatus::TYPE)?),
        None => None,
    };
    // Nor, without `--rules`, the people's rules about speaking first.
    let obeys_rules = progress.as_ref().is_some_and(Progress::obeys_rules)
        || hello.as_ref().is_some_and(Hello::obeys_rules);
    let mut kept_rules = match obeys_rules {
        true => Some(Ear::new(bus, Rules::TOPIC, Rules::TYPE)?),
        false => None,
    };
    // Without `--hello` the voice's arrivals are not listened to at all.
    let mut arrivals = match hello {
        Some(_) => Some(Ear::new(bus, VoiceArrival::TOPIC, VoiceArrival::TYPE)?),
        None => None,
    };
    // The voice's verdict on each turn, only with `--hello-quiet
    // to-the-lamp` (#1155): a turn held back as not said to the lamp is
    // taken back from the hello's quiet.
    let mut turns = match hello.as_ref() {
        Some(hello) if hello.quiet_to_the_lamp() => {
            Some(Ear::new(bus, VoiceTurn::TOPIC, VoiceTurn::TYPE)?)
        }
        _ => None,
    };
    // Nor, without `--gate`, the skills' asks (#939).
    let mut skill_asks = match hello.as_ref() {
        Some(hello) if hello.answers_skills() => {
            Some(Ear::new(bus, SkillAsk::TOPIC, SkillAsk::TYPE)?)
        }
        _ => None,
    };
    // Nor, without `--addons`, the add-ons' says (#2530 part 2).
    let mut addon_asks = match hello.as_ref() {
        Some(hello) if hello.answers_addons() => {
            Some(Ear::new(bus, AddonAsk::TOPIC, AddonAsk::TYPE)?)
        }
        _ => None,
    };
    let judge_name = decider.name().to_owned();
    let (judges, judge_log) = match local {
        Some(local_options) => (
            Some(judges::Judges::new(local_options)),
            Some(judge_log::Writer::start(&options.log_dir)?),
        ),
        None => (None, None),
    };
    let mut policy = Policy {
        bus,
        say,
        started: Instant::now(),
        judge_name,
        asker: Asker::start(decider, options.deadline)?,
        asker_gone: false,
        asking: None,
        waiting: VecDeque::new(),
        gather: Gather::new(options.pause),
        lamp: Lamp::default(),
        begun: Begun::default(),
        watch: None,
        outcomes: Record::of("outcomes", &options.log_dir),
        room: Room::Unknown,
        seen: None,
        names: VecDeque::new(),
        name_listening: false,
        spoke_from: None,
        record: Record::new(&options.log_dir),
        out: None,
        reports: None,
        model: Trouble::default(),
        log: Trouble::default(),
        publishing: Trouble::default(),
        dropping: Trouble::default(),
        totals: Totals::default(),
        judges,
        judge_log,
        progress,
        attention_shadow: SHADOW && !options.act_attention,
        attention_next,
        rule_decides,
        hello,
        not_judged: Vec::new(),
        pictures,
        timing,
    };
    policy.second();
    if let Some(judges) = policy.judges.take() {
        judges.record_model(&mut policy);
        policy.judges = Some(judges);
    }
    let mut shown = Instant::now();
    let mut was_listening = false;
    while !stop.load(Ordering::Relaxed) {
        // What the lamp is doing and saying first, so that a piece heard
        // in the same turn finds it as it was. What the lamp says while
        // this turn waits for a piece is taken in with the next turn: an
        // utterance may miss the lamp's last 20 ms, and never has any of
        // what came after its own beginning.
        let now = Instant::now();
        let now_us = lamp_ipc::monotonic_us();
        let unreadable = &mut policy.totals.unreadable;
        // A turn's verdict before the voice's state: one for the last turn
        // is its, though the next has begun in the same look (#1155). Said
        // once a turn, silent between: its source is not said gone and back
        // around each, as the voice's words were on 14 October.
        if let Some(turns) = turns.as_mut() {
            for turn in turns.take::<VoiceTurn>(Duration::ZERO, unreadable, policy.say) {
                if let Some(hello) = policy.hello.as_mut() {
                    hello.verdict(turn.request, turn.verdict);
                }
            }
            turns.sources_unsaid();
        }
        for (state, at_us) in voice.take_sent::<VoiceState>(Duration::ZERO, unreadable, policy.say)
        {
            if let Some(judges) = policy.judges.as_mut() {
                judges.voice_seen(now);
            }
            if let Some(progress) = policy.progress.as_mut() {
                progress.voice(state.state, now);
            }
            if let Some(hello) = policy.hello.as_mut() {
                hello.voice(state.state, now);
            }
            let speaking = state.state == Voice::Speaking;
            policy.lamp.state(speaking, now);
            // The outcome times the lamp's sound by its message's stamp, so
            // that a turn that takes it up with what was heard before it,
            // this first, still has them in order (#1652). What the policy
            // decides keeps the time it was taken up.
            let sent = sent_at(at_us, now, now_us);
            if policy.begun.state(speaking, sent)
                && let Some((watch, _)) = policy.watch.as_mut()
            {
                watch.spoke(sent);
            }
            let listening = state.state == Voice::Listening;
            policy.gather.person_stopped(!listening);
            // Someone has begun to speak, and what they say will be asked
            // about when they stop: the model gets ready while they talk.
            // With a question out or waiting the thread is busy, and it
            // is ready already.
            let began = listening && !std::mem::replace(&mut was_listening, true);
            if began {
                policy.spoke_from = Some(now);
            }
            if began
                && policy.judges.is_none()
                && policy.asking.is_none()
                && policy.waiting.is_empty()
                && !policy.asker_gone
            {
                let _ = policy.asker.states.try_send(Ask::Ready);
            }
            was_listening = listening;
        }
        if voice.source(policy.say) {
            if let Some(judges) = policy.judges.as_mut() {
                judges.voice_gone(now);
            }
            policy.lamp.gone();
            policy.begun.gone();
            policy.gather.person_stopped(false);
            was_listening = false;
            if let Some(progress) = policy.progress.as_mut() {
                progress.voice_gone();
            }
            if let Some(hello) = policy.hello.as_mut() {
                hello.voice_gone();
            }
        }
        for (piece, at_us) in said.take_sent::<Speech>(Duration::ZERO, unreadable, policy.say) {
            if let Some(progress) = policy.progress.as_mut() {
                progress.talk(now);
            }
            if let Some(hello) = policy.hello.as_mut() {
                hello.talk(now);
            }
            policy.lamp.said(&piece.text, piece.request, now);
            let sent = sent_at(at_us, now, now_us);
            if policy.begun.said(piece.request, sent)
                && let Some((watch, _)) = policy.watch.as_mut()
            {
                watch.said(piece.request, sent);
            }
        }
        said.source(policy.say);
        // How the room is and who the eyes see, as each was last said. A
        // source that has gone leaves its fact not known, never as it was.
        for state in room.take::<RoomState>(Duration::ZERO, unreadable, policy.say) {
            policy.room = match state.state {
                lamp_msgs::Room::Quiet => Room::Quiet,
                lamp_msgs::Room::Busy => Room::Busy,
                lamp_msgs::Room::Unknown => Room::Unknown,
            };
        }
        if room.source(policy.say) {
            policy.room = Room::Unknown;
        }
        for faces in eyes.take::<Faces>(Duration::ZERO, unreadable, policy.say) {
            if let Some(judges) = policy.judges.as_mut() {
                judges.faces(&faces, now);
            }
            policy.seen = Some((counted(&faces), now));
            // What the picture's askings look at as they go (#871).
            if let Some(pictures) = policy.pictures.as_ref() {
                pictures.presence().saw(counted(&faces).0, now);
            }
        }
        if eyes.source(policy.say) {
            if let Some(judges) = policy.judges.as_mut() {
                judges.eyes_gone(now);
            }
            policy.seen = None;
            if let Some(pictures) = policy.pictures.as_ref() {
                pictures.presence().gone();
            }
        }
        // Each hearing of the lamp's name, dated from when it began.
        for heard in names.take::<NameHeard>(Duration::ZERO, unreadable, policy.say) {
            let began = now
                .checked_sub(Duration::from_millis(u64::from(heard.ago_ms)))
                .unwrap_or(now);
            policy.names.push_back((began, heard.score));
        }
        names.source(policy.say);
        // What the people at the lamp asked about speaking first, newest,
        // with `--rules` only. Kept when the voice goes: a rule does not end
        // with it.
        if let Some(kept_rules) = kept_rules.as_mut() {
            for told in kept_rules.take::<Rules>(Duration::ZERO, unreadable, policy.say) {
                if let Some(hello) = policy.hello.as_mut().filter(|hello| hello.obeys_rules()) {
                    hello.rules(told.clone());
                }
                if let Some(progress) = policy.progress.as_mut().filter(|p| p.obeys_rules()) {
                    progress.rules(told);
                }
            }
            kept_rules.source(policy.say);
        }
        // A hello due, with `--hello` only.
        if let Some(arrivals) = arrivals.as_mut() {
            for arrival in arrivals.take::<VoiceArrival>(Duration::ZERO, unreadable, policy.say) {
                if let Some(hello) = policy.hello.as_mut() {
                    hello.arrival(arrival, now);
                }
            }
            arrivals.source(policy.say);
        }
        // A skill asks whether the lamp may do something it would start
        // itself, with `--gate` only (#939): a hello is put to the hello
        // question; news, a reminder and anything unprompted to the moment
        // question. Anything not judged here is answered `wait` by the rule
        // at once, so that no asker waits for nothing.
        if let Some(skill_asks) = skill_asks.as_mut() {
            for ask in skill_asks.take::<SkillAsk>(Duration::ZERO, unreadable, policy.say) {
                if ask.check().is_err() {
                    continue;
                }
                policy.totals.skill_asks += 1;
                if ask.act == crate::hello::ABOUT {
                    if let Some(hello) = policy.hello.as_mut() {
                        hello.asked(&ask, now);
                    }
                } else if !policy
                    .hello
                    .as_mut()
                    .is_some_and(|hello| hello.asked_moment(&ask, now))
                {
                    policy.not_judged.push(ask.ask);
                }
            }
            skill_asks.source(policy.say);
        }
        // An add-on's say past its judge, with `--addons` only: put to the
        // moment question, its decision `about` `addon`. Only the add-ons'
        // judge writes `addon/ask`.
        if let Some(addon_asks) = addon_asks.as_mut() {
            for ask in addon_asks.take::<AddonAsk>(Duration::ZERO, unreadable, policy.say) {
                if ask.check().is_err() {
                    continue;
                }
                policy.totals.addon_asks += 1;
                if let Some(hello) = policy.hello.as_mut() {
                    hello.asked_addon(&ask, now);
                }
            }
            addon_asks.source(policy.say);
        }
        // How the work handed over is going, with `--progress` only.
        if let Some(tasks) = tasks.as_mut() {
            for status in tasks.take::<TaskStatus>(Duration::ZERO, unreadable, policy.say) {
                if let Some(progress) = policy.progress.as_mut() {
                    progress.status(status);
                }
            }
            if tasks.source(policy.say)
                && let Some(progress) = policy.progress.as_mut()
            {
                progress.gone();
            }
        }
        while policy
            .names
            .front()
            .is_some_and(|(at, _)| now.saturating_duration_since(*at) > NAMES_KEPT)
        {
            policy.names.pop_front();
        }
        // The wait of the loop is here: for something to be heard.
        let wait = if let Some(judges) = policy.judges.as_ref() {
            judges.wait(Instant::now())
        } else if policy.asking.is_some() || policy.hello.as_ref().is_some_and(Hello::busy) {
            TICK_WHILE_ASKING
        } else {
            TICK
        };
        let pieces = heard.take_sent::<Speech>(wait, unreadable, policy.say);
        heard.source(policy.say);
        let now = Instant::now();
        let now_us = lamp_ipc::monotonic_us();
        if !pieces.is_empty() {
            if let Some(progress) = policy.progress.as_mut() {
                progress.talk(now);
            }
            if let Some(hello) = policy.hello.as_mut() {
                for (piece, _) in &pieces {
                    hello.heard(now, piece.request);
                }
            }
        }
        for (piece, at_us) in pieces {
            policy.gather.piece_sent(
                &piece.text,
                piece.finished,
                piece.request,
                &policy.lamp,
                now,
                sent_at(at_us, now, now_us),
            );
        }
        policy.gather.tick(now);
        while let Some(utterance) = policy.gather.over() {
            policy.heard(utterance);
        }
        policy.judge(now);
        policy.hello_turn(now);
        policy.moment_turn(now);
        policy.not_judged_turn();
        policy.progress_turn(now);
        policy.after(now);
        if now.saturating_duration_since(shown) >= options.report_every {
            shown = now;
            policy.second();
        }
    }
    // The original path waits for its Decider contract. The opt-in path
    // never lets one broken/slow remote judge hold shutdown or later local
    // decisions; its single outstanding worker may finish after this run.
    let was_local = policy.judges.is_some();
    if let Some(mut judges) = policy.judges.take() {
        judges.finish(&mut policy);
    }
    if let Some(writer) = policy.judge_log.take() {
        let finished = writer.finish(Duration::from_millis(100));
        let counts = finished.stats;
        (policy.say)(Note::JudgesFinished {
            queued: counts.queued,
            written: counts.written,
            failed: counts.failed,
            dropped: counts.dropped,
            abandoned: counts.abandoned,
            pending: counts.pending,
            drained: finished.drained,
        });
    }
    let Policy { asker, totals, .. } = policy;
    drop(asker.states);
    if !was_local || asker.thread.is_finished() {
        let _ = asker.thread.join();
    }
    Ok(totals)
}
