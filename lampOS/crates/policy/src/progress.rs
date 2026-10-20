//! The decision "say this progress now?" (`docs/policy.md`, "Progress
//! news").
//!
//! The lamp hands computer work to Claude Code or Codex and, like a good
//! chief of staff, tells the person how it is going when a real step was
//! taken and the moment suits them. The tasks part says how each piece of
//! work is going on `tasks/status`, as typed state without the agent's
//! words; this asks a decision model whether a change is worth telling now,
//! from that state and the room. Watching and deciding cost no model
//! tokens of the realtime model: it is used only to say it, later, and not
//! by this.
//!
//! Two hard rules sit around the decider and are not left to it: at most
//! one note per task in [`Options::note_every`] (15 minutes), and never
//! while anyone is talking to the lamp: while the voice listens, thinks or
//! speaks, or within [`Options::quiet_for`] (15 s) of anything heard or
//! said. A question is not asked while either holds; an answer that comes
//! back once one holds is turned to `wait`, and asked again later. An
//! answer about work that is no longer as it was asked about (another
//! store, the task gone or no longer at work, or its line changed again) is
//! a `wait` too, and changes nothing kept for the work there now. With
//! `lamp-policy --rules` a third hard rule holds a question as the other
//! two do: a rule the person at the lamp kept about it speaking first
//! ([`crate::rules`], #813).
//!
//! Shadow first: decisions are published on `policy/decisions` and written
//! down with the state they were made from, and nothing acts on them.
//! With `lamp-policy --act progress` (#752) they are published with
//! `shadow: false`, and the voice tells a `say_now` (`task_news`, with
//! `LAMP_PROGRESS_NEWS=on`); every other question stays shadow, and the log
//! is written either way.
use crate::{
    attention::Room,
    decide::{Answer, Asked, Connection, Decider, Failed, Question, Usage},
    rules::{Kept, Kind},
};
/// Where the lamp is in time (#820).
pub use lamp_local_time::LocalTime;
use lamp_msgs::{AgentState, Decision, Rules, Sign, TaskProgress, TaskStatus, Voice};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    io,
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

/// What this decision is called on the bus and in the log.
pub const ABOUT: &str = "progress";
/// From this probability up, the lamp would say it now.
pub const SAY_FROM: f32 = 0.5;
/// At most one progress note per task in this long (#732).
pub const NOTE_EVERY: Duration = Duration::from_secs(15 * 60);
/// Nothing heard or said for this long, and the voice neither listening,
/// thinking nor speaking: nobody is talking to the lamp. The same quiet as
/// the voice's own news (`task_news::QUIET_US`).
pub const QUIET_FOR: Duration = Duration::from_secs(15);
/// A turn of the lamp's that began with nobody listened to in this long
/// before it is one it began itself, unasked.
pub const UNASKED_AFTER: Duration = Duration::from_secs(10);
const HOUR: Duration = Duration::from_secs(3_600);
/// What the rule gives when no model answers, yes and no. Authored, not
/// measured: nothing has been labelled yet to say how often it is right.
pub const RULE_YES: f32 = 0.6;
pub const RULE_NO: f32 = 0.1;

/// How the progress question is asked (`lamp-policy --progress`).
#[derive(Clone, Debug)]
pub struct Options {
    /// How long the decision model has for one asking.
    pub deadline: Duration,
    /// The lamp's local time, for the hour of the day: a fixed offset
    /// (`--utc-offset-minutes`) or a zone (`--zone`), whose offset is taken
    /// at the decision's moment (#820). Without it the hour is not known and
    /// is shown as null.
    pub local: Option<Arc<LocalTime>>,
    /// The two hard rules ([`NOTE_EVERY`], [`QUIET_FOR`]); shorter only in
    /// a test.
    pub note_every: Duration,
    pub quiet_for: Duration,
    /// The decisions act (`lamp-policy --act progress`): they are published
    /// with `shadow: false`, and the voice tells a `say_now`. Off unless
    /// set: shadow, as every other question.
    pub act: bool,
    /// It obeys what the people at the lamp asked about speaking first
    /// (`memory/rules`, `lamp-policy --rules`, [`crate::rules`], #813).
    /// Off unless set: the topic is not listened to.
    pub rules: bool,
}

impl Options {
    pub fn new(deadline: Duration, local: Option<Arc<LocalTime>>) -> Self {
        Self {
            deadline,
            local,
            note_every: NOTE_EVERY,
            quiet_for: QUIET_FOR,
            act: false,
            rules: false,
        }
    }
}

/// What the lamp would do with a piece of progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Choice {
    /// Tell the person now.
    SayNow,
    /// Keep it for now.
    Wait,
}

impl Choice {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SayNow => "say_now",
            Self::Wait => "wait",
        }
    }
}

/// A hard rule that kept a question from being asked, or turned an answer
/// to `wait`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Hold {
    /// Someone is talking to the lamp, or was a moment ago.
    Talking,
    /// A note about this task was chosen less than 15 minutes ago.
    ToldRecently,
    /// The answer came back about work that is no longer as it was asked
    /// about: another store, the task gone or no longer at work, or its
    /// line changed again (or counted afresh) while the question was out.
    Outdated,
    /// The person at the lamp asked it not to speak first now (#813).
    KeptRule,
}

impl Hold {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Talking => "talking",
            Self::ToldRecently => "told_recently",
            Self::Outdated => "outdated",
            Self::KeptRule => "kept_rule",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Judgement {
    pub probability: f32,
    pub choice: Choice,
    /// No model's answers are behind it: the rule decided.
    pub from_rule: bool,
}

/// Everything known when the question was put: the work, from
/// `tasks/status`, and the room, from the lamp's own topics.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Context {
    pub store_id: String,
    pub task: u64,
    /// The count of changes of the progress line that is judged, and the
    /// run of the tasks part that counted it: counts begin again at each
    /// run.
    pub change: u32,
    pub run: u64,
    pub engine: String,
    pub state: AgentState,
    pub working_s: Option<u64>,
    /// Seconds since the agent last did anything, by the two machines'
    /// wall clocks.
    pub idle_s: Option<u64>,
    pub changed_now: bool,
    pub new_share: f32,
    /// Signs seen since the last note about this task, and those told.
    pub new_signs: Vec<Sign>,
    pub told_signs: Vec<Sign>,
    pub notes_told: u32,
    pub since_note_s: Option<u64>,
    /// Whether the eyes see someone near the lamp; not known without a
    /// list of faces younger than two seconds.
    pub someone_here: Option<bool>,
    /// Seconds since anything was heard or said, or the voice listened,
    /// thought or spoke; none since the policy began.
    pub quiet_s: Option<u64>,
    pub room: Room,
    /// Turns the lamp began itself in the last hour ([`UNASKED_AFTER`]),
    /// and the notes this chose in it, which in shadow nobody hears.
    pub spoke_up_last_hour: u32,
    pub hour: Option<u8>,
}

impl Context {
    /// `<store>:<task>:<change>@<run>`, as `Decision::task` has it.
    pub fn key(&self) -> String {
        format!(
            "{}:{}:{}@{}",
            self.store_id, self.task, self.change, self.run
        )
    }
}

const LAMP: &str = "A small desk lamp robot on the desk of the person it works for. It hands computer work to coding agents and tells the person how it is going, like a thoughtful chief of staff: only what they would be glad to hear, at a moment that suits them.";

/// What the agent's maker calls it, as the person does.
fn agent(engine: &str) -> &str {
    match engine {
        "claude" => "Claude Code",
        "codex" => "Codex",
        // Devin Cloud's name on the bus (#1854).
        "devin_cloud" => "Devin",
        other => other,
    }
}

fn sign(sign: Sign) -> &'static str {
    match sign {
        Sign::TestsPass => "the tests pass",
        Sign::BuildWorks => "the build works",
        Sign::Fixed => "something was fixed",
        Sign::Found => "something was found",
        Sign::Done => "a part was done",
        Sign::PullRequest => "a pull request was opened",
    }
}

fn state_word(state: AgentState) -> &'static str {
    match state {
        AgentState::Working => "working",
        AgentState::Idle => "idle",
        AgentState::NeedsInput => "waiting for the person",
        AgentState::Starting => "starting",
        AgentState::Installing => "installing",
        AgentState::Stopped => "stopped",
        AgentState::Offline => "offline",
        AgentState::Failed => "failed",
        AgentState::Unknown => "unknown",
        AgentState::Missing => "not listed",
    }
}

/// Minutes to a tenth.
fn minutes(seconds: Option<u64>) -> Value {
    json!(seconds.map(|s| (s as f64 / 6.0).round() / 10.0))
}

fn room_told(room: Room) -> &'static str {
    match room {
        Room::Quiet => "quiet: nobody else has been talking",
        Room::Busy => "busy: people are talking to each other",
        Room::Unknown => "unknown",
    }
}

/// The state as the text that is sent, its fields in this order. A JSON
/// value keeps no order, and whatever is fitted to a model's answers later
/// belongs to the text it was shown, so it is put together here.
pub fn state_text(context: &Context) -> String {
    let signs = |signs: &[Sign]| json!(signs.iter().map(|s| sign(*s)).collect::<Vec<_>>());
    format!(
        "{{\"lamp\":{},\"work\":{{\"agent\":{},\"state\":{},\"minutes_at_it\":{},\
         \"minutes_since_it_last_did_anything\":{},\"its_progress_line_just_changed\":{},\
         \"share_of_the_line_that_is_new\":{},\"new_signs_of_a_step_forward\":{},\
         \"signs_the_person_was_already_told_about\":{},\"progress_notes_told_for_this_work\":{},\
         \"minutes_since_the_last_note_for_this_work\":{}}},\"room\":{{\"someone_at_the_lamp\":{},\
         \"seconds_since_anything_was_said\":{},\"room\":{},\
         \"times_the_lamp_spoke_up_unasked_in_the_last_hour\":{},\"hour_of_day\":{}}}}}",
        json!(LAMP),
        json!(agent(&context.engine)),
        json!(state_word(context.state)),
        minutes(context.working_s),
        minutes(context.idle_s),
        context.changed_now,
        json!((f64::from(context.new_share) * 100.0).round() / 100.0),
        signs(&context.new_signs),
        signs(&context.told_signs),
        context.notes_told,
        minutes(context.since_note_s),
        json!(context.someone_here),
        json!(context.quiet_s),
        json!(room_told(context.room)),
        context.spoke_up_last_hour,
        json!(context.hour),
    )
}

/// [`state_text`] as a value, for the log.
pub fn state(context: &Context) -> Value {
    serde_json::from_str(&state_text(context)).expect("the state is JSON")
}

const OPENING: &str = "state.work is how a piece of computer work that the lamp handed to a coding agent is going while the agent is still at it, read from the agent's own progress line without its words; state.room is how things are around the lamp now. ";

/// The three questions: each one's name and the rest of its instruction
/// after [`OPENING`]. Authored, not fitted: no example has been labelled.
const ASKED: [(&str, &str); 3] = [
    (
        "worth_telling",
        "Is there a real step forward in state.work that the person who asked for the work would be glad to hear about, such as the tests now passing, a build that works, a bug found or fixed, or a pull request opened, rather than routine work going on?",
    ),
    (
        "good_moment",
        "Is this a good moment for the lamp to speak up unasked with one short piece of news: someone is at the lamp, nobody has spoken for a little while, the room is not busy with other people's talk, the lamp has not spoken up unasked often in the last hour, and it is not late at night or early in the morning?",
    ),
    (
        "too_much",
        "Would one more note about this work now be too much for the person: notes about it were told often or a short while ago, or its new signs are only ones they were already told about?",
    ),
];

/// The three questions, in the order they are asked in.
pub fn questions() -> Vec<(String, Question)> {
    ASKED
        .iter()
        .map(|(name, rest)| {
            (
                (*name).to_owned(),
                Question::Noul {
                    instructions: format!("{OPENING}{rest}"),
                },
            )
        })
        .collect()
}

/// The probability that the lamp should say it now: worth telling, at a
/// good moment, and not too much, as if the three were independent. Each
/// answer by its name, or the name of the first that is missing.
pub fn combine(
    answers: &BTreeMap<String, Answer>,
) -> Result<(Judgement, BTreeMap<String, f32>), &'static str> {
    let mut three = [0.0f32; 3];
    for (to, (name, _)) in three.iter_mut().zip(ASKED) {
        match answers.get(name) {
            Some(Answer::Noul(yes)) if (0.0..=1.0).contains(yes) => *to = *yes,
            _ => return Err(name),
        }
    }
    let [worth, moment, too_much] = three;
    let probability = (worth * moment * (1.0 - too_much)).clamp(0.0, 1.0);
    let by_name = ASKED
        .iter()
        .zip(three)
        .map(|((name, _), yes)| ((*name).to_owned(), yes))
        .collect();
    Ok((
        Judgement {
            probability,
            choice: if probability >= SAY_FROM {
                Choice::SayNow
            } else {
                Choice::Wait
            },
            from_rule: false,
        },
        by_name,
    ))
}

/// The judgement when no model answers: say it only with a sign not told
/// yet, someone seen at the lamp, a minute of quiet or more, a room not
/// known to be busy, fewer than two turns begun unasked in the hour, and
/// between eight in the morning and ten at night if the hour is known.
pub fn fallback(context: &Context) -> Judgement {
    let yes = !context.new_signs.is_empty()
        && context.someone_here == Some(true)
        && context.quiet_s.is_none_or(|quiet| quiet >= 60)
        && context.room != Room::Busy
        && context.spoke_up_last_hour < 2
        && context.hour.is_none_or(|hour| (8..22).contains(&hour));
    Judgement {
        probability: if yes { RULE_YES } else { RULE_NO },
        choice: if yes { Choice::SayNow } else { Choice::Wait },
        from_rule: true,
    }
}

/// The hour of the day at `unix_ms`, `offset` minutes east of UTC.
pub fn hour_of_day(unix_ms: u64, offset: i32) -> u8 {
    let minutes = (unix_ms / 60_000) as i64 + i64::from(offset);
    (minutes.rem_euclid(1_440) / 60) as u8
}

/// One line of the decisions log for this decision: what was known, what
/// the model was shown and answered, and what was chosen.
#[derive(Clone, Debug, Serialize)]
pub struct Line {
    pub at_unix_ms: u64,
    pub about: &'static str,
    pub shadow: bool,
    pub context: Context,
    pub state: Value,
    pub asked: String,
    pub answers: BTreeMap<String, f32>,
    pub judgement: Judgement,
    pub by: String,
    pub took_ms: u32,
    /// A hard rule that turned the answer to `wait` after it was asked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guard: Option<Hold>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<Connection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed_why: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

/// A decision made: what goes on the bus, what is written down.
#[derive(Clone, Debug)]
pub struct Decided {
    pub decision: Decision,
    pub line: Line,
}

/// What was counted from start to stop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    pub decided: u64,
    pub by_rule: u64,
    pub said: u64,
    /// Questions kept back by each hard rule, once for each change.
    pub held_talking: u64,
    pub held_recent: u64,
    pub held_rule: u64,
}

#[derive(Debug)]
struct Tracked {
    last: TaskProgress,
    /// The newest change asked about and decided.
    judged: u32,
    /// The hold last counted for this change, so each is counted once.
    held: Option<(u32, Hold)>,
    told_at: Option<Instant>,
    told: BTreeSet<Sign>,
    notes: u32,
    /// No progress of it has been read in this run yet (`unread` on
    /// `tasks/status`, or a new run that has not read it): `last` is an
    /// earlier run's, and nothing is asked about it until a read comes.
    unread: bool,
}

type Answered = (Result<Asked, Failed>, Duration);

/// The progress question as it runs beside the policy's loop, with its own
/// thread and its own decision model: a progress question never waits for
/// an attention one, nor the other way round.
pub struct Progress {
    options: Options,
    judge_name: String,
    states: mpsc::SyncSender<String>,
    answers: mpsc::Receiver<Answered>,
    asker_gone: bool,
    asking: Option<(Context, Instant)>,
    store_id: Option<String>,
    /// The run of the tasks part the counts now kept are of.
    run: Option<u64>,
    /// The tasks part is there: a status has come since it was last gone.
    /// While it is not, nothing is asked and an answer is outdated, and
    /// what is kept of each task (its notes, the quarter hour after one)
    /// stays for when it is back (#751 review, 17:05).
    live: bool,
    tasks: BTreeMap<u64, Tracked>,
    voice: Option<Voice>,
    last_talk: Option<Instant>,
    last_listening: Option<Instant>,
    /// The turn the voice is in, from its first `listening`, `thinking` or
    /// `speaking` to `resting`.
    turn: Option<Turn>,
    unasked: VecDeque<Instant>,
    notes: VecDeque<Instant>,
    /// The newest rules about speaking first, with `--rules` only.
    rules: Kept,
    totals: Totals,
}

/// One turn of the voice's, as it was known at its beginning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Turn {
    /// The lamp began it itself: its first state was `thinking` or
    /// `speaking`, with nobody listened to in the [`UNASKED_AFTER`] before.
    unasked: bool,
    /// Its sound has begun.
    spoke: bool,
}

impl Progress {
    /// Start the thread that asks `decider`.
    pub fn start<D: Decider + Send + 'static>(
        mut decider: D,
        options: Options,
    ) -> io::Result<Self> {
        let judge_name = decider.name().to_owned();
        let (states, asked) = mpsc::sync_channel::<String>(1);
        let (answered, answers) = mpsc::channel();
        let deadline = options.deadline;
        thread::Builder::new()
            .name("ask-progress".into())
            .spawn(move || {
                let questions = questions();
                for state in asked {
                    let started = Instant::now();
                    let answer = decider.ask(&state, &questions, deadline);
                    if answered.send((answer, started.elapsed())).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Self {
            options,
            judge_name,
            states,
            answers,
            asker_gone: false,
            asking: None,
            store_id: None,
            run: None,
            live: false,
            tasks: BTreeMap::new(),
            voice: None,
            last_talk: None,
            last_listening: None,
            turn: None,
            unasked: VecDeque::new(),
            notes: VecDeque::new(),
            rules: Kept::default(),
            totals: Totals::default(),
        })
    }

    pub fn totals(&self) -> Totals {
        self.totals
    }

    /// Whether its decisions act ([`Options::act`]).
    pub fn acts(&self) -> bool {
        self.options.act
    }

    /// Whether it obeys the people's rules ([`Options::rules`]).
    pub fn obeys_rules(&self) -> bool {
        self.options.rules
    }

    /// A new status from the tasks part. A message that fails its check is
    /// passed over. Another store begins again. Another run of the tasks
    /// part (it restarted, keeping its store and task numbers, and counts
    /// again from nothing) is judged from its own beginning for the tasks
    /// already followed: every change it counts is new here, whatever count
    /// an earlier run reached. Their notes and the quarter hour after one
    /// stay: those are the person's. A task seen for the first time is
    /// judged from what it says then, as always.
    pub fn status(&mut self, status: TaskStatus) {
        if status.check().is_err() {
            return;
        }
        self.live = true;
        let mut new_run = false;
        if self.store_id.as_deref() != Some(status.store_id.as_str()) {
            self.tasks.clear();
            self.store_id = Some(status.store_id.clone());
            self.run = Some(status.run);
        } else if self.run != Some(status.run) {
            new_run = true;
            for tracked in self.tasks.values_mut() {
                tracked.judged = 0;
                tracked.held = None;
                tracked.unread = true;
            }
            self.run = Some(status.run);
        }
        // A task still watched but not read in this run (it restarted, or
        // its reads failed) keeps what is kept of it; one gone from the
        // watch is let go.
        self.tasks.retain(|task, _| {
            status.tasks.iter().any(|t| t.task == *task) || status.unread.contains(task)
        });
        for progress in status.tasks {
            match self.tasks.get_mut(&progress.task) {
                Some(tracked) => {
                    // The tasks part began counting again in the same run
                    // (one without a run of its own to say so): what it
                    // says now is where judging begins again. A new run is
                    // judged from its beginning, above, whatever count it
                    // is first seen at: its first status may have been
                    // missed (#751 review, 17:05). So is a task's first read
                    // after a new run listed it unread: its count then is
                    // the new run's, not a counter going back (#793 review,
                    // 17:48).
                    if !new_run && !tracked.unread && progress.changes < tracked.last.changes {
                        tracked.judged = progress.changes;
                    }
                    tracked.last = progress;
                    tracked.unread = false;
                }
                None => {
                    // What it says now is where judging begins: a change
                    // seen before the policy was is not news to it.
                    let judged = progress.changes;
                    self.tasks.insert(
                        progress.task,
                        Tracked {
                            judged,
                            held: None,
                            told_at: None,
                            told: BTreeSet::new(),
                            notes: 0,
                            unread: false,
                            last: progress,
                        },
                    );
                }
            }
        }
        for task in &status.unread {
            if let Some(tracked) = self.tasks.get_mut(task) {
                tracked.unread = true;
            }
        }
    }

    /// The tasks part has gone: how the work stands is not known until it
    /// says again, and nothing is asked meanwhile. What is kept of each
    /// task stays: a restart is not a reason to tell the person again
    /// within the quarter hour.
    pub fn gone(&mut self) {
        self.live = false;
    }

    /// What the voice is doing. A turn whose first state is `thinking` or
    /// `speaking`, with nobody listened to in the [`UNASKED_AFTER`] before,
    /// is one the lamp began itself, counted once at its first sound
    /// however often it speaks before it ends. Whether a turn was asked
    /// for is known at its beginning and kept to its end: an answer whose
    /// first sound is slow (the model may take 30 s) is still an answer
    /// (the reviewer on #883, 05:22, of the hello's same count).
    pub fn voice(&mut self, state: Voice, now: Instant) {
        match state {
            Voice::Listening => {
                // Someone is listened to: whatever turn this is, it was
                // asked for.
                self.turn = Some(Turn {
                    unasked: false,
                    spoke: false,
                });
                self.last_listening = Some(now);
                self.last_talk = Some(now);
            }
            Voice::Thinking | Voice::Speaking => {
                let turn = self.turn.get_or_insert(Turn {
                    unasked: self
                        .last_listening
                        .is_none_or(|at| now.saturating_duration_since(at) > UNASKED_AFTER),
                    spoke: false,
                });
                if state == Voice::Speaking && turn.unasked && !turn.spoke {
                    self.unasked.push_back(now);
                }
                if state == Voice::Speaking {
                    turn.spoke = true;
                }
                self.last_talk = Some(now);
            }
            // The quiet begins when the voice stops, not at its last
            // heartbeat before: an active state is said again once a
            // second, so that one can be most of a second old.
            Voice::Resting => {
                self.turn = None;
                if matches!(
                    self.voice,
                    Some(Voice::Listening | Voice::Thinking | Voice::Speaking)
                ) {
                    self.last_talk = Some(now);
                }
            }
        }
        self.voice = Some(state);
    }

    pub fn voice_gone(&mut self) {
        self.voice = None;
        self.turn = None;
    }

    /// Something was heard, or said by the lamp.
    pub fn talk(&mut self, now: Instant) {
        self.last_talk = Some(now);
    }

    fn talking(&self, now: Instant) -> bool {
        matches!(
            self.voice,
            Some(Voice::Listening | Voice::Thinking | Voice::Speaking)
        ) || self
            .last_talk
            .is_some_and(|at| now.saturating_duration_since(at) < self.options.quiet_for)
    }

    /// Whether the work a question was about is still as it was asked
    /// about: this store, the task still watched and at work, and its line
    /// at the same change.
    fn current(&self, context: &Context) -> bool {
        self.live
            && self.store_id.as_deref() == Some(context.store_id.as_str())
            && self.run == Some(context.run)
            && self.tasks.get(&context.task).is_some_and(|tracked| {
                !tracked.unread
                    && tracked.last.state == AgentState::Working
                    && tracked.last.changes == context.change
            })
    }

    /// What the people at the lamp asked it about speaking first, newest.
    pub fn rules(&mut self, rules: Rules) {
        self.rules.set(rules);
    }

    fn hold(&self, tracked: &Tracked, now: Instant, unix_ms: u64, room: Room) -> Option<Hold> {
        if self.talking(now) {
            Some(Hold::Talking)
        } else if tracked
            .told_at
            .is_some_and(|at| now.saturating_duration_since(at) < self.options.note_every)
        {
            Some(Hold::ToldRecently)
        } else if self.rules.hold(
            Kind::News,
            unix_ms,
            // The zone's offset at this moment, so a kept morning ends at
            // the zone's noon across daylight saving (#820).
            self.options
                .local
                .as_ref()
                .map(|local| local.offset_minutes_at(unix_ms)),
            room == Room::Quiet,
        ) {
            Some(Hold::KeptRule)
        } else {
            None
        }
    }

    /// Take up an answer if one has come; else put the next question, if
    /// there is new progress and no hard rule holds. At most one decision
    /// a turn, which the caller publishes and writes down.
    pub fn turn(
        &mut self,
        now: Instant,
        unix_ms: u64,
        room: Room,
        someone_here: Option<bool>,
    ) -> Option<Decided> {
        while self
            .unasked
            .front()
            .is_some_and(|at| now.saturating_duration_since(*at) > HOUR)
        {
            self.unasked.pop_front();
        }
        while self
            .notes
            .front()
            .is_some_and(|at| now.saturating_duration_since(*at) > HOUR)
        {
            self.notes.pop_front();
        }
        if self.asking.is_some() {
            return match self.answers.try_recv() {
                Ok((answer, took)) => {
                    let (context, _) = self.asking.take().expect("a question is out");
                    Some(self.decide(context, Some(answer), took, now, unix_ms, room))
                }
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.asker_gone = true;
                    let (context, sent) = self.asking.take().expect("a question is out");
                    let took = now.saturating_duration_since(sent);
                    Some(self.decide(context, None, took, now, unix_ms, room))
                }
            };
        }
        let task = self.next(now, unix_ms, room)?;
        let context = self.context(task, now, unix_ms, room, someone_here);
        if self.asker_gone {
            return Some(self.decide(context, None, Duration::ZERO, now, unix_ms, room));
        }
        match self.states.try_send(state_text(&context)) {
            Ok(()) => {
                self.asking = Some((context, now));
                None
            }
            Err(mpsc::TrySendError::Full(_)) => None,
            Err(mpsc::TrySendError::Disconnected(_)) => {
                self.asker_gone = true;
                Some(self.decide(context, None, Duration::ZERO, now, unix_ms, room))
            }
        }
    }

    /// The task with new progress to ask about: still at work, its line
    /// changed since it was last judged, and no hard rule holding it. Each
    /// hold is counted once for each change it holds.
    fn next(&mut self, now: Instant, unix_ms: u64, room: Room) -> Option<u64> {
        if !self.live {
            return None;
        }
        let mut found = None;
        let mut holds = Vec::new();
        for (task, tracked) in &self.tasks {
            if tracked.unread
                || tracked.last.state != AgentState::Working
                || tracked.last.changes <= tracked.judged
            {
                continue;
            }
            match self.hold(tracked, now, unix_ms, room) {
                Some(hold) => holds.push((*task, hold)),
                None => {
                    found.get_or_insert(*task);
                }
            }
        }
        for (task, hold) in holds {
            let tracked = self.tasks.get_mut(&task).expect("listed");
            let change = tracked.last.changes;
            if tracked.held != Some((change, hold)) {
                tracked.held = Some((change, hold));
                match hold {
                    Hold::Talking => self.totals.held_talking += 1,
                    Hold::ToldRecently => self.totals.held_recent += 1,
                    Hold::KeptRule => self.totals.held_rule += 1,
                    // Only an answer can be outdated, never a question.
                    Hold::Outdated => {}
                }
            }
        }
        found
    }

    fn context(
        &self,
        task: u64,
        now: Instant,
        unix_ms: u64,
        room: Room,
        someone_here: Option<bool>,
    ) -> Context {
        let tracked = &self.tasks[&task];
        let last = &tracked.last;
        let seen: BTreeSet<Sign> = last.signs_seen.iter().copied().collect();
        Context {
            store_id: self.store_id.clone().unwrap_or_default(),
            task,
            change: last.changes,
            run: self.run.unwrap_or_default(),
            engine: last.engine.clone(),
            state: last.state,
            working_s: last.working_s,
            idle_s: last
                .last_active_unix_ms
                .map(|at| unix_ms.saturating_sub(at) / 1_000),
            changed_now: last.changed,
            new_share: last.new_share,
            new_signs: seen.difference(&tracked.told).copied().collect(),
            told_signs: tracked.told.iter().copied().collect(),
            notes_told: tracked.notes,
            since_note_s: tracked
                .told_at
                .map(|at| now.saturating_duration_since(at).as_secs()),
            someone_here,
            quiet_s: self
                .last_talk
                .map(|at| now.saturating_duration_since(at).as_secs()),
            room,
            // In shadow nobody hears the notes chosen, so they are counted
            // here; acting, the voice says them and they are counted as
            // turns the lamp began itself, once.
            spoke_up_last_hour: u32::try_from(
                self.unasked.len()
                    + if self.options.act {
                        0
                    } else {
                        self.notes.len()
                    },
            )
            .unwrap_or(u32::MAX),
            hour: self
                .options
                .local
                .as_ref()
                .map(|local| local.hour_of_day(unix_ms)),
        }
    }

    /// Decide from the model's answers, or by the rule when there are none
    /// to use (`answer` is none when the asking thread has gone).
    /// `room` is the room as it is now, when the answer has come: a hard
    /// rule holds by what is so now, and the question's own context stays
    /// as it was asked, for its record (the reviewer on #937).
    fn decide(
        &mut self,
        context: Context,
        answer: Option<Result<Asked, Failed>>,
        took: Duration,
        now: Instant,
        unix_ms: u64,
        room: Room,
    ) -> Decided {
        let connection = answer
            .as_ref()
            .and_then(|a| a.as_ref().ok())
            .and_then(|asked| asked.connection);
        let (mut judgement, answers, by, failed, usage) = match answer {
            Some(Ok(asked)) => match combine(&asked.answers) {
                Ok((judgement, answers)) => (judgement, answers, asked.model, None, asked.usage),
                Err(question) => (
                    fallback(&context),
                    BTreeMap::new(),
                    crate::policy::RULE.to_owned(),
                    Some(Failed::Missing(question.to_owned())),
                    asked.usage,
                ),
            },
            Some(Err(failed)) => (
                fallback(&context),
                BTreeMap::new(),
                crate::policy::RULE.to_owned(),
                Some(failed),
                None,
            ),
            None => (
                fallback(&context),
                BTreeMap::new(),
                crate::policy::RULE.to_owned(),
                Some(Failed::Unreachable {
                    during: "asking",
                    kind: io::ErrorKind::BrokenPipe,
                }),
                None,
            ),
        };
        // The hard rules again, as the answer comes: a yes that a rule now
        // holds becomes a wait, and the change is asked about again later.
        // Talking holds it whatever became of the task. An answer about
        // work no longer as it was asked about is a wait, and touches
        // nothing kept for what is there now: a task of another store with
        // the same number is not judged or held back by it.
        let current = self.current(&context);
        let guard = if self.talking(now) {
            Some(Hold::Talking)
        } else if !current {
            Some(Hold::Outdated)
        } else {
            self.tasks
                .get(&context.task)
                .and_then(|tracked| self.hold(tracked, now, unix_ms, room))
        }
        .filter(|_| judgement.choice == Choice::SayNow);
        if guard.is_some() {
            judgement.choice = Choice::Wait;
        }
        if current
            && guard.is_none()
            && let Some(tracked) = self.tasks.get_mut(&context.task)
        {
            tracked.judged = tracked.judged.max(context.change);
            if judgement.choice == Choice::SayNow {
                tracked.told_at = Some(now);
                tracked.told.extend(context.new_signs.iter().copied());
                tracked.notes = tracked.notes.saturating_add(1);
            }
        }
        if judgement.choice == Choice::SayNow {
            self.notes.push_back(now);
            self.totals.said += 1;
        }
        self.totals.decided += 1;
        self.totals.by_rule += u64::from(judgement.from_rule);
        let took_ms = u32::try_from(took.as_millis()).unwrap_or(u32::MAX);
        let decision = Decision {
            about: ABOUT.to_owned(),
            choice: judgement.choice.as_str().to_owned(),
            probability: judgement.probability,
            answers: answers.clone(),
            by: by.clone(),
            took_ms,
            shadow: !self.options.act,
            request: None,
            task: Some(context.key()),
            arrival: None,
            ask: None,
        };
        let line = Line {
            at_unix_ms: unix_ms,
            about: ABOUT,
            shadow: !self.options.act,
            state: state(&context),
            context,
            asked: self.judge_name.clone(),
            answers,
            judgement,
            by,
            took_ms,
            guard,
            connection,
            failed: failed.as_ref().map(Failed::kind),
            failed_why: failed.as_ref().map(Failed::to_string),
            usage,
        };
        Decided { decision, line }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> Context {
        Context {
            store_id: "0123456789abcdef0123456789abcdef".into(),
            task: 3,
            change: 4,
            run: 1_791_964_000_000,
            engine: "claude".into(),
            state: AgentState::Working,
            working_s: Some(754),
            idle_s: Some(12),
            changed_now: true,
            new_share: 0.666,
            new_signs: vec![Sign::TestsPass, Sign::PullRequest],
            told_signs: vec![Sign::Found],
            notes_told: 1,
            since_note_s: Some(1_200),
            someone_here: Some(true),
            quiet_s: Some(340),
            room: Room::Quiet,
            spoke_up_last_hour: 1,
            hour: Some(14),
        }
    }

    /// Devin Cloud is named on the bus `devin_cloud` (#1854): the model is
    /// told Devin.
    #[test]
    fn devin_cloud_is_told_as_devin() {
        assert_eq!(agent("devin_cloud"), "Devin");
        assert_eq!(agent("claude"), "Claude Code");
    }

    #[test]
    fn the_state_is_this_text_to_the_byte_and_holds_no_words_of_the_agent() {
        assert_eq!(
            state_text(&context()),
            "{\"lamp\":\"A small desk lamp robot on the desk of the person it works for. It hands computer work to coding agents and tells the person how it is going, like a thoughtful chief of staff: only what they would be glad to hear, at a moment that suits them.\",\
             \"work\":{\"agent\":\"Claude Code\",\"state\":\"working\",\"minutes_at_it\":12.6,\"minutes_since_it_last_did_anything\":0.2,\
             \"its_progress_line_just_changed\":true,\"share_of_the_line_that_is_new\":0.67,\
             \"new_signs_of_a_step_forward\":[\"the tests pass\",\"a pull request was opened\"],\
             \"signs_the_person_was_already_told_about\":[\"something was found\"],\"progress_notes_told_for_this_work\":1,\
             \"minutes_since_the_last_note_for_this_work\":20.0},\"room\":{\"someone_at_the_lamp\":true,\
             \"seconds_since_anything_was_said\":340,\"room\":\"quiet: nobody else has been talking\",\
             \"times_the_lamp_spoke_up_unasked_in_the_last_hour\":1,\"hour_of_day\":14}}"
        );
        // Not known is null.
        let unknown = Context {
            engine: "codex".into(),
            working_s: None,
            idle_s: None,
            since_note_s: None,
            someone_here: None,
            quiet_s: None,
            hour: None,
            room: Room::Unknown,
            ..context()
        };
        let value = state(&unknown);
        assert_eq!(value["work"]["agent"], "Codex");
        for (part, field) in [
            ("work", "minutes_at_it"),
            ("work", "minutes_since_it_last_did_anything"),
            ("work", "minutes_since_the_last_note_for_this_work"),
            ("room", "someone_at_the_lamp"),
            ("room", "seconds_since_anything_was_said"),
            ("room", "hour_of_day"),
        ] {
            assert!(value[part][field].is_null(), "{field}");
        }
        assert_eq!(
            context().key(),
            "0123456789abcdef0123456789abcdef:3:4@1791964000000"
        );
    }

    fn answers(three: [f32; 3]) -> BTreeMap<String, Answer> {
        questions()
            .into_iter()
            .zip(three)
            .map(|((name, _), yes)| (name, Answer::Noul(yes)))
            .collect()
    }

    #[test]
    fn worth_telling_at_a_good_moment_and_not_too_much_is_said_now() {
        let (yes, named) = combine(&answers([0.9, 0.8, 0.1])).unwrap();
        assert!((yes.probability - 0.648).abs() < 1e-6);
        assert_eq!((yes.choice, yes.from_rule), (Choice::SayNow, false));
        assert_eq!(named["too_much"], 0.1);
        for three in [[0.4, 1.0, 0.0], [1.0, 0.4, 0.0], [1.0, 1.0, 0.6]] {
            assert_eq!(combine(&answers(three)).unwrap().0.choice, Choice::Wait);
        }
        let mut missing = answers([1.0, 1.0, 0.0]);
        missing.remove("good_moment");
        assert_eq!(combine(&missing).unwrap_err(), "good_moment");
        let mut wrong = answers([1.0, 1.0, 0.0]);
        wrong.insert("too_much".into(), Answer::Noul(1.5));
        assert_eq!(combine(&wrong).unwrap_err(), "too_much");
        // The questions are asked in this order, each yes or no.
        let asked: Vec<String> = questions().into_iter().map(|(name, _)| name).collect();
        assert_eq!(asked, ["worth_telling", "good_moment", "too_much"]);
        assert!(questions().iter().all(|(_, q)| matches!(
            q,
            Question::Noul { instructions } if instructions.starts_with(OPENING)
        )));
    }

    #[test]
    fn the_rule_says_a_new_sign_only_to_someone_there_in_a_quiet_daytime() {
        assert_eq!(fallback(&context()).choice, Choice::SayNow);
        assert_eq!(fallback(&context()).probability, RULE_YES);
        for changed in [
            Context {
                new_signs: vec![],
                ..context()
            },
            Context {
                someone_here: None,
                ..context()
            },
            Context {
                quiet_s: Some(59),
                ..context()
            },
            Context {
                room: Room::Busy,
                ..context()
            },
            Context {
                spoke_up_last_hour: 2,
                ..context()
            },
            Context {
                hour: Some(23),
                ..context()
            },
            Context {
                hour: Some(7),
                ..context()
            },
        ] {
            let judged = fallback(&changed);
            assert_eq!((judged.choice, judged.probability), (Choice::Wait, RULE_NO));
        }
        assert!(fallback(&context()).from_rule);
    }

    struct Never;
    impl Decider for Never {
        fn ask(&mut self, _: &str, _: &[(String, Question)], _: Duration) -> Result<Asked, Failed> {
            Err(Failed::TimedOut)
        }
        fn name(&self) -> &str {
            "never"
        }
    }

    const RUN_A: u64 = 1_791_964_000_000;
    const RUN_B: u64 = 1_791_965_000_000;

    fn status(changes: u32, state: AgentState) -> TaskStatus {
        status_of(RUN_A, changes, state)
    }

    fn status_of(run: u64, changes: u32, state: AgentState) -> TaskStatus {
        TaskStatus {
            store_id: "0123456789abcdef0123456789abcdef".into(),
            run,
            unread: vec![],
            settled: Vec::new(),
            tasks: vec![TaskProgress {
                task: 3,
                engine: "codex".into(),
                state,
                working_s: None,
                last_active_unix_ms: None,
                read_unix_ms: 0,
                changes,
                changed: changes > 0,
                new_share: 0.0,
                signs: vec![],
                signs_seen: vec![],
            }],
        }
    }

    #[test]
    fn judging_begins_at_what_is_first_seen_and_again_when_the_counting_does() {
        let mut progress =
            Progress::start(Never, Options::new(Duration::from_secs(1), None)).unwrap();
        let now = Instant::now();
        progress.status(status(5, AgentState::Working));
        assert_eq!(progress.next(now, 0, Room::Quiet), None);
        progress.status(status(6, AgentState::Working));
        assert_eq!(progress.next(now, 0, Room::Quiet), Some(3));
        // The tasks part restarted and counts from nothing again.
        progress.status(status(0, AgentState::Working));
        assert_eq!(progress.next(now, 0, Room::Quiet), None);
        progress.status(status(1, AgentState::Working));
        assert_eq!(progress.next(now, 0, Room::Quiet), Some(3));
        // Not at work: not progress.
        progress.status(status(2, AgentState::Idle));
        assert_eq!(progress.next(now, 0, Room::Quiet), None);
        // Another store: begun again; a status that fails its check is
        // passed over.
        let mut other = status(9, AgentState::Working);
        other.store_id = "f".repeat(32);
        progress.status(other);
        assert_eq!(progress.next(now, 0, Room::Quiet), None);
        let mut wrong = status(10, AgentState::Working);
        wrong.store_id = "x".into();
        progress.status(wrong);
        assert_eq!(progress.next(now, 0, Room::Quiet), None);
        // The tasks part gone: nothing is asked until it says again.
        progress.status(status(11, AgentState::Working));
        progress.gone();
        assert_eq!(progress.next(now, 0, Room::Quiet), None);
        progress.status(status(12, AgentState::Working));
        assert_eq!(progress.next(now, 0, Room::Quiet), Some(3));
    }

    #[test]
    fn a_turn_the_lamp_begins_with_nobody_listened_to_is_unasked() {
        let mut progress =
            Progress::start(Never, Options::new(Duration::from_secs(1), None)).unwrap();
        let t = Instant::now();
        // Listened to, then answered: asked.
        progress.voice(Voice::Listening, t);
        progress.voice(Voice::Thinking, t + Duration::from_secs(2));
        progress.voice(Voice::Speaking, t + Duration::from_secs(3));
        progress.voice(Voice::Speaking, t + Duration::from_secs(4));
        progress.voice(Voice::Resting, t + Duration::from_secs(5));
        assert!(progress.unasked.is_empty());
        // Begun after a long quiet: unasked, once for the turn.
        progress.voice(Voice::Speaking, t + Duration::from_secs(60));
        progress.voice(Voice::Speaking, t + Duration::from_secs(61));
        assert_eq!(progress.unasked.len(), 1);
        // Talking holds the question; a quiet of 15 s lets it go.
        assert!(progress.talking(t + Duration::from_secs(61)));
        progress.voice(Voice::Resting, t + Duration::from_secs(62));
        assert!(progress.talking(t + Duration::from_secs(70)));
        assert!(!progress.talking(t + Duration::from_secs(62) + QUIET_FOR));
        progress.talk(t + Duration::from_secs(100));
        assert!(progress.talking(t + Duration::from_secs(101)));
    }

    /// The reviewer on #883 (05:22), of the hello's same count: an answer
    /// is an answer however slow its first sound, and one turn that speaks,
    /// thinks and speaks again is one turn. A turn the lamp began itself is
    /// counted, also when its sound is late.
    #[test]
    fn a_slow_answer_is_not_unasked_and_a_turn_is_counted_once() {
        let count = |turns: &[&[(Voice, u64)]]| {
            let mut progress =
                Progress::start(Never, Options::new(Duration::from_secs(1), None)).unwrap();
            let mut at = Instant::now();
            for turn in turns {
                for (state, after) in *turn {
                    progress.voice(*state, at + Duration::from_secs(*after));
                }
                at += Duration::from_secs(120);
            }
            progress.unasked.len()
        };
        use Voice::{Listening, Resting, Speaking, Thinking};
        // Thinking is said again once a second while it waits.
        let slow: Vec<(Voice, u64)> = [(Listening, 0)]
            .into_iter()
            .chain((1..30).map(|second| (Thinking, second)))
            .chain([(Speaking, 30), (Resting, 33)])
            .collect();
        let quick: &[(Voice, u64)] = &[(Listening, 0), (Thinking, 1), (Speaking, 1), (Resting, 4)];
        assert_eq!(count(&[&slow, &slow]), 0);
        assert_eq!(count(&[quick, quick]), 0);
        // Begun by the lamp itself: once each, however often it speaks.
        let own: &[(Voice, u64)] = &[
            (Thinking, 0),
            (Speaking, 2),
            (Thinking, 5),
            (Speaking, 6),
            (Resting, 9),
        ];
        assert_eq!(count(&[own]), 1);
        assert_eq!(count(&[own, own]), 2);
        // Its first sound late: still its own.
        let late: Vec<(Voice, u64)> = (0..30)
            .map(|second| (Thinking, second))
            .chain([(Speaking, 30), (Resting, 33)])
            .collect();
        assert_eq!(count(&[&late]), 1);
    }

    /// Says yes to everything, each answer only when the test lets it go.
    struct Held(mpsc::Receiver<()>);
    impl Decider for Held {
        fn ask(&mut self, _: &str, _: &[(String, Question)], _: Duration) -> Result<Asked, Failed> {
            self.0.recv().map_err(|_| Failed::TimedOut)?;
            Ok(Asked {
                answers: answers([0.9, 0.9, 0.0]),
                model: "held".into(),
                took: Duration::ZERO,
                usage: None,
                connection: None,
            })
        }
        fn name(&self) -> &str {
            "held"
        }
    }

    /// Ask about task 3's change 2 of the first store, do `meanwhile` while
    /// the question is out, then let the yes come back and take what is
    /// decided at `then`.
    fn asked_then(meanwhile: impl FnOnce(&mut Progress), then: Instant) -> (Progress, Decided) {
        asked_then_in(meanwhile, then, Room::Quiet)
    }

    /// As [`asked_then`], the answer taken up with the room as `room`.
    fn asked_then_in(
        meanwhile: impl FnOnce(&mut Progress),
        then: Instant,
        room: Room,
    ) -> (Progress, Decided) {
        let (release, held) = mpsc::channel();
        let mut progress =
            Progress::start(Held(held), Options::new(Duration::from_secs(1), None)).unwrap();
        let t = then - Duration::from_secs(60);
        progress.status(status(1, AgentState::Working));
        progress.status(status(2, AgentState::Working));
        assert!(progress.turn(t, 0, Room::Quiet, Some(true)).is_none());
        assert!(progress.asking.is_some(), "the question is out");
        meanwhile(&mut progress);
        release.send(()).unwrap();
        let waited = Instant::now();
        loop {
            if let Some(decided) = progress.turn(then, 0, room, Some(true)) {
                return (progress, decided);
            }
            assert!(waited.elapsed() < Duration::from_secs(5), "no answer");
            thread::sleep(Duration::from_millis(2));
        }
    }

    /// The reviewer's two transitions on #751 (2026-10-15, 15:23): a yes
    /// held while the task left and someone began to talk went out as
    /// `say_now`, because the talking rule was reached only through the
    /// task; and a yes for the first store's task 3, coming back after
    /// another store's task 3 had taken its place, marked that one judged
    /// and told, holding it back for 15 minutes it had not earned.
    #[test]
    fn an_answer_about_work_no_longer_as_asked_is_a_wait_and_changes_nothing_there_now() {
        let then = Instant::now() + Duration::from_secs(120);
        // Nothing moved: said now, and the task told.
        let (mut progress, decided) = asked_then(|_| {}, then);
        assert_eq!(
            (decided.decision.choice.as_str(), decided.line.guard),
            ("say_now", None)
        );
        progress.status(status(3, AgentState::Working));
        assert_eq!(
            progress.next(then, 0, Room::Quiet),
            None,
            "told less than 15 minutes ago"
        );

        // The task left the watch and someone began to talk: talking holds
        // it whatever became of the task.
        let (_, decided) = asked_then(
            |progress| {
                progress.status(TaskStatus {
                    store_id: "0123456789abcdef0123456789abcdef".into(),
                    run: RUN_A,
                    unread: vec![],
                    settled: Vec::new(),
                    tasks: vec![],
                });
                progress.voice(Voice::Listening, then);
            },
            then,
        );
        assert_eq!(
            (decided.decision.choice.as_str(), decided.line.guard),
            ("wait", Some(Hold::Talking))
        );
        // The task left, and nobody talks: outdated.
        let (_, decided) = asked_then(|progress| progress.gone(), then);
        assert_eq!(
            (decided.decision.choice.as_str(), decided.line.guard),
            ("wait", Some(Hold::Outdated))
        );
        assert_eq!(
            decided.decision.task.as_deref(),
            Some("0123456789abcdef0123456789abcdef:3:2@1791964000000")
        );

        // Another store's task 3 took its place: a wait, and that task is
        // neither judged nor held back by it.
        let other = |changes| {
            let mut other = status(changes, AgentState::Working);
            other.store_id = "f".repeat(32);
            other
        };
        let (mut progress, decided) = asked_then(
            |progress| {
                progress.status(other(7));
                progress.status(other(8));
            },
            then,
        );
        assert_eq!(
            (decided.decision.choice.as_str(), decided.line.guard),
            ("wait", Some(Hold::Outdated))
        );
        let there = &progress.tasks[&3];
        assert_eq!((there.judged, there.told_at, there.notes), (7, None, 0));
        assert_eq!(
            progress.next(then, 0, Room::Quiet),
            Some(3),
            "its own change is asked"
        );

        // The line changed again while the question was out, or the task
        // stopped working: a wait, and the newer change is asked about.
        let (mut progress, decided) = asked_then(
            |progress| progress.status(status(3, AgentState::Working)),
            then,
        );
        assert_eq!(decided.line.guard, Some(Hold::Outdated));
        assert_eq!(progress.tasks[&3].judged, 1);
        assert_eq!(progress.next(then, 0, Room::Quiet), Some(3));
        let (_, decided) = asked_then(
            |progress| progress.status(status(2, AgentState::Idle)),
            then,
        );
        assert_eq!(decided.line.guard, Some(Hold::Outdated));
    }

    /// The reviewer's finding on #765 (2026-10-15, 16:02): the tasks part
    /// counts changes afresh in each run and keeps its store and task
    /// numbers, so run B's change 2 is not run A's. A yes asked about run
    /// A's change 2 coming back once run B has reached change 2 too is
    /// about a line that is no longer there: a wait, and run B's change is
    /// asked about in its own right.
    #[test]
    fn a_yes_about_one_runs_change_coming_back_at_anothers_same_count_is_outdated() {
        let then = Instant::now() + Duration::from_secs(120);
        let (mut progress, decided) = asked_then(
            |progress| {
                progress.status(status_of(RUN_B, 0, AgentState::Working));
                progress.status(status_of(RUN_B, 2, AgentState::Working));
            },
            then,
        );
        assert_eq!(
            (decided.decision.choice.as_str(), decided.line.guard),
            ("wait", Some(Hold::Outdated))
        );
        assert_eq!(
            decided.decision.task.as_deref(),
            Some("0123456789abcdef0123456789abcdef:3:2@1791964000000")
        );
        assert_eq!(progress.tasks[&3].judged, 0);
        assert_eq!(
            progress.next(then, 0, Room::Quiet),
            Some(3),
            "run B's change is asked"
        );
        let asked = progress.context(3, then, 0, Room::Quiet, Some(true));
        assert_eq!(
            asked.key(),
            "0123456789abcdef0123456789abcdef:3:2@1791965000000"
        );
    }

    /// The same finding, for the question: after a restart the line at
    /// the count the last run was judged at is another line, and is asked
    /// about, even when the new run's first count was missed. The quarter
    /// hour after a note stays across the restart.
    #[test]
    fn a_restart_with_another_line_at_the_same_count_is_asked_about_again() {
        let mut progress =
            Progress::start(Never, Options::new(Duration::from_secs(1), None)).unwrap();
        let now = Instant::now();
        progress.status(status_of(RUN_A, 0, AgentState::Working));
        progress.status(status_of(RUN_A, 1, AgentState::Working));
        assert_eq!(progress.next(now, 0, Room::Quiet), Some(3));
        progress.tasks.get_mut(&3).unwrap().judged = 1;
        assert_eq!(
            progress.next(now, 0, Room::Quiet),
            None,
            "run A's change 1 was judged"
        );
        // Run B, its first count not seen here, at the same count.
        progress.status(status_of(RUN_B, 1, AgentState::Working));
        assert_eq!(progress.next(now, 0, Room::Quiet), Some(3));
        // A note told a moment ago still holds the task back.
        progress.tasks.get_mut(&3).unwrap().told_at = Some(now);
        progress.status(status_of(RUN_A, 1, AgentState::Working));
        assert_eq!(progress.next(now, 0, Room::Quiet), None);
    }

    /// The reviewer's finding on #751 (15:26): `Resting` left the quiet
    /// counted from the last active heartbeat, said once a second, so a
    /// change could be asked about after 14.1 s of quiet. It is counted
    /// from when the voice stopped; a `Resting` said again moves nothing.
    #[test]
    fn the_quiet_begins_when_the_voice_stops() {
        let mut progress =
            Progress::start(Never, Options::new(Duration::from_secs(1), None)).unwrap();
        let t = Instant::now();
        progress.status(status(1, AgentState::Working));
        progress.voice(Voice::Speaking, t);
        let stopped = t + Duration::from_millis(900);
        progress.voice(Voice::Resting, stopped);
        progress.status(status(2, AgentState::Working));
        assert!(progress.talking(t + QUIET_FOR));
        assert_eq!(progress.next(t + QUIET_FOR, 0, Room::Quiet), None);
        // Resting said again a few seconds on: the quiet still began when
        // the voice stopped.
        progress.voice(Voice::Resting, stopped + Duration::from_secs(5));
        assert!(progress.talking(stopped + QUIET_FOR - Duration::from_millis(1)));
        assert!(!progress.talking(stopped + QUIET_FOR));
        assert_eq!(progress.next(stopped + QUIET_FOR, 0, Room::Quiet), Some(3));
        // From Listening and from Thinking too.
        for active in [Voice::Listening, Voice::Thinking] {
            let at = t + Duration::from_secs(100);
            progress.voice(active, at);
            progress.voice(Voice::Resting, at + Duration::from_millis(900));
            assert!(progress.talking(at + QUIET_FOR));
            assert!(!progress.talking(at + Duration::from_millis(900) + QUIET_FOR));
        }
    }

    /// The reviewer on #751 (17:05): run A reached change 2, the tasks part
    /// restarted, run B's first status was missed, and B's change 1 came:
    /// the old rule for a count that went down set judging to 1, so it was
    /// never asked about. A new run is judged from its beginning.
    #[test]
    fn a_new_run_first_seen_at_a_lower_count_is_asked_about() {
        let mut progress =
            Progress::start(Never, Options::new(Duration::from_secs(1), None)).unwrap();
        let now = Instant::now();
        progress.status(status_of(RUN_A, 1, AgentState::Working));
        progress.status(status_of(RUN_A, 2, AgentState::Working));
        assert_eq!(progress.next(now, 0, Room::Quiet), Some(3));
        progress.tasks.get_mut(&3).unwrap().judged = 2;
        progress.status(status_of(RUN_B, 1, AgentState::Working));
        assert_eq!(progress.tasks[&3].judged, 0);
        assert_eq!(progress.next(now, 0, Room::Quiet), Some(3));
    }

    /// The reviewer on #751 (17:05): a note was chosen, the tasks part went
    /// (its publisher's lease ran out) and came back as another run, and its
    /// next changed line came 34 s after the note. It was asked about again,
    /// as if no note had been told. What is kept of the task outlives the
    /// tasks part going: the quarter hour holds it, and is counted.
    #[test]
    fn the_quarter_hour_after_a_note_outlives_the_tasks_part_going_and_coming_back() {
        let then = Instant::now() + Duration::from_secs(120);
        let (mut progress, decided) = asked_then(|_| {}, then);
        assert_eq!(decided.decision.choice, "say_now");
        progress.gone();
        // While it is gone nothing is asked, and an answer would be outdated.
        assert_eq!(
            progress.next(then + Duration::from_secs(4), 0, Room::Quiet),
            None
        );
        assert!(!progress.live);
        progress.status(status_of(RUN_B, 0, AgentState::Working));
        progress.status(status_of(RUN_B, 1, AgentState::Working));
        let at = then + Duration::from_secs(34);
        assert_eq!(
            progress.next(at, 0, Room::Quiet),
            None,
            "told less than 15 minutes ago"
        );
        assert_eq!(progress.totals().held_recent, 1);
        assert_eq!(progress.tasks[&3].notes, 1);
        // After the quarter hour, the new run's change is asked about.
        assert_eq!(progress.next(then + NOTE_EVERY, 0, Room::Quiet), Some(3));
    }

    /// The reviewer on #793 (16:46): a new run's first value lists its
    /// watched tasks as `unread` until a read comes. The policy keeps what
    /// it knows of them through it, and asks nothing of a task whose only
    /// progress is an earlier run's.
    #[test]
    fn a_task_unread_in_a_new_run_is_kept_and_not_asked_about_until_read() {
        let then = Instant::now() + Duration::from_secs(120);
        let (mut progress, decided) = asked_then(|_| {}, then);
        assert_eq!(decided.decision.choice, "say_now");
        // Run B starts: nothing read yet, task 3 still watched.
        progress.status(TaskStatus {
            store_id: "0123456789abcdef0123456789abcdef".into(),
            run: RUN_B,
            unread: vec![3],
            settled: Vec::new(),
            tasks: vec![],
        });
        let kept = &progress.tasks[&3];
        assert!(kept.unread && kept.notes == 1 && kept.told_at.is_some());
        // Run A's change 2 is no change of run B's to ask about.
        assert_eq!(progress.next(then + NOTE_EVERY, 0, Room::Quiet), None);
        // Its first read in run B: asked about once there is a change.
        progress.status(status_of(RUN_B, 0, AgentState::Working));
        assert!(!progress.tasks[&3].unread);
        assert_eq!(progress.next(then + NOTE_EVERY, 0, Room::Quiet), None);
        progress.status(status_of(RUN_B, 1, AgentState::Working));
        assert_eq!(progress.next(then + NOTE_EVERY, 0, Room::Quiet), Some(3));
        // The reviewer on #793 (17:48): run A reached count 2, run B listed
        // the task unread, its first read at count 0 was missed, and its
        // next came at count 1. That is run B's first change, asked about.
        let mut missed =
            Progress::start(Never, Options::new(Duration::from_secs(1), None)).unwrap();
        missed.status(status_of(RUN_A, 1, AgentState::Working));
        missed.status(status_of(RUN_A, 2, AgentState::Working));
        missed.tasks.get_mut(&3).unwrap().judged = 2;
        missed.status(TaskStatus {
            store_id: "0123456789abcdef0123456789abcdef".into(),
            run: RUN_B,
            unread: vec![3],
            settled: Vec::new(),
            tasks: vec![],
        });
        missed.status(status_of(RUN_B, 1, AgentState::Working));
        assert_eq!(missed.tasks[&3].judged, 0);
        assert_eq!(missed.next(then + NOTE_EVERY, 0, Room::Quiet), Some(3));
        // A task neither read nor unread has left the watch: let go.
        progress.status(TaskStatus {
            store_id: "0123456789abcdef0123456789abcdef".into(),
            run: RUN_B,
            unread: vec![],
            settled: Vec::new(),
            tasks: vec![],
        });
        assert!(progress.tasks.is_empty());
    }

    #[test]
    fn with_a_zone_the_hour_is_the_zones_at_the_moment_of_the_decision() {
        // #820: 23:00 in New York is 04:00 UTC in winter, 03:00 in summer.
        let york = Arc::new(LocalTime::zone("America/New_York").unwrap());
        let mut progress =
            Progress::start(Never, Options::new(Duration::from_secs(1), Some(york))).unwrap();
        let now = Instant::now();
        progress.status(status(5, AgentState::Working));
        progress.status(status(6, AgentState::Working));
        let task = progress.next(now, 0, Room::Quiet).unwrap();
        let hour = |unix_ms| progress.context(task, now, unix_ms, Room::Quiet, None).hour;
        assert_eq!(hour(1_768_449_600_000), Some(23));
        assert_eq!(hour(1_784_084_400_000), Some(23));
        // A fixed 300 minutes west is an hour off in summer.
        assert_eq!(hour_of_day(1_784_084_400_000, -300), 22);
    }

    #[test]
    fn the_hour_is_local_by_the_offset_given() {
        // 2026-10-14T17:31:02Z is half past midnight at UTC+7.
        assert_eq!(hour_of_day(1_791_999_062_000, 0), 17);
        assert_eq!(hour_of_day(1_791_999_062_000, 420), 0);
        assert_eq!(hour_of_day(1_791_999_062_000, -300), 12);
        assert_eq!(hour_of_day(0, -60), 23);
    }

    fn rules(quiet: bool, quiet_mornings: bool) -> Rules {
        Rules {
            person: "p-be9688998f085098".into(),
            quiet,
            quiet_mornings,
            rules: 1,
            hello: lamp_msgs::Unprompted::default(),
            news: lamp_msgs::Unprompted::default(),
            keep_light_on: false,
        }
    }

    /// #813: what the person at the lamp asked about it speaking first
    /// holds a question as the other hard rules do: not asked while it
    /// holds, counted once for the change, and asked once it no longer
    /// holds (noon at the lamp, or the rule forgotten). An answer that
    /// comes back after a rule was kept is a `wait` by it.
    #[test]
    fn a_kept_rule_holds_progress_news_and_no_model_is_asked() {
        // 2026-10-16 08:59 at a lamp seven hours east of UTC, and its noon.
        let (hanoi_0859, hanoi_noon) = (1_792_115_940_000, 1_792_115_940_000 + 181 * 60_000);
        let mut progress = Progress::start(
            Never,
            Options::new(
                Duration::from_secs(1),
                Some(Arc::new(LocalTime::Offset(7 * 60))),
            ),
        )
        .unwrap();
        let now = Instant::now();
        progress.status(status(1, AgentState::Working));
        progress.status(status(2, AgentState::Working));
        assert_eq!(progress.next(now, hanoi_0859, Room::Quiet), Some(3));
        progress.rules(rules(false, true));
        assert_eq!(progress.next(now, hanoi_0859, Room::Quiet), None);
        assert_eq!(progress.next(now, hanoi_0859, Room::Quiet), None);
        assert_eq!(progress.totals().held_rule, 1, "once for the change");
        assert_eq!(progress.next(now, hanoi_noon, Room::Quiet), Some(3));
        progress.rules(rules(true, false));
        assert_eq!(progress.next(now, hanoi_noon, Room::Quiet), None);
        progress.rules(rules(false, false));
        assert_eq!(progress.next(now, hanoi_0859, Room::Quiet), Some(3));
        // The seventh's typed limits (#904): news not while they talk holds
        // in a busy room, or one not known, and not in a quiet one; a
        // limit on the hello holds no news.
        progress.rules(Rules {
            news: lamp_msgs::Unprompted {
                not_while_talking: true,
                ..lamp_msgs::Unprompted::default()
            },
            hello: lamp_msgs::Unprompted {
                never: true,
                ..lamp_msgs::Unprompted::default()
            },
            ..rules(false, false)
        });
        assert_eq!(progress.next(now, hanoi_0859, Room::Busy), None);
        assert_eq!(progress.next(now, hanoi_0859, Room::Unknown), None);
        assert_eq!(progress.next(now, hanoi_0859, Room::Quiet), Some(3));
        assert_eq!(progress.totals().decided, 0);

        let (_, decided) = asked_then(
            |progress| progress.rules(rules(true, false)),
            Instant::now() + Duration::from_secs(120),
        );
        assert_eq!(
            (decided.line.judgement.choice, decided.line.guard),
            (Choice::Wait, Some(Hold::KeptRule))
        );
        assert_eq!(decided.decision.choice, "wait");
        assert_eq!(Hold::KeptRule.as_str(), "kept_rule");
    }

    /// #820: with a zone, a kept morning ends at the zone's noon on that
    /// day, daylight saving included; a fixed offset of the winter's is an
    /// hour late in summer.
    #[test]
    fn with_a_zone_a_kept_morning_ends_at_the_zones_noon() {
        // 2026-07-15 15:30 and 16:30 UTC: 11:30 and 12:30 in New York (EDT).
        let (york_1130, york_1230) = (1_784_129_400_000, 1_784_133_000_000);
        let york = Arc::new(LocalTime::zone("America/New_York").unwrap());
        let mut progress =
            Progress::start(Never, Options::new(Duration::from_secs(1), Some(york))).unwrap();
        let now = Instant::now();
        progress.status(status(1, AgentState::Working));
        progress.status(status(2, AgentState::Working));
        progress.rules(rules(false, true));
        assert_eq!(progress.next(now, york_1130, Room::Quiet), None);
        assert_eq!(progress.next(now, york_1230, Room::Quiet), Some(3));
        let winter = Arc::new(LocalTime::Offset(-300));
        let mut fixed =
            Progress::start(Never, Options::new(Duration::from_secs(1), Some(winter))).unwrap();
        fixed.status(status(1, AgentState::Working));
        fixed.status(status(2, AgentState::Working));
        fixed.rules(rules(false, true));
        assert_eq!(fixed.next(now, york_1230, Room::Quiet), None);
    }

    /// The reviewer on #937 (08:06): asked in a quiet room with news not
    /// while they talk kept, the answer comes back once the room is busy, or
    /// no longer known. The rule holds by the room as it is when the answer
    /// comes: `wait`. In a room still quiet the yes is said, and the record
    /// keeps the room the question was asked in.
    #[test]
    fn a_yes_that_comes_back_into_a_room_now_busy_is_held_by_not_while_talking() {
        let not_while_talking = || Rules {
            news: lamp_msgs::Unprompted {
                not_while_talking: true,
                ..lamp_msgs::Unprompted::default()
            },
            ..rules(false, false)
        };
        let then = Instant::now() + Duration::from_secs(120);
        for (room, expected) in [
            (Room::Busy, (Choice::Wait, Some(Hold::KeptRule))),
            (Room::Unknown, (Choice::Wait, Some(Hold::KeptRule))),
            (Room::Quiet, (Choice::SayNow, None)),
        ] {
            let (_, decided) =
                asked_then_in(|progress| progress.rules(not_while_talking()), then, room);
            assert_eq!(
                (decided.line.judgement.choice, decided.line.guard),
                expected,
                "{room:?}"
            );
            assert_eq!(decided.line.context.room, Room::Quiet, "{room:?}");
        }
    }
}
