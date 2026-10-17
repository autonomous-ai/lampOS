//! News from the agents the lamp handed work to (`docs/tasks.md`, "Telling
//! the person").
//!
//! The lamp is the person's chief of staff: it hands work to Claude Code or
//! Codex on another computer ([`crate::agent_tasks`]) and tells the person
//! when the work comes back, unasked, the way someone at the next desk
//! would: "Oh, Claude Code just finished the names you asked for." The
//! tasks part watches each task handed over (unless `lamp-tasks serve --no-watch`)
//! and keeps what its agent came back with once it has finished, stopped,
//! or stopped to ask something. A thread of the voice's own reads that
//! every ten seconds, and at once when `tasks/status` lists a task newly
//! settled (#560), and offers each piece of news once; the coordinator
//! tells it at a good moment (someone there, nothing being said, the room
//! not busy) as a turn the lamp begins itself, and marks it told. What was
//! told is kept in a file, so a restart does not tell it again.
//!
//! On whenever the tasks part is ([`crate::agent_tasks::ENVIRONMENT`]).
//! `LAMP_TASKS_TOLD` names the file of what was told, [`DEFAULT_TOLD`]
//! unless set.
//!
//! **Progress** while an agent works (#732, #752) is told too, only with
//! [`PROGRESS_ENVIRONMENT`] `on`: the policy decides on `policy/decisions`
//! whether a change of the agent's progress line is worth telling now
//! (`lamp-policy --progress --act progress`), and a `say_now` that is fresh,
//! of this store and about the newest change on `tasks/status` is fetched
//! (`get_progress`) and offered here like any other news, under the same
//! gates.
use lamp_bus::{Bus, Subscriber};
use lamp_msgs::{AgentState, Decision, Sign, TaskStatus};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// Where the coordinator reads the file of what was told.
pub const TOLD_ENVIRONMENT: &str = "LAMP_TASKS_TOLD";
/// `skill`: news of work finished, stopped or asking waits for a skill's
/// `from: tasks` ask, at most [`crate::say::SKILL_WAITS_US`], and is then
/// told with its ticket, or by the voice itself (#939, (c)). Progress is
/// the Policy's to time already, and is told as it was.
pub const BY_SKILL_ENVIRONMENT: &str = "LAMP_TASK_NEWS";

/// Whether news waits for a skill ([`BY_SKILL_ENVIRONMENT`]).
pub fn by_skill() -> bool {
    std::env::var(BY_SKILL_ENVIRONMENT).as_deref() == Ok("skill")
}
pub const DEFAULT_TOLD: &str = "/var/lib/lampOS/voice/tasks-told";
/// How often the tasks part is read.
const READ_EVERY: Duration = Duration::from_secs(10);
/// Each request to the tasks part, connecting included.
const ASK_DEADLINE: Duration = Duration::from_millis(1_500);
/// News older than this is not told unasked: the day it belonged to is over.
const OLDEST_MS: u64 = 12 * 3_600_000;
/// Within this the work has "just" finished; after it, it finished earlier.
const JUST_MS: u64 = 10 * 60_000;
/// The most of an agent's answer put before the model.
pub(crate) const MOST_ANSWER_CHARS: usize = 1_200;
/// The most of the task's own words.
pub(crate) const MOST_TASK_CHARS: usize = 300;
/// When a note is too long, the task's words are cut to this many first:
/// the person knows their task from a few words, and the gist is in the
/// answer. With a task of 300 characters the note kept 163 bytes of a
/// long answer; on lamp-4ace on 2026-10-15 at 08:02 the lamp then told a
/// 300-word story as only "the rest is waiting on your computer" (#757).
const TASK_CHARS_KEPT_FIRST: usize = 100;
/// At most this many finished tasks are told in one turn
/// ([`TaskNews::tell_together`]): one sentence each, from one note.
pub const TOGETHER_MOST: usize = 3;
/// Told together, each task's words are kept to this many characters: what
/// it was, for the person to know it by.
const TOGETHER_TASK_CHARS: usize = 60;
/// Told together, each answer keeps at least this many characters, or
/// fewer tasks are told together: less is no gist. One note holds
/// `lamp_gemini::wire::MAX_NOTE_BYTES`.
const TOGETHER_ANSWER_LEAST: usize = 120;
/// Nothing said by or to the lamp for this long before news is told.
pub const QUIET_US: u64 = 15_000_000;
/// `on`: news is told at the first pause of [`PAUSE_US`] instead of after
/// [`QUIET_US`] with no turn (#560). The first builder's bound (20:49 on
/// #560): a person at the lamp who handed work to it hears the result
/// within 20 s of the agent settling, at their first pause of at least 1 s
/// if they are talking. Read at start; off unless set.
pub const AT_PAUSE_ENVIRONMENT: &str = "LAMP_TASK_NEWS_AT_PAUSE";
/// The pause news waits for with [`AT_PAUSE_ENVIRONMENT`] after someone
/// spoke: nobody heard speaking for this long, and the turn over as long,
/// as a due reminder waits (`coordinator::reminder_quiet`).
pub const PAUSE_US: u64 = 1_000_000;
/// After the lamp's own words, news waits this long with nobody starting:
/// 1 s after the lamp stops is the person's turn to answer it, not a pause
/// (someone answering begins 0.2 to 1.5 s after), and a friend with news
/// lets them have it (the first builder on #1161).
pub const AFTER_LAMP_US: u64 = 3_000_000;
/// Speech heard this soon after the lamp's own words stop is their tail,
/// not someone starting: the detector fires on the lamp's own cancelled
/// voice, and the microphone took up to 270 ms to fall back to its floor
/// after a reply (`duplex-listening.md`). The duplex mask's wait before it
/// hears anyone in the clear (`Hearing::clear_after_blocks`, 10 ms each).
pub const LAMP_TAIL_US: u64 =
    lamp_audio::duplex::Hearing::EVALUATED.clear_after_blocks as u64 * 10_000;

/// Whether news is told at the first pause ([`AT_PAUSE_ENVIRONMENT`]):
/// `on`, and nothing else.
pub fn at_pause(value: Option<&str>) -> bool {
    value == Some("on")
}

/// Whether it is quiet enough now (`now_us`) to tell news: [`QUIET_US`]
/// since the last turn, by or to the lamp (`last_turn_us`); or, `at_pause`,
/// the first pause: [`PAUSE_US`] since the turn, with nobody heard speaking
/// for as long (`speech_paused`, as the coordinator judges it for a
/// reminder), and, unless anyone's speech (`last_speech_us`) was heard past
/// the tail of the lamp's own words (`lamp_spoke_us`, [`LAMP_TAIL_US`]),
/// [`AFTER_LAMP_US`] since them.
pub fn quiet_for_news(
    at_pause: bool,
    now_us: u64,
    last_turn_us: u64,
    lamp_spoke_us: Option<u64>,
    last_speech_us: u64,
    speech_paused: bool,
) -> bool {
    let since_turn = now_us.saturating_sub(last_turn_us);
    if !at_pause {
        return since_turn >= QUIET_US;
    }
    let after_the_lamp = lamp_spoke_us.is_none_or(|spoke| {
        spoke.saturating_add(LAMP_TAIL_US) < last_speech_us
            || now_us.saturating_sub(spoke) >= AFTER_LAMP_US
    });
    speech_paused && since_turn >= PAUSE_US && after_the_lamp
}
/// Without eyes that say someone is there, something said this recently
/// says it.
pub const HEARD_WITHIN_US: u64 = 120_000_000;
/// Progress news is told only with this set to `on`, read at start (#752).
/// Off unless set: without it no decision is subscribed to and no progress
/// is asked for or kept. `tasks/status` is listened to either way, only for
/// a task newly settled (#560).
pub const PROGRESS_ENVIRONMENT: &str = "LAMP_PROGRESS_NEWS";
/// A decision to say progress is let go this long after it was made: told
/// later, it would be old news.
pub const PROGRESS_FRESH_MS: u64 = 60_000;
/// The most of the agent's progress line put before the model.
pub(crate) const MOST_ABOUT_CHARS: usize = 400;
/// How long the progress reader waits for a decision before it looks again
/// whether it is to stop.
const PROGRESS_WAIT: Duration = Duration::from_millis(200);
/// How long the settled watcher waits for a status before it looks again
/// whether it is to stop.
const SETTLED_WAIT: Duration = Duration::from_millis(200);
/// The least time between two reads a settled task wakes: a settled list
/// that keeps changing never has the tasks part read back to back (the
/// first builder on #1156).
const WOKEN_GAP: Duration = Duration::from_secs(1);

/// What wakes the reading thread before its next read.
#[derive(Debug, PartialEq, Eq)]
enum Wake {
    /// News just told: kept in the file at once.
    Told(String),
    /// A task newly settled on `tasks/status` (#560): read now, not at the
    /// next of [`READ_EVERY`].
    Settled,
}

/// One piece of news, ready to tell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct News {
    /// The tasks part's store, task and observation (or `hand-over`, or
    /// `progress-<run>-<change>`): what is told once.
    pub key: String,
    /// The note the model is given.
    pub note: String,
    /// Let go if not told by then: progress is told fresh or not at all.
    pub until: Option<Instant>,
    /// For progress, which change of which task it tells: withdrawn once
    /// `tasks/status` no longer shows that change of that task at work.
    pub progress: Option<ProgressOf>,
    /// For a task finished with an answer, what its note is made of, so
    /// that several can be told in one turn.
    pub finished: Option<Finished>,
    /// The keys of the news told with this, in the same turn.
    pub also: Vec<String>,
}

impl News {
    /// The task it is news of, from its key (`<store>:<task>:<what>`): a
    /// skill's ask for that task takes it (`say::Says::take_from`).
    pub fn task(&self) -> Option<u64> {
        let mut fields = self.key.rsplitn(3, ':');
        fields.next()?;
        let task = fields.next()?;
        fields.next()?;
        task.parse().ok()
    }
}

/// What a finished task's news is made of.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finished {
    who: &'static str,
    asked: String,
    answer: String,
    /// The answer was cut before it came here.
    cut: bool,
    /// Within [`JUST_MS`]: "just now", not "a while ago".
    just: bool,
}

/// The piece of progress a note tells: a change is named by its count and
/// the run of the tasks part that counted it, since counts begin again at
/// each run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgressOf {
    pub store: String,
    pub task: u64,
    pub change: u32,
    pub run: u64,
}

/// Whether `of` is still the newest change of a task whose agent works,
/// as the newest `tasks/status` says: what a progress note may still say.
fn still_current(status: Option<&TaskStatus>, of: &ProgressOf) -> bool {
    status.is_some_and(|status| {
        status.store_id == of.store
            && status.run == of.run
            && status.tasks.iter().any(|entry| {
                entry.task == of.task
                    && entry.state == AgentState::Working
                    && entry.changes == of.change
            })
    })
}

pub struct TaskNews {
    offered: Receiver<News>,
    told: Sender<Wake>,
    waiting: VecDeque<News>,
    /// What the file says was told, as the reading thread last found it:
    /// news the person asked for and heard ([`crate::agent_tasks`]) is not
    /// told again.
    told_already: Arc<Mutex<BTreeSet<String>>>,
    /// Set when this is let go, so that the progress reader and the
    /// settled watcher end.
    stopping: Arc<AtomicBool>,
    /// The newest `tasks/status` the progress reader has: a queued note is
    /// checked against it, never waited on.
    status: Arc<Mutex<Option<TaskStatus>>>,
    /// News heard whole, for the tasks part to be told ([`Self::heard`]).
    heard: Sender<String>,
}

impl Drop for TaskNews {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::Relaxed);
    }
}

/// Whether progress news is on: `on`, and nothing else.
pub fn progress_on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// The file of what was told.
pub fn told_file() -> PathBuf {
    std::env::var_os(TOLD_ENVIRONMENT)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| PathBuf::from(DEFAULT_TOLD))
}

/// Keep `key` as told.
pub fn keep_told(file: &Path, key: &str) {
    let kept = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
        .and_then(|mut file| writeln!(file, "{key}"));
    if let Err(error) = kept {
        eprintln!(
            "lamp-voice: task news told, not kept in {}: {error}",
            file.display()
        );
    }
}

/// The store, task and observation that the news `key` tells
/// (`<store>:<task>:<observation>`); none for a hand-over or progress.
fn observation_of(key: &str) -> Option<(&str, u64, u64)> {
    let mut fields = key.rsplitn(3, ':');
    let observation = fields.next()?.parse().ok()?;
    let task = fields.next()?.parse().ok()?;
    Some((fields.next()?, task, observation))
}

/// What became of one `told` for the tasks part.
#[derive(Debug, PartialEq, Eq)]
enum Sent {
    /// Taken, taken before, or of nothing the tasks part still keeps: never
    /// sent again.
    Done,
    /// It couldn't be reached, or stayed busy: sent again at the next pass.
    Again,
}

/// How many times [`send_told`] reads the store and sends again in one pass,
/// while the tasks part changed in between or was busy.
const TOLD_TRIES: usize = 3;
/// How many passes, [`READ_EVERY`] apart, a `told` the tasks part couldn't
/// take is kept for: an hour, as through a restart of the tasks part.
const TOLD_PASSES: u32 = 360;

/// Tell the tasks part in `directory`, on its trusted socket, that the news
/// `key` of a task's observation was heard (#1390): with
/// `lamp-tasks serve --close-told` the harness the lamp started for it is
/// then closed. The model says the news in its own words, so no text is
/// claimed, as `claim_speech` would. News of another store than the one
/// kept now is of a task gone.
fn send_told(directory: &Path, key: &str) -> Sent {
    let Some((store, id, observation_id)) = observation_of(key) else {
        return Sent::Done;
    };
    for _ in 0..TOLD_TRIES {
        let Some(status) = ask(&directory.join("request.sock"), &json!({"op":"status"})) else {
            return Sent::Again;
        };
        if status["store_id"] != json!(store) {
            return Sent::Done;
        }
        let (Some(revision), Some(session)) = (
            status["revision"].as_u64(),
            status["summary"]["session_id"].as_str(),
        ) else {
            return Sent::Again;
        };
        let line = json!({"op":"told","guard":{"store_id":store,"revision":revision,
            "request_id":format!("voice-told-{id}-{observation_id}-{revision}")},
            "id":id,"observation_id":observation_id,"session_id":session});
        let Some(answer) = crate::tools::exchange_up_to(
            &directory.join("voice.sock"),
            &line,
            ASK_DEADLINE,
            crate::tools::MAX_TASKS_ANSWER_BYTES,
        ) else {
            return Sent::Again;
        };
        if answer["ok"] == json!(true) || answer["error"] == json!("already_told") {
            return Sent::Done;
        }
        // Changed since the read, not sure it was kept, or the tasks part
        // restarted in between (a new session) or was not taking requests:
        // read again. One kept meanwhile is then `already_told`.
        let code = answer["error"].as_str().unwrap_or_default();
        if ![
            "conflict",
            "uncertain",
            "busy",
            "timeout",
            "stale_session",
            "unavailable",
        ]
        .contains(&code)
        {
            eprintln!("lamp-voice: the tasks part did not take the news heard ({key}): {code}");
            return Sent::Done;
        }
    }
    Sent::Again
}

/// Tell the tasks part in `directory` of each piece of news heard as it
/// comes from `heard`, and again every [`READ_EVERY`] while it can't take
/// one, for [`TOLD_PASSES`] passes. Ends when the news is let go.
fn send_heard(directory: &Path, heard: &Receiver<String>, stopping: &AtomicBool) {
    let mut unsent: Vec<(String, u32)> = Vec::new();
    loop {
        match heard.recv_timeout(READ_EVERY) {
            Ok(key) => {
                if observation_of(&key).is_some() && !unsent.iter().any(|(kept, _)| *kept == key) {
                    unsent.push((key, 0));
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        if stopping.load(Ordering::Relaxed) {
            return;
        }
        unsent.retain_mut(|(key, passes)| {
            if send_told(directory, key) == Sent::Done {
                return false;
            }
            *passes += 1;
            if *passes < TOLD_PASSES {
                return true;
            }
            eprintln!("lamp-voice: the news heard ({key}) could not be told to the tasks part");
            false
        });
    }
}

/// Whether `key` is in the file of what was told: news told unasked, or
/// heard when the person asked.
pub(crate) fn was_told(file: &Path, key: &str) -> bool {
    read_told(file).contains(key)
}

fn read_told(file: &Path) -> BTreeSet<String> {
    std::fs::read_to_string(file)
        .map(|text| text.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

impl TaskNews {
    /// Reading starts on a thread of its own when the tasks part is set,
    /// woken by a task newly settled on the lamp's bus (#560); progress
    /// too, with [`PROGRESS_ENVIRONMENT`] `on`.
    pub fn from_environment() -> Option<Self> {
        let directory = std::env::var_os(crate::agent_tasks::ENVIRONMENT)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())?;
        let progress = progress_on(std::env::var(PROGRESS_ENVIRONMENT).ok().as_deref());
        let bus = match Bus::from_environment() {
            Ok(bus) => Some(bus),
            Err(error) => {
                eprintln!(
                    "lamp-voice: task news cannot use the bus, so it reads every {} s{}: {error}",
                    READ_EVERY.as_secs(),
                    if progress {
                        " and tells no progress"
                    } else {
                        ""
                    }
                );
                None
            }
        };
        Self::start(&directory, told_file(), bus.as_ref(), progress)
    }

    /// Read the tasks part in `directory` and keep what is told in
    /// `told_file`. With a `bus`, a task newly settled on `tasks/status`
    /// has it read at once (#560); with `progress` as well, progress
    /// decisions are taken from it too. Without a bus nothing is
    /// subscribed to, it is read every [`READ_EVERY`], and no progress is
    /// asked for.
    pub fn start(
        directory: &Path,
        told_file: PathBuf,
        bus: Option<&Bus>,
        progress: bool,
    ) -> Option<Self> {
        Self::start_with_clock(directory, told_file, bus, progress, boot_clock)
    }

    /// [`Self::start`], with the tasks part's clock read from `clock`: a
    /// test's own, where there is no `/proc` (the first builder on #1156).
    pub fn start_with_clock(
        directory: &Path,
        told_file: PathBuf,
        bus: Option<&Bus>,
        progress: bool,
        clock: fn() -> Option<BootClock>,
    ) -> Option<Self> {
        let directory = directory.to_path_buf();
        let (offer, offered) = mpsc::channel();
        let (told, heard_told) = mpsc::channel();
        let told_already = Arc::new(Mutex::new(BTreeSet::new()));
        let stopping = Arc::new(AtomicBool::new(false));
        let status = Arc::new(Mutex::new(None));
        if let Some(bus) = bus {
            // Subscribed before this returns, as progress is: the kept
            // status comes first, and a task settled after is not missed.
            match bus.subscribe_typed(TaskStatus::TOPIC, "voice", TaskStatus::TYPE) {
                Ok(statuses) => {
                    let (wake, stopping) = (told.clone(), stopping.clone());
                    // Without the watcher news is still read every
                    // `READ_EVERY` (the first builder on #1156).
                    if let Err(error) = std::thread::Builder::new()
                        .name("lamp-voice-task-settled".into())
                        .spawn(move || wake_on_settled(statuses, &wake, &stopping))
                    {
                        eprintln!(
                            "lamp-voice: task news cannot watch {}, so it reads every {} s: {error}",
                            TaskStatus::TOPIC,
                            READ_EVERY.as_secs()
                        );
                    }
                }
                Err(error) => eprintln!(
                    "lamp-voice: task news cannot subscribe to {}, so it reads every {} s: {error}",
                    TaskStatus::TOPIC,
                    READ_EVERY.as_secs()
                ),
            }
        }
        if let Some(bus) = bus.filter(|_| progress) {
            // Subscribed before this returns: nothing published after it is
            // missed, and `tasks/status`'s kept value comes first.
            let made = bus
                .subscribe_typed(Decision::TOPIC, "voice", Decision::TYPE)
                .and_then(|decisions| {
                    Ok((
                        decisions,
                        bus.subscribe_typed(TaskStatus::TOPIC, "voice", TaskStatus::TYPE)?,
                    ))
                });
            match made {
                Ok((decisions, statuses)) => {
                    let reader = ProgressReader {
                        socket: directory.join("request.sock"),
                        offer: offer.clone(),
                        told: told_already.clone(),
                        stopping: stopping.clone(),
                        status: status.clone(),
                    };
                    std::thread::Builder::new()
                        .name("lamp-voice-progress-news".into())
                        .spawn(move || reader.run(decisions, statuses))
                        .ok()?;
                }
                Err(error) => {
                    eprintln!("lamp-voice: progress news cannot subscribe: {error}");
                }
            }
        }
        // News heard whole is told to the tasks part on a thread of its own
        // (#1390), so that neither the coordinator nor the reading of news
        // waits on it.
        let (heard, to_send) = mpsc::channel();
        {
            let (directory, stopping) = (directory.clone(), stopping.clone());
            std::thread::Builder::new()
                .name("lamp-voice-task-heard".into())
                .spawn(move || send_heard(&directory, &to_send, &stopping))
                .ok()?;
        }
        let shared = told_already.clone();
        std::thread::Builder::new()
            .name("lamp-voice-task-news".into())
            .spawn(move || {
                read_forever(&directory, &told_file, &offer, &heard_told, &shared, &clock)
            })
            .ok()?;
        Some(Self {
            offered,
            told,
            waiting: VecDeque::new(),
            told_already,
            stopping,
            status,
            heard,
        })
    }

    /// News told in a turn that was heard whole, on the proof a say's
    /// `spoken` takes (`coordinator`): the tasks part is told so, so that
    /// with `--close-told` the harness its task started is closed (#1390).
    /// News taken to tell but cut short, or not heard, never comes here.
    /// Never waits.
    pub fn heard(&self, keys: Vec<String>) {
        for key in keys {
            let _ = self.heard.send(key);
        }
    }

    /// The news waiting to be told, if any. Call every tick: it never blocks.
    pub fn waiting(&mut self) -> Option<&News> {
        while let Ok(news) = self.offered.try_recv() {
            if !self.waiting.iter().any(|waiting| waiting.key == news.key) {
                self.waiting.push_back(news);
            }
        }
        if let Ok(told) = self.told_already.try_lock() {
            self.waiting.retain(|news| !told.contains(&news.key));
        }
        // Progress not told in time is old news, and progress whose task
        // has finished, gone or moved on would say what is no longer so:
        // let go. The status is looked at only if it is free this tick.
        let now = Instant::now();
        self.waiting
            .retain(|news| news.until.is_none_or(|until| now < until));
        if self.waiting.iter().any(|news| news.progress.is_some())
            && let Ok(status) = self.status.try_lock()
        {
            self.waiting.retain(|news| {
                news.progress
                    .as_ref()
                    .is_none_or(|of| still_current(status.as_ref(), of))
            });
        }
        self.waiting.front()
    }

    /// Take the news waiting, as it is being told: it is not offered again.
    /// Progress is looked at once more first: one past its time, or whose
    /// task finished, went or moved on while the lamp waited for its moment
    /// (`coordinator`: quiet, someone there, a provider ready), is let go
    /// and nothing is told this tick; one whose status is being written
    /// just now is kept for the next tick. Never waits.
    pub fn tell(&mut self) -> Option<News> {
        let front = self.waiting.front()?;
        let past = front.until.is_some_and(|until| Instant::now() >= until);
        let moved_on = match (&front.progress, past) {
            (Some(of), false) => match self.status.try_lock() {
                Ok(status) => !still_current(status.as_ref(), of),
                Err(_) => return None,
            },
            _ => false,
        };
        let news = self.waiting.pop_front()?;
        if past || moved_on {
            return None;
        }
        let _ = self.told.send(Wake::Told(news.key.clone()));
        Some(news)
    }

    /// [`Self::tell`], with other tasks that finished told in the same
    /// turn, at most [`TOGETHER_MOST`] in all (`LAMP_TASK_NEWS_AT_PAUSE=on`,
    /// the reviewer on #1161). A person would say them together. Told one
    /// by one, each 3 s after the lamp's words before it, three were told
    /// at about 1, 9 and 17 s, each "Oh, Claude Code just finished…", and a
    /// fourth missed the 20 s bound. Only finished tasks with an answer go
    /// together; one waiting for the person, one that stopped, a hand-over
    /// that failed and progress are told on their own. Fewer go together
    /// when one note would keep too little of each answer
    /// ([`TOGETHER_ANSWER_LEAST`]). The coordinator does not tell
    /// together while news waits for a skill ([`by_skill`]), which tells
    /// each task in its own run (#1042).
    pub fn tell_together(&mut self) -> Option<News> {
        let mut news = self.tell()?;
        let Some(first) = news.finished.clone() else {
            return Some(news);
        };
        // Where the finished ones wait in line. How many go together is
        // settled before any is taken out, so what is left keeps its place:
        // put back at the front, a task left out went ahead of one waiting
        // for the person that came before it (the reviewer on #1223).
        let mut places: Vec<usize> = self
            .waiting
            .iter()
            .enumerate()
            .filter(|(_, waiting)| waiting.finished.is_some())
            .map(|(place, _)| place)
            .take(TOGETHER_MOST - 1)
            .collect();
        while !places.is_empty() {
            let note = {
                let parts: Vec<&Finished> = std::iter::once(&first)
                    .chain(
                        places
                            .iter()
                            .filter_map(|place| self.waiting[*place].finished.as_ref()),
                    )
                    .collect();
                note_together(&parts)
            };
            if let Some(note) = note {
                // From the back, so that the places before stay right.
                let mut others: Vec<News> = places
                    .iter()
                    .rev()
                    .filter_map(|place| self.waiting.remove(*place))
                    .collect();
                others.reverse();
                for other in &others {
                    let _ = self.told.send(Wake::Told(other.key.clone()));
                }
                news.note = note;
                news.also = others.into_iter().map(|other| other.key).collect();
                return Some(news);
            }
            places.pop();
        }
        Some(news)
    }
}

/// Read the tasks part every [`READ_EVERY`], and at once when a task newly
/// settled is seen on `tasks/status` ([`Wake::Settled`], #560), with the
/// tasks part's own clock (`clock`).
fn read_forever(
    directory: &Path,
    told_file: &Path,
    offer: &Sender<News>,
    told: &Receiver<Wake>,
    shared: &Mutex<BTreeSet<String>>,
    clock: &dyn Fn() -> Option<BootClock>,
) {
    let socket = directory.join("request.sock");
    let mut offered = BTreeSet::new();
    loop {
        // A settled task's wake waiting here is answered by the read just
        // below.
        while let Ok(wake) = told.try_recv() {
            if let Wake::Told(key) = wake {
                keep_told(told_file, &key);
            }
        }
        // Read again each time: the provider worker keeps there what the
        // person asked for and heard.
        let told_keys = read_told(told_file);
        if let Ok(mut shared) = shared.lock() {
            shared.clone_from(&told_keys);
        }
        if let Some(now) = clock() {
            for news in read_once(&socket, &now, &told_keys, &offered) {
                offered.insert(news.key.clone());
                if offer.send(news).is_err() {
                    return;
                }
            }
        }
        // Wait for the next read, keeping what is told the moment it is
        // told: on lamp-4ace on 2026-10-15 a build whose provider ended
        // just after news was told told the same news three times, each
        // time before this thread's next pass had kept it. A task newly
        // settled ends the wait (#560), including one seen while the read
        // above was under way, though never sooner than `WOKEN_GAP` after
        // that read.
        let read_at = std::time::Instant::now();
        let mut next = read_at + READ_EVERY;
        loop {
            let left = next.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                break;
            }
            match told.recv_timeout(left) {
                Ok(Wake::Told(key)) => keep_told(told_file, &key),
                Ok(Wake::Settled) => next = next.min(read_at + WOKEN_GAP),
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
    }
}

/// Whether `status` lists a task settled that `seen` does not have: its
/// store, task and when it settled (#560). `seen` becomes what `status`
/// lists, so it never holds more than one status does. A task stays listed
/// for the tasks part's `SETTLED_FOR_MS` on every status published, so it
/// wakes the reader once; one gone from a status and listed again wakes
/// it again, which costs only a read.
fn newly_settled(status: &TaskStatus, seen: &mut BTreeSet<(String, u64, u64)>) -> bool {
    let listed: BTreeSet<_> = status
        .settled
        .iter()
        .map(|settled| (status.store_id.clone(), settled.task, settled.at_unix_ms))
        .collect();
    let new = !listed.is_subset(seen);
    *seen = listed;
    new
}

/// Wake the reader (`wake`) each time `tasks/status` lists a task newly
/// settled (#560): the tasks part publishes one the moment it keeps it, so
/// the news is read then and not up to [`READ_EVERY`] later. Ends when
/// `stopping` is set or the reader has gone.
fn wake_on_settled(mut statuses: Subscriber, wake: &Sender<Wake>, stopping: &AtomicBool) {
    let mut seen = BTreeSet::new();
    while !stopping.load(Ordering::Relaxed) {
        match statuses.recv(SETTLED_WAIT) {
            Ok(Some(message)) => {
                if let Ok(status) = message.parse::<TaskStatus>()
                    && status.check().is_ok()
                    && newly_settled(&status, &mut seen)
                    && wake.send(Wake::Settled).is_err()
                {
                    return;
                }
            }
            Ok(None) => {}
            Err(_) => std::thread::sleep(SETTLED_WAIT),
        }
    }
}

/// The boot and its clock as the tasks part keeps them (`CLOCK_BOOTTIME`,
/// which `/proc/uptime` reads).
#[derive(Clone, Debug)]
pub struct BootClock {
    pub boot_id: String,
    pub ms: u64,
}

fn boot_clock() -> Option<BootClock> {
    let boot_id = std::fs::read_to_string("/proc/sys/kernel/random/boot_id").ok()?;
    let uptime = std::fs::read_to_string("/proc/uptime").ok()?;
    let seconds: f64 = uptime.split_whitespace().next()?.parse().ok()?;
    Some(BootClock {
        boot_id: boot_id.trim().to_owned(),
        ms: (seconds * 1_000.0) as u64,
    })
}

fn ask(socket: &Path, line: &Value) -> Option<Value> {
    crate::tools::exchange_up_to(
        socket,
        line,
        ASK_DEADLINE,
        crate::tools::MAX_TASKS_ANSWER_BYTES,
    )
    .filter(|answer| answer["ok"] == json!(true))
}

/// Every piece of news there is now that has not been told or offered.
fn read_once(
    socket: &Path,
    now: &BootClock,
    told: &BTreeSet<String>,
    offered: &BTreeSet<String>,
) -> Vec<News> {
    let mut news = Vec::new();
    let mut after = 0;
    loop {
        let Some(page) = ask(socket, &json!({"op":"list","after_id":after})) else {
            return news;
        };
        let store = page["store_id"].as_str().unwrap_or_default().to_owned();
        for item in page["items"].as_array().into_iter().flatten() {
            let Some(id) = item["id"].as_u64() else {
                continue;
            };
            // Work the person said yes to that never reached its agent is
            // news too: the lamp said it was handing it over.
            if let Some(failed) = failed_hand_over(item, now) {
                let key = format!("{store}:{id}:hand-over");
                if !told.contains(&key)
                    && !offered.contains(&key)
                    && let Some(task) = ask(socket, &json!({"op":"get_task","id":id}))
                    && let Some(note) = note_failed(&task, failed)
                {
                    news.push(News {
                        key,
                        note,
                        until: None,
                        progress: None,
                        finished: None,
                        also: Vec::new(),
                    });
                }
                continue;
            }
            if item["observations"].as_u64().unwrap_or(0) == 0 {
                continue;
            }
            let Some(task) = ask(socket, &json!({"op":"get_task","id":id})) else {
                continue;
            };
            let Some((observation_id, age_ms)) = newest_settled(&task, now) else {
                continue;
            };
            let key = format!("{store}:{id}:{observation_id}");
            if told.contains(&key) || offered.contains(&key) || age_ms > OLDEST_MS {
                continue;
            }
            let Some(observation) = ask(
                socket,
                &json!({"op":"get_observation","id":id,"observation_id":observation_id}),
            ) else {
                continue;
            };
            let mut data = observation["data"].clone();
            // The watch follows an idle answer and keeps the end the agent
            // went on to; Harness gives that answer with the earlier one at
            // its front (lamp-75c9, 2026-10-16, "…once it finishes.done"),
            // which was told already: only what came after is news.
            if let Some(earlier) = earlier_idle(&task, observation_id).and_then(|earlier_id| {
                ask(
                    socket,
                    &json!({"op":"get_observation","id":id,"observation_id":earlier_id}),
                )
            }) && let Some(rest) = went_on(
                earlier["data"]["answer"].as_str().unwrap_or_default(),
                data["answer"].as_str().unwrap_or_default(),
            ) {
                data["answer"] = json!(rest);
            }
            if let Some(note) = note(&task, &data, age_ms) {
                news.push(News {
                    key,
                    note,
                    until: None,
                    progress: None,
                    finished: finished(&task, &data, age_ms),
                    also: Vec::new(),
                });
            }
        }
        match page["next_after_id"].as_u64() {
            Some(next) if next > after => after = next,
            _ => return news,
        }
    }
}

/// The newest observation before `observation_id` in which the task's agent
/// had finished with an answer: what a later answer may begin with.
fn earlier_idle(task: &Value, observation_id: u64) -> Option<u64> {
    task["observations"]
        .as_array()?
        .iter()
        .filter(|observation| observation["state"].as_str() == Some("idle"))
        .filter_map(|observation| observation["id"].as_u64())
        .filter(|id| *id < observation_id)
        .max()
}

/// What `answer` says after `earlier`, when it begins with it: the part of
/// the agent's answer that is new. None when it does not, or adds nothing.
fn went_on<'a>(earlier: &str, answer: &'a str) -> Option<&'a str> {
    let earlier = earlier.trim();
    let rest = answer.trim_start().strip_prefix(earlier)?.trim();
    (!earlier.is_empty() && !rest.is_empty()).then_some(rest)
}

/// The task's newest observation in which its agent had finished, stopped,
/// or stopped to ask, and how long ago it was seen: only one seen in this
/// boot can be dated.
fn newest_settled(task: &Value, now: &BootClock) -> Option<(u64, u64)> {
    task["observations"]
        .as_array()?
        .iter()
        .filter(|observation| {
            matches!(
                observation["state"].as_str(),
                Some("idle" | "needs-input" | "stopped" | "failed")
            )
        })
        .filter_map(|observation| {
            let seen = &observation["observed"];
            (seen["boot_id"].as_str() == Some(now.boot_id.as_str())).then_some(())?;
            Some((
                observation["id"].as_u64()?,
                now.ms.saturating_sub(seen["monotonic_ms"].as_u64()?),
            ))
        })
        .max_by_key(|(id, _)| *id)
}

/// Whether a task the person said yes to failed to reach its agent within
/// the last [`JUST_MS`] of this boot: `false` when it surely did not start,
/// `true` when it may have.
fn failed_hand_over(item: &Value, now: &BootClock) -> Option<bool> {
    let dispatch = &item["dispatch"];
    let unsure = match dispatch["state"].as_str()? {
        "not_sent" => false,
        "uncertain" => true,
        _ => return None,
    };
    if dispatch["confirmed"].is_null() {
        return None;
    }
    let finished = &dispatch["finished"];
    (finished["boot_id"].as_str() == Some(now.boot_id.as_str())
        && now.ms.saturating_sub(finished["monotonic_ms"].as_u64()?) <= JUST_MS)
        .then_some(unsure)
}

fn note_failed(task: &Value, unsure: bool) -> Option<String> {
    let who = who(task);
    let (asked, _) = quoted(task["text"].as_str()?, MOST_TASK_CHARS);
    let what = if unsure {
        format!("may not have reached {who}: you cannot tell whether it started")
    } else {
        format!("did not reach {who}: it could not be started just now")
    };
    Some(format!(
        "(Nobody has spoken. The work the person asked you to hand over, \"{asked}\", {what}. \
         Tell them in one plain, light sentence, with no technical words, and offer to try again.)"
    ))
}

/// Who the work went to, as the person calls them.
pub(crate) fn who(task: &Value) -> &'static str {
    match task["target"]["engine"].as_str() {
        Some("codex") => "Codex",
        Some("opencode") => "OpenCode",
        _ => "Claude Code",
    }
}

/// At most `most` characters of `text` on one line, with no double quotes
/// to end the note's quotation early.
pub(crate) fn quoted(text: &str, most: usize) -> (String, bool) {
    let flat: String = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('"', "'");
    let cut = flat.chars().count() > most;
    (flat.chars().take(most).collect(), cut)
}

/// What the model is told, for the agent's state; none when there is
/// nothing to tell.
fn note(task: &Value, data: &Value, age_ms: u64) -> Option<String> {
    let who = who(task);
    let (asked, _) = quoted(task["text"].as_str()?, MOST_TASK_CHARS);
    let (answer, cut) = quoted(
        data["answer"].as_str().unwrap_or_default(),
        MOST_ANSWER_CHARS,
    );
    let cut = cut || data["truncated"] == json!(true);
    let when = if age_ms <= JUST_MS {
        "just"
    } else {
        "a while ago, and you have not told them yet:"
    };
    let opening = "(Nobody has spoken. News from work you handed over for the person.";
    let state = match data["state"].as_str()? {
        "idle" if !answer.is_empty() => "idle",
        state @ ("needs-input" | "stopped" | "failed") => state,
        _ => return None,
    };
    let say = |asked: &str, answer: &str, more: &str| match state {
        // Cut: the gist first, from what is kept, then where the rest is.
        "idle" if !more.is_empty() => format!(
            "{opening} {who} {when} finished the task they gave it: \"{asked}\". Its answer is long; it begins: \"{answer}{more}\". \
             Tell them now in one or two natural spoken sentences, as a good chief of staff would: \"Oh, {who} just finished…\" (\"earlier\" if it was a while ago), \
             then its gist in one plain line from that beginning (a story: who it is about and what happens; code: what it does now), \
             then that the rest is on their computer. Never only that. No code, lists, file paths or links, and no question.)"
        ),
        "idle" => format!(
            "{opening} {who} {when} finished the task they gave it: \"{asked}\". What it came back with: \"{answer}{more}\". \
             Tell them now, the way a good chief of staff tells the person they work for: one or two natural spoken sentences, \
             starting with something like \"Oh, {who} just finished…\" (or \"earlier\" if it was a while ago), then the gist of the result in plain words. \
             Do not read out code, lists, file paths or links; if there is more than fits, say the rest is on their computer. No question at the end.)"
        ),
        "needs-input" if answer.is_empty() && more.is_empty() => format!(
            "{opening} {who} has stopped and is waiting for them about the task they gave it: \"{asked}\". {})",
            waiting(who, "")
        ),
        "needs-input" => format!(
            "{opening} {who} has stopped and is waiting for them about the task they gave it: \"{asked}\". What it last said: \"{answer}{more}\". {})",
            waiting(who, answer)
        ),
        _ => format!(
            "{opening} {who} stopped without finishing the task they gave it: \"{asked}\". What it last said: \"{answer}{more}\". \
             Tell them in one short, plain sentence, without alarm.)"
        ),
    };
    fitted(&asked, &answer, cut, say)
}

/// A task finished with an answer, as [`note`] reads it, for telling with
/// others; none for any other state.
fn finished(task: &Value, data: &Value, age_ms: u64) -> Option<Finished> {
    if data["state"].as_str()? != "idle" {
        return None;
    }
    let (asked, _) = quoted(task["text"].as_str()?, MOST_TASK_CHARS);
    let (answer, cut) = quoted(
        data["answer"].as_str().unwrap_or_default(),
        MOST_ANSWER_CHARS,
    );
    (!answer.is_empty()).then_some(Finished {
        who: who(task),
        asked,
        answer,
        cut: cut || data["truncated"] == json!(true),
        just: age_ms <= JUST_MS,
    })
}

/// The note for finished tasks told together: what each was and the
/// beginning of its answer, the answers cut alike to one written turn
/// (`lamp_gemini::wire::MAX_NOTE_BYTES`). None if each would keep less than
/// [`TOGETHER_ANSWER_LEAST`] of an answer that is longer.
fn note_together(parts: &[&Finished]) -> Option<String> {
    let first = parts.first()?.who;
    let how_many = match parts.len() {
        2 => "two",
        3 => "three",
        _ => return None,
    };
    let say = |answer_most: usize| {
        let mut cut = false;
        let each: Vec<String> = parts
            .iter()
            .map(|part| {
                let asked: String = part.asked.chars().take(TOGETHER_TASK_CHARS).collect();
                let answer: String = part.answer.chars().take(answer_most).collect();
                let more = if part.cut || answer.len() < part.answer.len() {
                    cut = true;
                    GOES_ON
                } else {
                    ""
                };
                let when = if part.just { "just now" } else { "a while ago" };
                format!(
                    "{} finished \"{asked}\" {when}: \"{answer}{more}\".",
                    part.who
                )
            })
            .collect();
        format!(
            "(Nobody has spoken. News from {how_many} pieces of work you handed over for the person. {} \
             Tell them now, together, in one turn: one plain spoken sentence for each, starting with something like \
             \"Oh, {first} just finished…\" (\"earlier\" for one a while ago), each with the gist of its result. \
             No code, lists, file paths or links.{} No question.)",
            each.join(" "),
            // #757's guard, as for one cut answer: once the lamp told a
            // story as only "the rest is waiting on your computer" (the
            // reviewer on #1223).
            if cut {
                " Where an answer goes on, give its gist in one plain line from that beginning, \
                 then that the rest is on their computer: never only that."
            } else {
                ""
            }
        )
    };
    let longest = parts.iter().map(|part| part.answer.chars().count()).max()?;
    let mut answer_most = longest;
    loop {
        let text = say(answer_most);
        let over = text.len().saturating_sub(lamp_gemini::wire::MAX_NOTE_BYTES);
        if over == 0 {
            return Some(text);
        }
        // A character is at least one byte: each answer at the most loses
        // its share of what is over.
        answer_most = answer_most.saturating_sub(over.div_ceil(parts.len()));
        if answer_most < TOGETHER_ANSWER_LEAST.min(longest) {
            return None;
        }
    }
}

/// What the model is told to say while the agent waits for the person
/// (`needs-input`), in both the news and `check_agent_task`. Harness
/// 0.4.6's `show` carries no question while it waits (probed 2026-10-15,
/// #770): `answer` is empty or the previous answer. So the model is never
/// asked to voice a question it was not given.
pub(crate) fn waiting(who: &str, answer: &str) -> String {
    let tell = format!(
        "Tell them once, in one short sentence, that {who} is waiting for them on their computer and that they can answer it there."
    );
    if answer.is_empty() {
        format!(
            "{tell} It did not say what it is asking: do not ask a question for it or guess one."
        )
    } else {
        format!(
            "{tell} If what it last said is a question to them, you may pass it on in your own words; otherwise do not ask a question for it or guess one."
        )
    }
}

/// A progress decision that is to be told: what it is about.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Wanted {
    store: String,
    task: u64,
    change: u32,
    /// The run of the tasks part that counted the change.
    run: u64,
    /// Seconds the agent has been at it, as `tasks/status` says.
    working_s: Option<u64>,
}

impl Wanted {
    /// What is kept as told. The run is in it: a restart of the tasks part
    /// keeps the store and the task numbers and counts changes from
    /// nothing, and its change 1 is not the last run's (#765 review,
    /// 2026-10-15).
    fn key(&self) -> String {
        format!(
            "{}:{}:progress-{}-{}",
            self.store, self.task, self.run, self.change
        )
    }
}

/// Whether `decision`, made `age_ms` ago, is progress to tell: `progress`,
/// `say_now`, acting (not shadow), at most [`PROGRESS_FRESH_MS`] old, of
/// the store and the run `tasks/status` is of, and about that task's
/// newest change while its agent still works. Its `task` is
/// `<store>:<task>:<change>@<run>`. Why not, in one word, if not.
fn wanted(
    decision: &Decision,
    age_ms: u64,
    status: Option<&TaskStatus>,
) -> Result<Wanted, &'static str> {
    if decision.about != "progress" {
        return Err("not_progress");
    }
    if decision.choice != "say_now" {
        return Err("wait");
    }
    if decision.shadow {
        return Err("shadow");
    }
    let parts: Vec<&str> = decision
        .task
        .as_deref()
        .unwrap_or_default()
        .split(':')
        .collect();
    let [store, task, change_of_run] = parts[..] else {
        return Err("no_task");
    };
    let Some((change, run)) = change_of_run.split_once('@') else {
        return Err("no_task");
    };
    let (Ok(task), Ok(change), Ok(run)) = (
        task.parse::<u64>(),
        change.parse::<u32>(),
        run.parse::<u64>(),
    ) else {
        return Err("no_task");
    };
    if age_ms > PROGRESS_FRESH_MS {
        return Err("stale");
    }
    let Some(status) = status else {
        return Err("no_status");
    };
    if status.store_id != store {
        return Err("other_store");
    }
    if status.run != run {
        return Err("other_run");
    }
    let Some(entry) = status.tasks.iter().find(|entry| entry.task == task) else {
        return Err("not_watched");
    };
    if entry.changes > change {
        return Err("older_change");
    }
    if entry.state != AgentState::Working {
        return Err("not_working");
    }
    Ok(Wanted {
        store: store.to_owned(),
        task,
        change,
        run,
        working_s: entry.working_s,
    })
}

/// A sign in the words the person would use.
fn sign_words(sign: Sign) -> &'static str {
    match sign {
        Sign::TestsPass => "the tests pass",
        Sign::BuildWorks => "the build works",
        Sign::Fixed => "something was fixed",
        Sign::Found => "something was found",
        Sign::Done => "a part was done",
        Sign::PullRequest => "a pull request was opened",
    }
}

/// What the model is told of progress: who, the task's words, the agent's
/// progress line, the signs not told yet in words and how long it has been
/// at it, cut to one written turn ([`fitted`]).
fn note_progress(
    task: &Value,
    about: &str,
    signs: &BTreeSet<Sign>,
    working_s: Option<u64>,
) -> Option<String> {
    let who = who(task);
    let (asked, _) = quoted(task["text"].as_str()?, MOST_TASK_CHARS);
    let (about, cut) = quoted(about, MOST_ABOUT_CHARS);
    if about.is_empty() {
        return None;
    }
    let signs = if signs.is_empty() {
        String::new()
    } else {
        let words: Vec<&str> = signs.iter().map(|sign| sign_words(*sign)).collect();
        format!(" Signs of a step forward in it: {}.", words.join(", "))
    };
    let time = match working_s.map(|s| (s + 30) / 60) {
        None => String::new(),
        Some(0 | 1) => " It has been at it for about a minute.".to_owned(),
        Some(minutes) => format!(" It has been at it for about {minutes} minutes."),
    };
    let say = |asked: &str, about: &str, more: &str| {
        format!(
            "(Nobody has spoken. Progress on work you handed over for the person: {who} is still working on the task they gave it: \"{asked}\". \
             Its latest progress line: \"{about}{more}\".{signs}{time} \
             Tell them how it is going in one short, natural sentence, the way a good chief of staff mentions it in passing, \
             starting with something like \"Oh, {who} has…\". Do not read the line out word for word, and no code, file paths or links. \
             No question at the end.)"
        )
    };
    fitted(&asked, &about, cut, say)
}

/// Reads progress decisions and offers each one to tell as news.
struct ProgressReader {
    socket: PathBuf,
    offer: Sender<News>,
    told: Arc<Mutex<BTreeSet<String>>>,
    stopping: Arc<AtomicBool>,
    /// Where the newest status is kept for [`TaskNews`].
    status: Arc<Mutex<Option<TaskStatus>>>,
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

impl ProgressReader {
    fn run(self, mut decisions: Subscriber, mut statuses: Subscriber) {
        let mut status: Option<TaskStatus> = None;
        // The signs already put in a note, for each store and task: a note
        // names only those not told yet.
        let mut mentioned: BTreeMap<(String, u64), BTreeSet<Sign>> = BTreeMap::new();
        while !self.stopping.load(Ordering::Relaxed) {
            let received = decisions.recv(PROGRESS_WAIT);
            // The newest status, taken after the wait: a decision is made
            // after the status it is about was published.
            while let Ok(Some(message)) = statuses.recv(Duration::ZERO) {
                if let Ok(newer) = message.parse::<TaskStatus>()
                    && newer.check().is_ok()
                {
                    // Kept for the notes already queued as well: a task that
                    // finishes or moves on withdraws them (#765 review).
                    if let Ok(mut shared) = self.status.lock() {
                        *shared = Some(newer.clone());
                    }
                    status = Some(newer);
                }
            }
            let message = match received {
                Ok(Some(message)) => message,
                Ok(None) => continue,
                Err(_) => {
                    std::thread::sleep(PROGRESS_WAIT);
                    continue;
                }
            };
            let Ok(decision) = message.parse::<Decision>() else {
                continue;
            };
            let age_ms = unix_ms().saturating_sub(message.header.at_unix_ms);
            // The one time it is let go, counted from when it was made: the
            // words fetched below take time of their own.
            let expires =
                Instant::now() + Duration::from_millis(PROGRESS_FRESH_MS.saturating_sub(age_ms));
            let wanted = match wanted(&decision, age_ms, status.as_ref()) {
                Ok(wanted) => wanted,
                Err(why) => {
                    if decision.about == "progress" && decision.choice == "say_now" {
                        eprintln!("lamp-voice: progress to say let go ({why})");
                    }
                    continue;
                }
            };
            if self
                .told
                .lock()
                .is_ok_and(|told| told.contains(&wanted.key()))
            {
                continue;
            }
            let said = mentioned
                .entry((wanted.store.clone(), wanted.task))
                .or_default();
            match self.news(&wanted, said, expires) {
                Ok(news) => {
                    eprintln!("lamp-voice: progress to say offered ({})", news.key);
                    if self.offer.send(news).is_err() {
                        return;
                    }
                }
                Err(why) => eprintln!("lamp-voice: progress to say let go ({why})"),
            }
        }
    }

    /// The words, fetched now that the lamp will say them: the progress
    /// line (`get_progress`) and the task (`get_task`), both of the store
    /// decided about, the agent still at work, and exactly the change
    /// decided about. A snapshot of anything else is let go: the task
    /// finished, or moved on, while the decision travelled. So is one that
    /// comes back at or past `expires`.
    fn news(
        &self,
        wanted: &Wanted,
        said: &mut BTreeSet<Sign>,
        expires: Instant,
    ) -> Result<News, &'static str> {
        let progress = ask(&self.socket, &json!({"op":"get_progress","id":wanted.task}))
            .ok_or("no_progress")?;
        let task =
            ask(&self.socket, &json!({"op":"get_task","id":wanted.task})).ok_or("no_task")?;
        if progress["store_id"].as_str() != Some(wanted.store.as_str())
            || task["store_id"].as_str() != Some(wanted.store.as_str())
        {
            return Err("other_store");
        }
        if Instant::now() >= expires {
            return Err("stale");
        }
        let progress = &progress["progress"];
        if progress["run"].as_u64() != Some(wanted.run) {
            return Err("other_run");
        }
        if progress["state"].as_str() != Some("working") {
            return Err("not_working");
        }
        if progress["changes"].as_u64() != Some(u64::from(wanted.change)) {
            return Err("other_change");
        }
        let seen: BTreeSet<Sign> =
            serde_json::from_value(progress["signs_seen"].clone()).unwrap_or_default();
        let new: BTreeSet<Sign> = seen.difference(said).copied().collect();
        let note = note_progress(
            &task,
            progress["about"].as_str().unwrap_or_default(),
            &new,
            wanted.working_s,
        )
        .ok_or("nothing_to_say")?;
        said.extend(new);
        Ok(News {
            key: wanted.key(),
            note,
            until: Some(expires),
            progress: Some(ProgressOf {
                store: wanted.store.clone(),
                task: wanted.task,
                change: wanted.change,
                run: wanted.run,
            }),
            finished: None,
            also: Vec::new(),
        })
    }
}

/// Said after an answer that was cut.
const GOES_ON: &str = " …(it goes on)";

/// The note `say` makes, cut to what one written turn may hold
/// (`lamp_gemini::wire::MAX_NOTE_BYTES`). The transport refuses a longer
/// one as `InvalidConfiguration`, and the provider worker ends on that
/// refusal: on lamp-4ace on 2026-10-15 the voice ended so 4 ms after
/// committing news, three times in three.
/// The task's words are cut to [`TASK_CHARS_KEPT_FIRST`] first, then the
/// agent's answer, then the rest of the task's words, by whole characters,
/// and a cut answer is said to go on. A note that fits is exactly what
/// `say` makes.
fn fitted(
    asked: &str,
    answer: &str,
    cut: bool,
    say: impl Fn(&str, &str, &str) -> String,
) -> Option<String> {
    let (asked_chars, answer_chars) = (asked.chars().count(), answer.chars().count());
    let (mut asked_kept, mut answer_kept) = (asked_chars, answer_chars);
    loop {
        let more = if cut || answer_kept < answer_chars {
            GOES_ON
        } else {
            ""
        };
        let text = say(
            &asked.chars().take(asked_kept).collect::<String>(),
            &answer.chars().take(answer_kept).collect::<String>(),
            more,
        );
        // A character is at least one byte: leaving out this many is enough
        // for what is over, or more than enough.
        let over = text.len().saturating_sub(lamp_gemini::wire::MAX_NOTE_BYTES);
        if over == 0 {
            return Some(text);
        }
        if asked_kept > TASK_CHARS_KEPT_FIRST {
            asked_kept = asked_kept.saturating_sub(over).max(TASK_CHARS_KEPT_FIRST);
        } else if answer_kept > 0 {
            answer_kept = answer_kept.saturating_sub(over);
        } else if asked_kept > 0 {
            asked_kept = asked_kept.saturating_sub(over);
        } else {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader},
        os::unix::net::UnixListener,
    };

    fn task(engine: &str, observations: Value) -> Value {
        json!({"ok":true,"text":"Suggest three \"friendly\" names for a desk lamp",
            "target":{"engine":engine,"machine_id":"c533fbfda3de5f8f1fdb6619ae2e30b5","project":"lamp-tasks"},
            "observations":observations})
    }
    fn seen(id: u64, state: &str, boot: &str, ms: u64) -> Value {
        json!({"id":id,"state":state,"observed":{"boot_id":boot,"monotonic_ms":ms}})
    }
    const NOW: u64 = 50_000_000;
    fn now() -> BootClock {
        BootClock {
            boot_id: "boot-a".into(),
            ms: NOW,
        }
    }

    #[test]
    fn the_newest_finished_look_in_this_boot_is_the_news_and_work_in_progress_is_not() {
        let observations = json!([
            seen(1, "working", "boot-a", NOW - 9_000),
            seen(2, "idle", "boot-a", NOW - 5_000),
            seen(3, "working", "boot-a", NOW - 1_000),
        ]);
        assert_eq!(
            newest_settled(&task("claude", observations), &now()),
            Some((2, 5_000))
        );
        // A look from before the lamp last started cannot be dated: not news.
        let earlier = json!([seen(1, "idle", "boot-b", 10)]);
        assert_eq!(newest_settled(&task("claude", earlier), &now()), None);
        let asking = json!([
            seen(1, "idle", "boot-a", 10),
            seen(4, "needs-input", "boot-a", NOW)
        ]);
        assert_eq!(newest_settled(&task("codex", asking), &now()), Some((4, 0)));
    }

    #[test]
    fn an_answer_that_went_on_is_told_from_where_the_earlier_one_ended() {
        let earlier = "The sleep is running in the background, and I'll reply once it finishes.";
        assert_eq!(
            went_on(
                earlier,
                "The sleep is running in the background, and I'll reply once it finishes.done"
            ),
            Some("done")
        );
        assert_eq!(
            went_on(earlier, &format!("{earlier}\n\nAll done.")),
            Some("All done.")
        );
        // Not the same beginning, the same answer, or nothing earlier: as is.
        assert_eq!(went_on(earlier, "Done: slept 90 s."), None);
        assert_eq!(went_on(earlier, earlier), None);
        assert_eq!(went_on("", "done"), None);
        // Only an earlier finished answer is looked at, the newest before.
        let observations = json!([
            seen(1, "idle", "boot-a", 10),
            seen(2, "needs-input", "boot-a", 20),
            seen(3, "idle", "boot-a", 30),
            seen(5, "idle", "boot-a", 50),
        ]);
        let task = task("claude", observations);
        assert_eq!(earlier_idle(&task, 5), Some(3));
        assert_eq!(earlier_idle(&task, 3), Some(1));
        assert_eq!(earlier_idle(&task, 1), None);
    }

    #[test]
    fn the_note_names_the_agent_quotes_safely_and_says_how_fresh_it_is() {
        let finished = json!({"state":"idle","answer":"Here are three:\n\n1. **Lumi**\n2. \"Sunny\"\n3. Archie","truncated":false});
        let note = note(&task("claude", json!([])), &finished, 60_000).unwrap();
        assert!(
            note.contains("Claude Code just finished the task"),
            "{note}"
        );
        assert!(
            note.contains("\"Suggest three 'friendly' names for a desk lamp\""),
            "{note}"
        );
        assert!(
            note.contains("1. **Lumi** 2. 'Sunny' 3. Archie\""),
            "{note}"
        );
        assert!(!note.contains("(it goes on)"));
        let later = super::note(&task("codex", json!([])), &finished, JUST_MS + 1).unwrap();
        assert!(later.contains("Codex a while ago"), "{later}");
        // Asking, and stopped.
        // Asking with words: waiting on their computer, by name; what it
        // last said may be passed on only if it is a question.
        let asking = json!({"state":"needs-input","answer":"Which folder?"});
        let waits = super::note(&task("claude", json!([])), &asking, 0).unwrap();
        assert!(
            waits.contains("Claude Code is waiting for them on their computer")
                && waits.contains("What it last said: \"Which folder?\"")
                && waits.contains("you may pass it on")
                && !waits.contains("ask its question"),
            "{waits}"
        );
        let stopped = json!({"state":"stopped","answer":"I could not reach the repository.","truncated":true});
        let told = super::note(&task("claude", json!([])), &stopped, 0).unwrap();
        assert!(told.contains("stopped without finishing") && told.contains("(it goes on)"));
        // An idle agent with nothing to say, or one at work: nothing to tell.
        assert_eq!(
            super::note(
                &task("claude", json!([])),
                &json!({"state":"idle","answer":" "}),
                0
            ),
            None
        );
        assert_eq!(
            super::note(
                &task("claude", json!([])),
                &json!({"state":"working","answer":"x"}),
                0
            ),
            None
        );
        // A long answer is cut and says so.
        let long = json!({"state":"idle","answer":"word ".repeat(400)});
        let long = super::note(&task("claude", json!([])), &long, 0).unwrap();
        assert!(
            long.contains("(it goes on)") && long.len() <= lamp_gemini::wire::MAX_NOTE_BYTES,
            "{}",
            long.len()
        );
    }

    /// Every note is one written turn, which the transport takes only up
    /// to `MAX_NOTE_BYTES`; a longer one is refused as
    /// `InvalidConfiguration`, which ends the provider worker, as the voice
    /// ended 4 ms after committing news on lamp-4ace on 2026-10-15. Long answers
    /// and long tasks, in letters of one byte or of several, fit and are
    /// taken by the transport; a note that fits is as it was.
    #[test]
    fn every_note_fits_in_one_written_turn() {
        let english = "Here is what I found and changed, file by file. ".repeat(40);
        let vietnamese = "Đây là những gì tôi đã tìm thấy và sửa, từng tệp một. ".repeat(40);
        // What the transport does with a note one byte over.
        let over = "x".repeat(lamp_gemini::wire::MAX_NOTE_BYTES + 1);
        assert!(matches!(
            lamp_gemini::wire::encode_written_turn(&over, None),
            Err(lamp_gemini::Error::InvalidConfiguration)
        ));
        let asks = [
            "Suggest three names for a desk lamp".to_owned(),
            "Đề xuất ba cái tên thân thiện cho một chiếc đèn bàn. ".repeat(10),
        ];
        for asked in &asks {
            for answer in [&english, &vietnamese] {
                for state in ["idle", "needs-input", "stopped"] {
                    for age_ms in [0, JUST_MS + 1] {
                        let task = json!({"text": asked, "target": {"engine": "claude"}});
                        let data = json!({"state": state, "answer": answer});
                        let told = note(&task, &data, age_ms).unwrap();
                        assert!(
                            told.len() <= lamp_gemini::wire::MAX_NOTE_BYTES,
                            "{state}: {} bytes",
                            told.len()
                        );
                        lamp_gemini::wire::encode_written_turn(&told, None).unwrap();
                        assert!(told.contains("(it goes on)"), "{told}");
                        assert!(told.ends_with(".)"), "{told}");
                    }
                }
            }
        }
        // As on lamp-4ace: a task of about a hundred characters and an
        // answer of six hundred, well within what is taken of each. The
        // note of both whole is over the bound: the answer is cut to fit.
        let asked = "Ask Claude Code on my office computer to look at why the build is slow and tell me what it finds";
        let answer =
            "The build spends most of its time compiling the same dependencies again. ".repeat(8);
        assert!(answer.chars().count() < MOST_ANSWER_CHARS);
        let told = note(
            &json!({"text": asked, "target": {"engine": "claude"}}),
            &json!({"state": "idle", "answer": answer}),
            0,
        )
        .unwrap();
        lamp_gemini::wire::encode_written_turn(&told, None).unwrap();
        assert!(told.contains("(it goes on)"), "{told}");
        assert!(told.contains("\"Ask Claude Code on my office computer to look at why the build is slow and tell me what it finds\""));
        // A note that fits is exactly as before.
        let finished = json!({"state":"idle","answer":"Lumi, Sunny and Archie.","truncated":false});
        assert_eq!(
            note(&task("claude", json!([])), &finished, 0).unwrap(),
            "(Nobody has spoken. News from work you handed over for the person. Claude Code just finished the task they gave it: \
             \"Suggest three 'friendly' names for a desk lamp\". What it came back with: \"Lumi, Sunny and Archie.\". \
             Tell them now, the way a good chief of staff tells the person they work for: one or two natural spoken sentences, \
             starting with something like \"Oh, Claude Code just finished…\" (or \"earlier\" if it was a while ago), then the gist of the result in plain words. \
             Do not read out code, lists, file paths or links; if there is more than fits, say the rest is on their computer. No question at the end.)"
        );
    }

    /// Harness 0.4.6 in auto mode, waiting on a question (#770): `show`
    /// says `needs-input` with no question text. The person is told the
    /// agent, by name, is waiting for them on their computer, and the
    /// model is never asked to voice a question it was not given.
    #[test]
    fn an_agent_waiting_with_no_question_is_told_as_waiting_on_their_computer() {
        let waiting = json!({"state":"needs-input","answer":"","truncated":false});
        for (engine, who) in [
            ("claude", "Claude Code"),
            ("codex", "Codex"),
            ("opencode", "OpenCode"),
        ] {
            let told = note(&task(engine, json!([])), &waiting, 0).unwrap();
            assert_eq!(
                told,
                format!(
                    "(Nobody has spoken. News from work you handed over for the person. {who} has stopped and is waiting for them about the task they gave it: \
                     \"Suggest three 'friendly' names for a desk lamp\". \
                     Tell them once, in one short sentence, that {who} is waiting for them on their computer and that they can answer it there. \
                     It did not say what it is asking: do not ask a question for it or guess one.)"
                )
            );
            assert!(!told.contains("What it last said") && !told.contains("ask its question"));
        }
    }

    /// The other states' notes are byte for byte what they were before
    /// #770 (idle is pinned in `every_note_fits_in_one_written_turn`).
    #[test]
    fn a_stop_or_a_failure_is_told_as_before() {
        for state in ["stopped", "failed"] {
            let data = json!({"state":state,"answer":"I could not reach the repository.","truncated":false});
            assert_eq!(
                note(&task("claude", json!([])), &data, 0).unwrap(),
                "(Nobody has spoken. News from work you handed over for the person. Claude Code stopped without finishing the task they gave it: \
                 \"Suggest three 'friendly' names for a desk lamp\". What it last said: \"I could not reach the repository.\". \
                 Tell them in one short, plain sentence, without alarm.)"
            );
        }
    }

    /// On lamp-4ace on 2026-10-15 at 08:02 a 300-word story was told as only
    /// "the rest is waiting on your computer" (#757): the note kept 163 bytes
    /// of it after a long task, and asked for the gist only in passing. A
    /// cut answer now keeps the task's words to a hundred characters first,
    /// keeps more of the answer, and asks for the gist in one line before
    /// where the rest is.
    #[test]
    fn a_long_result_keeps_enough_for_a_gist_and_asks_for_it_first() {
        let story =
            "Every night, when the office emptied, the desk lamp hummed a little tune to itself. "
                .repeat(20);
        let long_task =
            "Write me a short story of about three hundred words about a desk lamp that \
                         learns to sing at night when everyone has gone home, with a happy ending, \
                         something gentle I could read to my daughter before bed, and keep the \
                         words simple enough for a six year old to follow along"
                .to_owned();
        assert!(long_task.chars().count() > 250);
        for asked in [long_task.as_str(), "Write a story about a lamp"] {
            for age_ms in [0, JUST_MS + 1] {
                let told = note(
                    &json!({"text": asked, "target": {"engine": "claude"}}),
                    &json!({"state": "idle", "answer": story}),
                    age_ms,
                )
                .unwrap();
                lamp_gemini::wire::encode_written_turn(&told, None).unwrap();
                // The gist first, in one line, then where the rest is.
                assert!(
                    told.contains("then its gist in one plain line from that beginning")
                        && told
                            .contains("then that the rest is on their computer. Never only that."),
                    "{told}"
                );
                // At least 300 characters of the story to make it from,
                // even after a long task.
                let kept = told
                    .split("it begins: \"")
                    .nth(1)
                    .and_then(|rest| rest.split(" …(it goes on)").next())
                    .unwrap();
                assert!(
                    kept.chars().count() >= 300,
                    "{} kept: {told}",
                    kept.chars().count()
                );
                // The task's words are cut to a hundred characters, no fewer.
                let quoted_task = told
                    .split("finished the task they gave it: \"")
                    .nth(1)
                    .and_then(|rest| rest.split('"').next())
                    .unwrap();
                assert_eq!(
                    quoted_task.chars().count(),
                    asked.chars().count().min(TASK_CHARS_KEPT_FIRST),
                    "{told}"
                );
            }
        }
        // A question or a stop that is cut is as it was: the task first,
        // then the answer, by the same rule.
        let asking = note(
            &json!({"text": long_task, "target": {"engine": "codex"}}),
            &json!({"state": "needs-input", "answer": story}),
            0,
        )
        .unwrap();
        assert!(
            asking.contains("is waiting for them on their computer"),
            "{asking}"
        );
        assert!(!asking.contains("gist"), "{asking}");
    }

    #[test]
    fn each_piece_of_news_is_offered_once_and_told_news_is_kept() {
        let root = std::env::temp_dir().join(format!("task-news-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let socket = root.join("request.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        // The tasks part: two tasks, one with news, one still at work.
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                let asked: Value = serde_json::from_str(&line).unwrap();
                let answer = match (asked["op"].as_str().unwrap(), asked["id"].as_u64()) {
                    ("list", _) => json!({"ok":true,"store_id":"s1","items":[
                        {"id":1,"observations":1},{"id":2,"observations":1},{"id":3,"observations":0}]}),
                    ("get_task", Some(1)) => {
                        task("claude", json!([seen(1, "idle", "boot-a", NOW - 1_000)]))
                    }
                    ("get_task", Some(2)) => {
                        task("claude", json!([seen(2, "working", "boot-a", NOW - 1_000)]))
                    }
                    ("get_observation", Some(1)) => {
                        json!({"ok":true,"data":{"state":"idle","answer":"Lumi, Sunny and Archie.","truncated":false}})
                    }
                    _ => json!({"ok":false}),
                };
                writeln!(stream, "{answer}").unwrap();
            }
        });
        let told = BTreeSet::new();
        let mut offered = BTreeSet::new();
        let news = read_once(&socket, &now(), &told, &offered);
        assert_eq!(news.len(), 1);
        assert_eq!(news[0].key, "s1:1:1");
        assert!(news[0].note.contains("Lumi, Sunny and Archie."));
        offered.insert(news[0].key.clone());
        assert!(read_once(&socket, &now(), &told, &offered).is_empty());
        // Told before a restart: not told again.
        let told = BTreeSet::from(["s1:1:1".to_owned()]);
        assert!(read_once(&socket, &now(), &told, &BTreeSet::new()).is_empty());
        // Too old to tell unasked.
        let late = BootClock {
            boot_id: "boot-a".into(),
            ms: NOW + OLDEST_MS,
        };
        assert!(read_once(&socket, &late, &BTreeSet::new(), &BTreeSet::new()).is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn work_said_yes_to_that_never_reached_its_agent_is_told_soon_after() {
        let item = |state: &str, confirmed: bool, boot: &str, ms: u64| {
            json!({"id":3,"observations":0,"dispatch":{"state":state,
                "confirmed": if confirmed { json!({"boot_id":boot}) } else { Value::Null },
                "finished":{"boot_id":boot,"monotonic_ms":ms}}})
        };
        assert_eq!(
            failed_hand_over(&item("not_sent", true, "boot-a", NOW - 5_000), &now()),
            Some(false)
        );
        assert_eq!(
            failed_hand_over(&item("uncertain", true, "boot-a", NOW - 5_000), &now()),
            Some(true)
        );
        // Never said yes to, long ago, another boot, or started: not news.
        assert_eq!(
            failed_hand_over(&item("not_sent", false, "boot-a", NOW), &now()),
            None
        );
        assert_eq!(
            failed_hand_over(&item("not_sent", true, "boot-a", NOW - JUST_MS - 1), &now()),
            None
        );
        assert_eq!(
            failed_hand_over(&item("not_sent", true, "boot-b", NOW), &now()),
            None
        );
        assert_eq!(
            failed_hand_over(&item("accepted", true, "boot-a", NOW), &now()),
            None
        );
        let told = note_failed(&task("claude", json!([])), false).unwrap();
        assert!(
            told.contains("did not reach Claude Code") && told.contains("offer to try again"),
            "{told}"
        );
        assert!(
            note_failed(&task("codex", json!([])), true)
                .unwrap()
                .contains("may not have reached Codex")
        );
    }

    /// #560, V1's third lesson: Harness accepted work and its agent never
    /// answered (V1 held the run 15 minutes and said nothing). Here work the
    /// person said yes to was accepted half an hour ago, and every look since
    /// finds its agent `offline`: the person should hear that it isn't
    /// answering, in plain words, as a failed hand-over is told. Today an
    /// accepted agent that goes silent is watched for a day and never told.
    #[test]
    #[ignore = "#560: accepted work whose agent never answers is never told"]
    fn work_whose_agent_never_answers_after_it_was_accepted_is_told() {
        let root = std::env::temp_dir().join(format!("task-news-silent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let socket = root.join("request.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        const ACCEPTED: u64 = NOW - 30 * 60_000;
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                let asked: Value = serde_json::from_str(&line).unwrap();
                let answer = match (asked["op"].as_str().unwrap(), asked["id"].as_u64()) {
                    ("list", _) => json!({"ok":true,"store_id":"s1","items":[
                        {"id":4,"observations":3,"dispatch":{"state":"accepted",
                            "confirmed":{"boot_id":"boot-a"},
                            "finished":{"boot_id":"boot-a","monotonic_ms":ACCEPTED}}}]}),
                    ("get_task", Some(4)) => task(
                        "claude",
                        json!([
                            seen(1, "offline", "boot-a", ACCEPTED + 30_000),
                            seen(2, "offline", "boot-a", ACCEPTED + 600_000),
                            seen(3, "offline", "boot-a", NOW - 60_000),
                        ]),
                    ),
                    _ => json!({"ok":false}),
                };
                writeln!(stream, "{answer}").unwrap();
            }
        });
        let news = read_once(&socket, &now(), &BTreeSet::new(), &BTreeSet::new());
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(
            news.len(),
            1,
            "half an hour without an answer from the agent is news"
        );
        assert!(news[0].note.contains("Claude Code"), "{}", news[0].note);
    }

    const RUN: u64 = 1_791_964_000_000;

    fn status(store: &str, changes: u32, state: AgentState) -> TaskStatus {
        TaskStatus {
            store_id: store.into(),
            run: RUN,
            unread: vec![],
            settled: Vec::new(),
            tasks: vec![lamp_msgs::TaskProgress {
                task: 3,
                engine: "claude".into(),
                state,
                working_s: Some(125),
                last_active_unix_ms: None,
                read_unix_ms: 0,
                changes,
                changed: changes > 0,
                new_share: 0.5,
                signs: vec![],
                signs_seen: vec![],
            }],
        }
    }
    fn decided(choice: &str, shadow: bool, task: &str) -> Decision {
        Decision {
            about: "progress".into(),
            choice: choice.into(),
            probability: 0.9,
            answers: Default::default(),
            by: "stand-in".into(),
            took_ms: 1,
            shadow,
            request: None,
            task: Some(task.into()),
            arrival: None,
            ask: None,
        }
    }
    const STORE: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn only_a_fresh_acting_say_now_about_the_newest_change_of_this_store_is_told() {
        let key = format!("{STORE}:3:4@{RUN}");
        let now = status(STORE, 4, AgentState::Working);
        let good = wanted(&decided("say_now", false, &key), 1_000, Some(&now)).unwrap();
        assert_eq!(good.key(), format!("{STORE}:3:progress-{RUN}-4"));
        assert_eq!(good.run, RUN);
        assert_eq!(good.working_s, Some(125));
        // A status that lags behind the decision is no reason to drop it.
        let behind = status(STORE, 3, AgentState::Working);
        assert!(wanted(&decided("say_now", false, &key), 0, Some(&behind)).is_ok());
        let attention = Decision {
            about: "attention".into(),
            ..decided("say_now", false, &key)
        };
        for (decision, age, status, why) in [
            (attention, 0, Some(&now), "not_progress"),
            (decided("wait", false, &key), 0, Some(&now), "wait"),
            (decided("say_now", true, &key), 0, Some(&now), "shadow"),
            (decided("say_now", false, "3:4"), 0, Some(&now), "no_task"),
            // Without its run a count names no change.
            (
                decided("say_now", false, &format!("{STORE}:3:4")),
                0,
                Some(&now),
                "no_task",
            ),
            // Another run of the tasks part: its count 4 is another change.
            (
                decided("say_now", false, &format!("{STORE}:3:4@{}", RUN + 1)),
                0,
                Some(&now),
                "other_run",
            ),
            (
                decided("say_now", false, &key),
                PROGRESS_FRESH_MS + 1,
                Some(&now),
                "stale",
            ),
            (decided("say_now", false, &key), 0, None, "no_status"),
            (
                decided("say_now", false, &key),
                0,
                Some(&status(&"f".repeat(32), 4, AgentState::Working)),
                "other_store",
            ),
            (
                decided("say_now", false, &key),
                0,
                Some(&status(STORE, 5, AgentState::Working)),
                "older_change",
            ),
            (
                decided("say_now", false, &key),
                0,
                Some(&status(STORE, 4, AgentState::Idle)),
                "not_working",
            ),
            (
                decided("say_now", false, &format!("{STORE}:9:4@{RUN}")),
                0,
                Some(&now),
                "not_watched",
            ),
        ] {
            assert_eq!(wanted(&decision, age, status), Err(why));
        }
        assert!(
            wanted(
                &decided("say_now", false, &key),
                PROGRESS_FRESH_MS,
                Some(&now)
            )
            .is_ok()
        );
    }

    /// A progress note is one written turn too: long tasks and long lines,
    /// in letters of one byte or of several, fit and are taken.
    #[test]
    fn a_progress_note_says_who_what_how_far_and_fits_in_one_written_turn() {
        let signs = BTreeSet::from([Sign::TestsPass, Sign::PullRequest]);
        let short = note_progress(
            &task("claude", json!([])),
            "Fixed the tokenizer / all 118 tests pass",
            &signs,
            Some(600),
        )
        .unwrap();
        assert_eq!(
            short,
            "(Nobody has spoken. Progress on work you handed over for the person: Claude Code is still working on the task they gave it: \
             \"Suggest three 'friendly' names for a desk lamp\". Its latest progress line: \"Fixed the tokenizer / all 118 tests pass\". \
             Signs of a step forward in it: the tests pass, a pull request was opened. It has been at it for about 10 minutes. \
             Tell them how it is going in one short, natural sentence, the way a good chief of staff mentions it in passing, \
             starting with something like \"Oh, Claude Code has…\". Do not read the line out word for word, and no code, file paths or links. \
             No question at the end.)"
        );
        // No signs, a minute, Codex; nothing to say without a line.
        let bare = note_progress(
            &task("codex", json!([])),
            "Reading the parser",
            &BTreeSet::new(),
            Some(40),
        )
        .unwrap();
        assert!(
            bare.contains("Codex is still working")
                && bare.contains("about a minute.")
                && !bare.contains("Signs")
        );
        assert!(note_progress(&task("claude", json!([])), "  ", &signs, None).is_none());
        let english =
            "Read src/parser.rs and the failing test, fixed the end index, ran the suite. "
                .repeat(20);
        let vietnamese =
            "Đã đọc tệp phân tích cú pháp, sửa chỉ số cuối và chạy lại toàn bộ kiểm thử. "
                .repeat(20);
        let asks = [
            "Fix the quoted string bug in the parser".to_owned(),
            "Sửa lỗi chuỗi trong ngoặc kép ở bộ phân tích cú pháp, rồi chạy lại kiểm thử. "
                .repeat(10),
        ];
        for asked in &asks {
            for about in [&english, &vietnamese] {
                let told = note_progress(
                    &json!({"text": asked, "target": {"engine": "claude"}}),
                    about,
                    &BTreeSet::from([
                        Sign::TestsPass,
                        Sign::BuildWorks,
                        Sign::Fixed,
                        Sign::Found,
                        Sign::Done,
                        Sign::PullRequest,
                    ]),
                    Some(86_400),
                )
                .unwrap();
                assert!(
                    told.len() <= lamp_gemini::wire::MAX_NOTE_BYTES,
                    "{} bytes",
                    told.len()
                );
                lamp_gemini::wire::encode_written_turn(&told, None).unwrap();
                assert!(
                    told.contains("(it goes on)") && told.ends_with("No question at the end.)"),
                    "{told}"
                );
            }
        }
    }

    /// #765 review: words that come back at or past the decision's time
    /// are let go, however long the fetching took.
    #[test]
    fn words_that_come_back_past_the_decisions_time_are_let_go() {
        let root = std::env::temp_dir().join(format!("task-news-late-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let socket = root.join("request.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                let answer = json!({"ok":true,"store_id":STORE,"text":"Fix the parser",
                    "target":{"engine":"claude"},"progress":{"state":"working","run":RUN,"changes":4,
                    "about":"All tests pass","signs_seen":[]}});
                writeln!(stream, "{answer}").unwrap();
            }
        });
        let (offer, _offered) = mpsc::channel();
        let reader = ProgressReader {
            socket,
            offer,
            told: Arc::new(Mutex::new(BTreeSet::new())),
            stopping: Arc::new(AtomicBool::new(false)),
            status: Arc::new(Mutex::new(None)),
        };
        let wanted = Wanted {
            store: STORE.into(),
            task: 3,
            change: 4,
            run: RUN,
            working_s: None,
        };
        let mut said = BTreeSet::new();
        assert_eq!(
            reader.news(&wanted, &mut said, Instant::now()).unwrap_err(),
            "stale"
        );
        let fresh = Instant::now() + Duration::from_secs(60);
        let news = reader.news(&wanted, &mut said, fresh).unwrap();
        assert_eq!(news.until, Some(fresh));
        assert_eq!(
            news.progress,
            Some(ProgressOf {
                store: STORE.into(),
                task: 3,
                change: 4,
                run: RUN,
            })
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn progress_not_told_in_time_is_let_go() {
        let (offer, offered) = mpsc::channel();
        let (told, _heard) = mpsc::channel();
        let mut news = TaskNews {
            offered,
            told,
            waiting: VecDeque::new(),
            told_already: Arc::new(Mutex::new(BTreeSet::new())),
            stopping: Arc::new(AtomicBool::new(false)),
            status: Arc::new(Mutex::new(None)),
            heard: mpsc::channel().0,
        };
        let soon = News {
            key: "s:3:progress-1".into(),
            note: "(…)".into(),
            until: Some(Instant::now() + Duration::from_millis(50)),
            progress: None,
            finished: None,
            also: Vec::new(),
        };
        offer.send(soon.clone()).unwrap();
        assert_eq!(news.waiting(), Some(&soon));
        std::thread::sleep(Duration::from_millis(60));
        assert!(news.waiting().is_none());
    }

    #[test]
    fn news_waits_once_and_telling_takes_it() {
        let (offer, offered) = mpsc::channel();
        let (told, heard) = mpsc::channel();
        let told_already = Arc::new(Mutex::new(BTreeSet::new()));
        let mut news = TaskNews {
            offered,
            told,
            waiting: VecDeque::new(),
            told_already: told_already.clone(),
            stopping: Arc::new(AtomicBool::new(false)),
            status: Arc::new(Mutex::new(None)),
            heard: mpsc::channel().0,
        };
        assert!(news.waiting().is_none());
        let one = News {
            key: "s:1:1".into(),
            note: "(…)".into(),
            until: None,
            progress: None,
            finished: None,
            also: Vec::new(),
        };
        offer.send(one.clone()).unwrap();
        offer.send(one.clone()).unwrap();
        assert_eq!(news.waiting(), Some(&one));
        assert_eq!(news.tell(), Some(one));
        assert_eq!(heard.try_recv().unwrap(), Wake::Told("s:1:1".into()));
        assert!(news.waiting().is_none());
        // Asked for and heard before it was told: it is not told.
        let two = News {
            key: "s:2:1".into(),
            note: "(…)".into(),
            until: None,
            progress: None,
            finished: None,
            also: Vec::new(),
        };
        offer.send(two.clone()).unwrap();
        assert_eq!(news.waiting(), Some(&two));
        told_already.lock().unwrap().insert("s:2:1".into());
        assert!(news.waiting().is_none());
    }

    /// The task a piece of news is of, from its key, as each kind is made:
    /// a settled observation, a failed hand-over, and progress; a store
    /// with a colon in it is read from the right.
    #[test]
    fn news_names_its_task_from_its_key() {
        let of = |key: &str| {
            News {
                key: key.into(),
                note: String::new(),
                until: None,
                progress: None,
                finished: None,
                also: Vec::new(),
            }
            .task()
        };
        assert_eq!(of("s:12:34"), Some(12));
        assert_eq!(of("s:5:hand-over"), Some(5));
        assert_eq!(of("s:3:progress-7-4"), Some(3));
        assert_eq!(of("a:b:9:1"), Some(9));
        assert_eq!(of("s:x:1"), None);
        assert_eq!(of("s:1"), None);
    }

    /// News waiting to be told, with nothing behind it: its channels.
    fn waiting_news() -> (TaskNews, mpsc::Sender<News>, mpsc::Receiver<Wake>) {
        let (offer, offered) = mpsc::channel();
        let (told, heard) = mpsc::channel();
        let news = TaskNews {
            offered,
            told,
            waiting: VecDeque::new(),
            told_already: Arc::new(Mutex::new(BTreeSet::new())),
            stopping: Arc::new(AtomicBool::new(false)),
            status: Arc::new(Mutex::new(None)),
            heard: mpsc::channel().0,
        };
        (news, offer, heard)
    }

    /// The news of task `id` for `engine` in `state`, as the reader makes
    /// it, with an answer of `answer_chars` characters.
    fn news_of(id: u64, engine: &str, state: &str, answer_chars: usize) -> News {
        let mut task = task(engine, json!([]));
        task["text"] = json!(format!("Task number {id}: suggest names for a desk lamp"));
        let answer: String = format!("Answer {id}. ")
            .chars()
            .chain(std::iter::repeat('a'))
            .take(answer_chars)
            .collect();
        let data = json!({"state":state,"answer":answer,"truncated":false});
        News {
            key: format!("s:{id}:1"),
            note: note(&task, &data, 0).unwrap(),
            until: None,
            progress: None,
            finished: finished(&task, &data, 0),
            also: Vec::new(),
        }
    }

    /// The reviewer's P3 on #1161: tasks that finished together are told
    /// in one turn, at most three, in one note; one waiting for the person
    /// is told on its own, in its place; a fourth waits for the next turn.
    #[test]
    fn tasks_finished_together_are_told_in_one_turn() {
        let (mut news, offer, heard) = waiting_news();
        let (one, two) = (
            news_of(1, "claude", "idle", 80),
            news_of(2, "codex", "idle", 80),
        );
        let waits = news_of(3, "claude", "needs-input", 80);
        let (four, five) = (
            news_of(4, "claude", "idle", 80),
            news_of(5, "opencode", "idle", 80),
        );
        assert!(waits.finished.is_none());
        for each in [&one, &two, &waits, &four, &five] {
            offer.send(each.clone()).unwrap();
        }
        assert_eq!(news.waiting(), Some(&one));
        let told = news.tell_together().unwrap();
        assert_eq!(
            (told.key.as_str(), told.also.clone()),
            ("s:1:1", vec!["s:2:1".to_owned(), "s:4:1".to_owned()])
        );
        assert!(told.note.len() <= lamp_gemini::wire::MAX_NOTE_BYTES);
        assert!(told.note.contains("three pieces of work"), "{}", told.note);
        for (who, id) in [("Claude Code", 1), ("Codex", 2), ("Claude Code", 4)] {
            assert!(
                told.note.contains(&format!(
                    "{who} finished \"Task number {id}: suggest names for a desk lamp\" just now: \"Answer {id}."
                )),
                "{}",
                told.note
            );
        }
        let keys: Vec<Wake> = heard.try_iter().collect();
        assert_eq!(
            keys,
            ["s:1:1", "s:2:1", "s:4:1"].map(|key| Wake::Told(key.into()))
        );
        // The one waiting for the person, on its own and as it was.
        assert_eq!(news.waiting(), Some(&waits));
        assert_eq!(news.tell_together(), Some(waits));
        // The fifth, alone now: its own note.
        assert_eq!(news.tell_together(), Some(five));
        assert!(news.waiting().is_none());
    }

    /// Without the switch each is told on its own, as before.
    #[test]
    fn tasks_finished_together_are_told_one_by_one_without_the_switch() {
        let (mut news, offer, _heard) = waiting_news();
        let (one, two) = (
            news_of(1, "claude", "idle", 80),
            news_of(2, "codex", "idle", 80),
        );
        offer.send(one.clone()).unwrap();
        offer.send(two.clone()).unwrap();
        assert!(news.waiting().is_some());
        assert_eq!(news.tell(), Some(one));
        assert_eq!(news.tell(), Some(two));
    }

    /// One note holds 1,024 bytes. Three long answers would keep under 120
    /// characters each: two are told together, keeping more of each, and
    /// the third waits for the next turn, where it is told whole as ever.
    #[test]
    fn long_answers_told_together_each_keep_their_gist() {
        let (mut news, offer, _heard) = waiting_news();
        let long: Vec<News> = (1..=3)
            .map(|id| news_of(id, "claude", "idle", MOST_ANSWER_CHARS))
            .collect();
        for each in &long {
            offer.send(each.clone()).unwrap();
        }
        assert!(news.waiting().is_some());
        let told = news.tell_together().unwrap();
        assert_eq!(told.also, vec!["s:2:1".to_owned()]);
        assert!(told.note.len() <= lamp_gemini::wire::MAX_NOTE_BYTES);
        assert!(told.note.contains("two pieces of work"), "{}", told.note);
        for id in [1, 2] {
            let kept = told
                .note
                .split(&format!("\"Answer {id}. "))
                .nth(1)
                .and_then(|rest| rest.split(GOES_ON).next())
                .unwrap();
            assert!(
                kept.chars().count() + "Answer 1. ".len() >= TOGETHER_ANSWER_LEAST,
                "{kept:?}"
            );
        }
        assert_eq!(news.tell_together(), Some(long[2].clone()));
    }

    /// The reviewer on #1223: five wait, 1 and 2 finished with long
    /// answers, 3 waiting for the person, 4 finished with a long answer and
    /// 5 with a short one. Three long ones would keep too little, so 1 and
    /// 2 go together; 4, left out, keeps its place behind 3, which is told
    /// next and not a turn later; then 4 and 5. A cut answer told together
    /// keeps #757's guard: its gist from its beginning, never only that the
    /// rest is on their computer.
    #[test]
    fn a_task_left_out_of_a_turn_keeps_its_place_and_cut_answers_keep_their_gist() {
        let (mut news, offer, _heard) = waiting_news();
        let line = [
            news_of(1, "claude", "idle", MOST_ANSWER_CHARS),
            news_of(2, "codex", "idle", MOST_ANSWER_CHARS),
            news_of(3, "claude", "needs-input", 80),
            news_of(4, "claude", "idle", MOST_ANSWER_CHARS),
            news_of(5, "claude", "idle", 80),
        ];
        for each in &line {
            offer.send(each.clone()).unwrap();
        }
        assert!(news.waiting().is_some());
        let first = news.tell_together().unwrap();
        assert_eq!(
            (first.key.as_str(), first.also.clone()),
            ("s:1:1", vec!["s:2:1".to_owned()])
        );
        assert!(first.note.contains("never only that"), "{}", first.note);
        assert_eq!(news.tell_together(), Some(line[2].clone()));
        let third = news.tell_together().unwrap();
        assert_eq!(
            (third.key.as_str(), third.also.clone()),
            ("s:4:1", vec!["s:5:1".to_owned()])
        );
        assert!(third.note.len() <= lamp_gemini::wire::MAX_NOTE_BYTES);
        assert!(third.note.contains("never only that"), "{}", third.note);
        assert!(news.waiting().is_none());
        // Nothing cut, nothing said of the rest.
        let (mut news, offer, _heard) = waiting_news();
        offer.send(news_of(6, "claude", "idle", 80)).unwrap();
        offer.send(news_of(7, "codex", "idle", 80)).unwrap();
        assert!(news.waiting().is_some());
        let whole = news.tell_together().unwrap();
        assert_eq!(whole.also, vec!["s:7:1".to_owned()]);
        assert!(!whole.note.contains("never only that"), "{}", whole.note);
    }

    fn settled(task: u64) -> lamp_msgs::TaskSettled {
        lamp_msgs::TaskSettled {
            task,
            engine: "claude".into(),
            state: AgentState::Idle,
            settled: true,
            at_unix_ms: RUN + task,
        }
    }

    /// #560: a task settled wakes the reader once, while it stays listed;
    /// another task, or the same one settled again later, wakes it again.
    #[test]
    fn a_task_newly_settled_wakes_the_reader_once() {
        let mut seen = BTreeSet::new();
        let mut at = status(STORE, 4, AgentState::Working);
        assert!(!newly_settled(&at, &mut seen));
        at.settled = vec![settled(7)];
        assert!(newly_settled(&at, &mut seen));
        assert!(!newly_settled(&at, &mut seen));
        at.settled.push(settled(8));
        assert!(newly_settled(&at, &mut seen));
        assert!(!newly_settled(&at, &mut seen));
        // Task 8 found settled again, later, as a new observation.
        at.settled[1].at_unix_ms += 60_000;
        assert!(newly_settled(&at, &mut seen));
        // Gone from the topic: nothing, and `seen` holds no more than this.
        at.settled.clear();
        assert!(!newly_settled(&at, &mut seen));
        assert!(seen.is_empty());
    }

    /// The reviewer on #1425: the tasks part restarted between the voice's
    /// `status` and its `told`, so the `told` names the old session and is
    /// refused as `stale_session`. The store is read again, and the `told`
    /// sent with the new session is taken.
    #[test]
    fn a_told_refused_across_a_restart_of_the_tasks_part_is_sent_again() {
        let root = std::env::temp_dir().join(format!("task-told-restart-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let (sent, lines) = mpsc::channel();
        for (name, answers) in [
            (
                "request.sock",
                vec![
                    json!({"ok":true,"store_id":"s1","revision":4,"summary":{"session_id":"old"}}),
                    json!({"ok":true,"store_id":"s1","revision":4,"summary":{"session_id":"new"}}),
                ],
            ),
            (
                "voice.sock",
                vec![
                    json!({"ok":false,"error":"stale_session"}),
                    json!({"ok":true,"told":true}),
                ],
            ),
        ] {
            let listener = UnixListener::bind(root.join(name)).unwrap();
            let sent = sent.clone();
            std::thread::spawn(move || {
                for (stream, answer) in listener.incoming().zip(answers) {
                    let mut stream = stream.unwrap();
                    let mut line = String::new();
                    BufReader::new(&stream).read_line(&mut line).unwrap();
                    let _ = sent.send(serde_json::from_str::<Value>(&line).unwrap());
                    writeln!(stream, "{answer}").unwrap();
                }
            });
        }
        assert_eq!(send_told(&root, "s1:2:4"), Sent::Done);
        let told: Vec<Value> = lines
            .try_iter()
            .filter(|line| line["op"] == "told")
            .collect();
        assert_eq!(told.len(), 2);
        assert_eq!(
            (told[0]["session_id"].clone(), told[1]["session_id"].clone()),
            (json!("old"), json!("new"))
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// #560: the tasks part is read at once when a task settles, not at the
    /// next of the ten-second reads; news told is kept at once and has
    /// nothing read.
    #[test]
    fn a_task_newly_settled_is_read_at_once_and_told_news_reads_nothing() {
        let root = std::env::temp_dir().join(format!("task-news-wake-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let listener = UnixListener::bind(root.join("request.sock")).unwrap();
        let (listed, lists) = mpsc::channel();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = stream.unwrap();
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                let _ = listed.send(Instant::now());
                writeln!(stream, "{}", json!({"ok":true,"store_id":"s1","items":[]})).unwrap();
            }
        });
        let (offer, _offered) = mpsc::channel();
        let (wake, woken) = mpsc::channel();
        let told_file = root.join("told");
        let reader = {
            let (directory, told_file) = (root.clone(), told_file.clone());
            std::thread::spawn(move || {
                let shared = Mutex::new(BTreeSet::new());
                read_forever(&directory, &told_file, &offer, &woken, &shared, &|| {
                    Some(now())
                });
            })
        };
        // The first read, as the reader starts.
        lists.recv_timeout(Duration::from_secs(5)).unwrap();
        wake.send(Wake::Told("s1:1:1".into())).unwrap();
        assert!(lists.recv_timeout(Duration::from_millis(300)).is_err());
        assert!(read_told(&told_file).contains("s1:1:1"));
        let woke = Instant::now();
        wake.send(Wake::Settled).unwrap();
        let read = lists.recv_timeout(READ_EVERY).unwrap();
        assert!(
            read.duration_since(woke) < READ_EVERY / 2,
            "{:?}",
            read.duration_since(woke)
        );
        // Woken again at once: read again, but not sooner than a second
        // after that read (the first builder on #1156).
        wake.send(Wake::Settled).unwrap();
        let again = lists.recv_timeout(READ_EVERY).unwrap();
        let gap = again.duration_since(read);
        assert!(gap >= WOKEN_GAP && gap < READ_EVERY / 2, "{gap:?}");
        drop(wake);
        reader.join().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    /// #560: on the bus, a task settled on `tasks/status` wakes the reader
    /// once while it stays listed, and another task wakes it again.
    #[test]
    fn a_task_settled_on_the_bus_wakes_the_reader_once() {
        // A short root: a bus socket's path must fit in a socket address.
        let root = PathBuf::from(format!("/tmp/lv-settled-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = Bus::new(&root).unwrap();
        let statuses = bus
            .subscribe_typed(TaskStatus::TOPIC, "voice", TaskStatus::TYPE)
            .unwrap();
        let mut out = bus
            .publisher_declared(TaskStatus::TOPIC, &TaskStatus::declaration(), "tasks")
            .unwrap();
        let (wake, woken) = mpsc::channel();
        let stopping = Arc::new(AtomicBool::new(false));
        let watcher = {
            let stopping = stopping.clone();
            std::thread::spawn(move || wake_on_settled(statuses, &wake, &stopping))
        };
        let quiet = Duration::from_millis(400);
        let mut at = status(STORE, 4, AgentState::Working);
        out.publish_json(1, &at).unwrap();
        assert!(woken.recv_timeout(quiet).is_err());
        at.settled = vec![settled(7)];
        out.publish_json(2, &at).unwrap();
        assert_eq!(
            woken.recv_timeout(Duration::from_secs(5)),
            Ok(Wake::Settled)
        );
        // The next status, task 7 still listed: no second wake.
        at.tasks[0].changes = 5;
        out.publish_json(3, &at).unwrap();
        assert!(woken.recv_timeout(quiet).is_err());
        at.settled.push(settled(8));
        out.publish_json(4, &at).unwrap();
        assert_eq!(
            woken.recv_timeout(Duration::from_secs(5)),
            Ok(Wake::Settled)
        );
        stopping.store(true, Ordering::Relaxed);
        watcher.join().unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    /// When news is first told, stepping every 100 ms from `from` to 40 s
    /// on: a person speaks over `person`, the lamp over `lamp` (start and
    /// end), and the turn lasts while either does. In tenths of a second
    /// after `from`.
    fn first_told(
        at_pause: bool,
        from: u64,
        person: &[(u64, u64)],
        lamp: &[(u64, u64)],
    ) -> Option<u64> {
        const S: u64 = 1_000_000;
        let last = |spans: &[(u64, u64)], now: u64| {
            spans
                .iter()
                .filter(|(start, _)| *start <= now)
                .map(|(_, end)| (*end).min(now))
                .max()
        };
        (from..=from + 40 * S).step_by(100_000).find_map(|now| {
            let speech = last(person, now).unwrap_or(0);
            let spoke = last(lamp, now);
            let turn = speech.max(spoke.unwrap_or(0));
            let paused = now.saturating_sub(speech) >= PAUSE_US;
            quiet_for_news(at_pause, now, turn, spoke, speech, paused)
                .then_some((now - from) / 100_000)
        })
    }

    /// #560, the first builder's bound, the eighth's case: someone talks
    /// with the lamp until 18 s after the agent settled, and the lamp says
    /// nothing after. With `LAMP_TASK_NEWS_AT_PAUSE=on` the news is told at
    /// their first pause of a second, about 19 s; without it, 15 s after the
    /// last turn, about 33 s, as before.
    #[test]
    fn news_is_told_at_the_first_pause_of_a_second_only_with_the_switch() {
        const S: u64 = 1_000_000;
        let settled = 100 * S;
        let person = [(settled, settled + 18 * S)];
        assert_eq!(first_told(true, settled, &person, &[]), Some(190));
        assert_eq!(first_told(false, settled, &person, &[]), Some(330));
        // Only `on` turns it on.
        for value in [None, Some(""), Some("off"), Some("ON"), Some("1")] {
            assert!(!at_pause(value), "{value:?}");
        }
        assert!(at_pause(Some("on")));
    }

    /// The first builder on #1161: 1 s after the lamp's own reply is the
    /// person's turn, not a pause. The lamp's reply ends; the person starts
    /// at 1.2 s and talks 2 s: the news waits for that line to end and 1 s
    /// more (4.2 s), never told at 1 s. Nobody starts: it is told 3 s after
    /// the lamp's words (`AFTER_LAMP_US`), not at 1 s.
    #[test]
    fn after_the_lamps_own_words_news_waits_for_the_persons_turn() {
        const S: u64 = 1_000_000;
        let ended = 200 * S;
        let lamp = [(ended - 4 * S, ended)];
        let answered = [
            (ended - 6 * S, ended - 5 * S),
            (ended + 1_200_000, ended + 3_200_000),
        ];
        assert_eq!(first_told(true, ended, &answered, &lamp), Some(42));
        let asked = [(ended - 6 * S, ended - 5 * S)];
        assert_eq!(first_told(true, ended, &asked, &lamp), Some(30));
        // The reviewer on #1161: the lamp's own voice, scored as speech for
        // 0.2 s after its words end while the microphone falls back to its
        // floor, is not someone starting: still 3 s, not 1.2 s.
        let echoed = [(ended - 6 * S, ended - 5 * S), (ended, ended + 200_000)];
        assert_eq!(first_told(true, ended, &echoed, &lamp), Some(30));
        // A quick "yes" begun inside that tail and ending past it is the
        // person's: told 1 s after it, at 1.6 s.
        let yes = [
            (ended - 6 * S, ended - 5 * S),
            (ended + 250_000, ended + 600_000),
        ];
        assert_eq!(first_told(true, ended, &yes, &lamp), Some(16));
        // Without the switch, 15 s after the last turn either way.
        assert_eq!(first_told(false, ended, &asked, &lamp), Some(150));
    }
}
