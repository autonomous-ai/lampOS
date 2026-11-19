//! `LAMP_TASKS_ONE_SESSION` (#7246, phase 1, the lead's final design):
//! the lamp's default agent keeps ONE Harness pane, opened as soon as
//! lamp-tasks can (the Harness set up, the lamp's own machine known, its
//! default engine known), and everything goes into it as a message with
//! `harness reply` ([`crate::harness::Cli::reply_to`]): every skill run
//! for a gateway default agent (OpenClaw; its run's pass and token for
//! linked services still per run, through the skill-start launcher), and
//! every spoken task for the lamp itself on that engine
//! ([`spoken_launch`]). No own process or namespace per run. Off
//! (anything but `on` exactly), nothing here is made and every task is as
//! before.
//!
//! - **One agent at a time.** Harness has no steer verb and no turn id: a
//!   busy pane answers `NOT_IDLE`, and overlapping inputs are refused. So
//!   work that arrives while another holds the queue waits in a bounded FIFO
//!   ([`QUEUE_MOST`] waiting, [`QUEUE_WAIT`] each), spoken tasks ahead of
//!   skill runs; full, it is refused [`BUSY`], which lamp-skills already
//!   retries (#7239). One input, then `harness wait`, then `show`, then the
//!   next: each task's answer is its own turn's.
//! - **A header line per turn** ([`header`]), "New task <id>: ...", so the
//!   agent does not take a task as the last one's follow-up.
//! - **Never closed at a task's end.** A skill run's turn ends when the
//!   pane is idle on its header; a spoken task's when the store, which
//!   watches the pane as a session the person had (`Joined`), has read its
//!   answer ([`OneSession::release`]). A spoken task waiting is told its
//!   place ([`OneSession::queued_place`]); there is no overflow session.
//! - **A fresh start only near the context's limit, or for a session gone
//!   or stuck** (#7906, revising #7246's "no fresh start"; gone or stuck
//!   below). The pane is not replaced after a number of
//!   turns or idle time: the engines compact their own context (OpenClaw's
//!   gateway its `agents.defaults.compaction`, which the lamp's
//!   `openclaw.json` leaves at OpenClaw's own default; Hermes its own
//!   compression), and Harness has no compact verb. But OpenClaw's own
//!   compaction came too late on lamp-52e6: a make overflowed the 128k
//!   context at 175 messages, and the compaction dropped the make's task.
//!   So before each turn the pane's conversation is measured in OpenClaw's
//!   own record ([`crate::harness::openclaw_session_size`], a local SQLite
//!   read): a make goes into a fresh session once the old one holds
//!   [`MAKE_FRESH_TOKENS`] (60%) or more than [`MAKE_FRESH_MESSAGES`],
//!   any other task once it holds [`TASK_FRESH_TOKENS`] (85%) or more than
//!   [`TASK_FRESH_MESSAGES`] ([`near_context_limit`]); the record
//!   unreadable, the pane's own token count is weighed instead. The full
//!   session is replaced as at a rotation: the kept session `ensure
//!   --fresh`, its token kept, as for one gone or stuck, but closed by that
//!   `ensure` itself, never through [`OneSession::replace_kept`]: no
//!   `--now`, not counted toward [`REPLACE_MOST`]. At most one fresh start
//!   a turn; a small session is kept.
//! - **Next words only once the last were taken.** Before each reply the
//!   pane must be idle with its last words taken (`show`: no `told`, or
//!   `told.taken`): Harness queues words typed before that behind them,
//!   which would mix two tasks' answers.
//! - **Adopted by its id.** The pane's agent id from its `harness new` is
//!   kept in a state file ([`OneSession::with_state_file`]). At the next
//!   open (a lamp-tasks restart, a reboot: Harness's own restore keeps the
//!   same id) it is looked up with `show`: live, it is reused; stopped or
//!   gone, `harness resume <id>` once, then a fresh `new`. Never by name:
//!   a `new` with a name already there makes a second pane, and the agent
//!   may retitle it.
//! - **Reopened only when really gone**: Harness says the pane stopped,
//!   failed, is gone (`UNKNOWN_HARNESS`) or not running; Harness's daemon
//!   was started again ([`OneSession::forget`]); the lamp's machine id or
//!   its default engine changed. `offline`, `unknown`, `starting` and
//!   `installing` are waited on, but only for a bound (#7906). Reopened once
//!   for a turn; a second failure sends that run the per-run way, journaled.
//!   A spoken task is put back while the kept session is ensured again, at
//!   most [`STARTING_RESUMES_MOST`] times; then it fails [`NOT_STARTED`]
//!   (#7895).
//! - **Gone or stuck: replaced, never resumed for ever (#7906).** A kept
//!   session lamp-tasks closed itself (its token kept, its record gone) is
//!   ensured `--fresh` next, since `ensure` finds it by name and token. A
//!   session still `starting` 120 s from an open or a refused hand-over
//!   (sign 1), or a plain shell or gone (sign 2), goes through
//!   [`OneSession::replace_kept`]: closed `--now`, forgotten, its
//!   conversation deleted, and a fresh one opened; bounded by
//!   [`REPLACE_MOST`].
//! - **No reply, no pane for tasks.** Harness takes a reply only with the
//!   person's `programs-reply` setting on, off by default. A reply refused
//!   `REPLY_OFF` sends that task, and every later one until lamp-tasks
//!   starts again, the per-task way, as with the switch off: its own
//!   `harness new` with its words as `--task`, which needs no reply
//!   ([`OneSession::takes_reply`]), journaled once. Any other refusal is
//!   [`Sent::NotTyped`], never a second session. The pane is then closed
//!   and none opened again. Signed out, the pane is opened and adopted as
//!   signed in (#6407); only a `reply_off` stops it.
//! - **Cancel or deadline** ends the turn: Harness has no interrupt verb,
//!   so the pane is closed `--now` and opened again ([`OneSession::keep_open`]
//!   or the next run), journaled. That cost is paid only then.
//!
//! - **Rotation only where the engine can't compact.** No reset after a
//!   run. OpenClaw's pane is never rotated by count: its gateway compacts
//!   the session itself ([`rotation_due`]). An engine without that gets a
//!   fresh pane after [`ROTATE_TURNS`] turns or [`ROTATE_TOKENS`] tokens
//!   (estimated), as Hermes' gateway rotates its sessions.
//!
//! Out of scope for phase 1 (#7246): a Hermes default's pane (its skill
//! wrapper reads the run's token file once at its own start).
//!
//! **Opened as the person** (`--harness-as-person`, as the product's
//! lamp-tasks runs): Harness closes an idle pane after 30 min only when an
//! agent started it, so a pane opened as an agent would not stand. With
//! Harness called as an agent, no pane is opened and every task goes the
//! per-run way ([`lamp_place`]).
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use std::path::PathBuf;

use std::path::Path;

use crate::harness::{
    Cli, Closed, EngineKind, Ensured, Launch, LaunchOutcome, PaneSeen, SessionSize, Target, Took,
    TurnRecord, Typed,
};

/// The switch, in lamp-tasks' own environment: `on` exactly ([`on`]).
pub const SWITCH: &str = "LAMP_TASKS_ONE_SESSION";

/// [`SWITCH`]'s value: `on` exactly; anything else is off.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// The kept session's switch (openharness#4393), in lamp-tasks' own
/// environment: `on` exactly ([`on`]), and only with [`SWITCH`] on. On,
/// and Harness keeping program sessions (`harness capabilities`), the pane
/// is the lamp's own kept session: opened with `harness ensure`, typed into
/// with an acknowledged `reply`, closed with `close` and its OpenClaw
/// session deleted. Else, or off,
/// the pane is as before.
pub const KEPT_SWITCH: &str = "LAMP_TASKS_KEPT_SESSION";
/// The kept session's token file, in lamp-tasks' state folder: 0600, this
/// user's, 32 random bytes as hex, kept across restarts. Only its path is
/// passed to Harness, never its text, and never in any environment.
pub const TOKEN_FILE: &str = "kept-session.token";
/// The kept session's token, in random bytes (Harness takes 32 or more).
const TOKEN_BYTES: usize = 32;

/// The kept session's token file at `path` ([`TOKEN_FILE`]): kept as it is
/// when it is a plain file of this user's, 0600, of 32 bytes or more; else
/// written anew, 0600, in one rename. Its text is never journaled.
pub fn token_file(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    if let Ok(meta) = std::fs::symlink_metadata(path)
        && meta.file_type().is_file()
        && meta.permissions().mode() & 0o777 == 0o600
        && meta.uid() == rustix::process::geteuid().as_raw()
        && meta.len() >= TOKEN_BYTES as u64
    {
        return Ok(());
    }
    let mut bytes = [0u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes).map_err(|_| "no random bytes".to_string())?;
    let text: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    crate::hermes_config::replace(path, &text).map_err(|error| error.kind().to_string())?;
    eprintln!("tasks: one session: the kept session's token file made (0600)");
    Ok(())
}

/// An engine a kept record names, as [`EngineKind::argument`] says it.
fn engine_named(name: &str) -> Option<EngineKind> {
    [EngineKind::Openclaw, EngineKind::Hermes]
        .into_iter()
        .find(|engine| engine.argument() == name)
}

/// The pane's folder in lamp-tasks' Harness project (beside each run's
/// `skill-<run-id>`), the folder its `harness new` is started in.
pub const FOLDER: &str = "one-session";
/// The pane's name in Harness for `engine`, `lamp-one-<engine>`: for a
/// person reading `harness ps` only. The pane is found again by its id
/// ([`STATE_FILE`]), never by this name.
pub fn name_for(engine: EngineKind) -> String {
    format!("lamp-one-{}", engine.argument())
}
/// The file, in lamp-tasks' state folder, keeping the pane's agent id,
/// machine and engine, so the next lamp-tasks start adopts it.
pub const STATE_FILE: &str = "one-session.json";
/// The pane's first words: `harness new` needs a task, and OpenClaw waits
/// for its first prompt before it is ready.
pub const OPEN_PROMPT: &str = "You are the lamp's standing agent session. Tasks will be typed here one at a time, each starting with a line `New task <id>:`. Treat each as new work unless it says it follows an earlier one. For now, answer only: ready.";
/// The most runs waiting for the pane at once; one more is [`BUSY`].
pub const QUEUE_MOST: usize = 4;
/// How long one run waits for the pane before it is [`BUSY`]: as long as
/// lamp-skills' own queue holds a `busy` run (#7239).
pub const QUEUE_WAIT: Duration = Duration::from_secs(15 * 60);
/// How many times one spoken task's words may find the kept session not
/// running (`NOT_RUNNING`, or a `TIMEOUT` with nothing taken), each time
/// put back while the session is ensured again, before the task ends
/// [`NOT_STARTED`] (#7895). On lamp-52e6 a round took about 25 s, so 3
/// resumes end it at about 100 s, inside [`OPEN_IDLE_WITHIN`], the time a
/// new pane is given to become ready: a session not taking words by then
/// is not starting.
pub const STARTING_RESUMES_MOST: u32 = 3;
/// The `not_sent` code of a spoken task whose resumes are spent
/// ([`STARTING_RESUMES_MOST`]). Task news tells it with its plain `failed`
/// words ("it could not be started just now", with an offer to try again).
pub const NOT_STARTED: &str = "one_session_not_started";
/// [`Turn::send`]'s why when a spoken task's resumes are spent.
const STARTING_SPENT: &str = "the kept session did not take the words after its resumes";
/// Between two looks at a pane `wait` read idle but whose turn is not this
/// task's yet, or a busy pane before its words are typed again.
pub const TURN_LOOK: Duration = Duration::from_millis(500);
/// How long an open waits for the pane's first turn to end, so the pane
/// is ready before the first run; past it the pane is kept, and the first
/// run waits for it to be idle.
pub const OPEN_IDLE_WITHIN: Duration = Duration::from_secs(120);
/// The first wait after an open failed, doubled at each failure in a row
/// up to [`OPEN_RETRY_MOST`] (the daemon start's backoff style).
pub const OPEN_RETRY_FIRST: Duration = Duration::from_secs(5);
/// Between two `harness ensure` that said `ENGINE_NOT_READY` in a row
/// (openharness#4393: the old engine still exiting after a crash)...
pub const ENGINE_RETRY: Duration = Duration::from_secs(5);
/// ...within this, before [`NOT_READY_FIRST`]'s backoff.
pub const ENGINE_RETRY_WITHIN: Duration = Duration::from_secs(45);
/// The longest wait between two opens that failed.
pub const OPEN_RETRY_MOST: Duration = Duration::from_secs(300);
/// The pane's rotation, as Hermes' gateway rotates its sessions
/// (`runtimes/hermes/rotation.go`, `rotateMaxTurns`, `rotateTokenThreshold`):
/// a fresh pane after this many turns...
pub const ROTATE_TURNS: u64 = 40;
/// ...or this many tokens. Harness shows no token count, so they are
/// estimated from the words typed and the answers read, 4 characters a
/// token: a rough bound, not the engine's own count.
pub const ROTATE_TOKENS: u64 = 250_000;

/// Whether a pane on [`engine`] is due a fresh conversation after `turns`
/// turns and ~`tokens` tokens. OpenClaw's gateway compacts its own
/// sessions (`agents.defaults.compaction`), so its pane is never rotated
/// by count: the lamp keeps one conversation and lets OpenClaw keep it
/// small. Only an engine with no compaction of its own (Hermes, as its
/// gateway rotates) gets [`ROTATE_TURNS`] / [`ROTATE_TOKENS`]. The
/// tokens are counted from all words ever typed and read, not the
/// context's size, so they would rotate a compacting engine for nothing.
pub fn rotation_due(engine: EngineKind, turns: u64, tokens: u64) -> bool {
    match engine {
        EngineKind::Openclaw => false,
        _ => turns >= ROTATE_TURNS || tokens >= ROTATE_TOKENS,
    }
}
/// Stuck sessions replaced in a row with no turn answered between from
/// which the next open waits a backoff: [`OPEN_RETRY_FIRST`] at this many,
/// doubled at each more, at most [`OPEN_RETRY_MOST`].
pub const REPLACE_MOST: u32 = 2;
/// A `show` with no verdict on a pane refusing words ([`Seen::Unknown`]):
/// looked at again after this, never counted as stuck.
const UNKNOWN_LOOK_AGAIN: Duration = Duration::from_secs(10);
/// The context of the lamp's OpenClaw model, in tokens: 128k, as the
/// Gateway's overflow on lamp-52e6 named it (#7906: `estimated input
/// 127014 ... 128000-token context, messages=175`).
pub const CONTEXT_TOKENS: u64 = 128_000;
/// A make (#7906) is given a fresh session when the one it would be typed
/// into holds this many tokens or more (estimated at 4 stored bytes a
/// token, [`SessionSize`]): 60% of [`CONTEXT_TOKENS`], so the make's own
/// turn (its guide, the examples it reads, its tool output and its draft,
/// some 30-45k tokens) has 51k left...
pub const MAKE_FRESH_TOKENS: u64 = CONTEXT_TOKENS * 60 / 100;
/// ...or more than this many messages: 52e6 overflowed at 175 messages and
/// 127k tokens, about 725 tokens a message, so 100 is about 57%.
pub const MAKE_FRESH_MESSAGES: u64 = 100;
/// Any other task gets a fresh session only nearer the limit: 85% of
/// [`CONTEXT_TOKENS`], 19k left for a turn with no guide...
pub const TASK_FRESH_TOKENS: u64 = CONTEXT_TOKENS * 85 / 100;
/// ...or more than this many messages (about 85% at 725 tokens each).
pub const TASK_FRESH_MESSAGES: u64 = 150;

/// Whether a session of `size` (OpenClaw's own record; `None`: it could
/// not be read, so only `tokens`, the pane's own count of words typed and
/// read, is weighed) is too big for the next turn: a make's
/// ([`MAKE_FRESH_TOKENS`], [`MAKE_FRESH_MESSAGES`]) or another task's
/// ([`TASK_FRESH_TOKENS`], [`TASK_FRESH_MESSAGES`]).
pub fn near_context_limit(size: Option<SessionSize>, tokens: u64, make: bool) -> bool {
    let (most_tokens, most_messages) = if make {
        (MAKE_FRESH_TOKENS, MAKE_FRESH_MESSAGES)
    } else {
        (TASK_FRESH_TOKENS, TASK_FRESH_MESSAGES)
    };
    match size {
        Some(size) => size.bytes / 4 >= most_tokens || size.messages > most_messages,
        None => tokens >= most_tokens,
    }
}

/// #7906 sign 4a: a kept turn's words taken (Harness's `Taken`), but the
/// Gateway's record of its session not past where it was at the hand-over
/// this long after: the Gateway writes the task's message as soon as it
/// gets it, so no exec of this turn can be live.
pub const TURN_START_WITHIN: Duration = Duration::from_secs(60);
/// #7906 sign 4b: a turn open (`working`), its session `running`, and the
/// record's highest `seq` unmoved this long, with its last row a message
/// the model has not answered (`user`, or a `toolResult`). A last row of
/// the assistant's (a tool call still running, a long exec) is never cut
/// here: lamp-tasks can't see the exec's process, so it is left to the
/// run's deadline.
pub const TURN_STALL: Duration = Duration::from_secs(180);
/// #7906: a turn open while its session's status is no longer `running`
/// (`killed`, `failed`, `timeout`) this long: dead, though Harness still
/// says working (its own close needs two 1 s reads).
pub const DEAD_WORKING: Duration = Duration::from_secs(15);
/// How often a kept turn's record is read while its end is waited for:
/// beside `harness wait`, never in a healthy turn's way.
pub const TURN_WATCH: Duration = Duration::from_secs(5);

/// #7906 signs 3 and 4: why the kept session is gone, or as good as gone,
/// though Harness still lists it. It is replaced the one way every stuck
/// sign is ([`OneSession::kept_gone`], [`OneSession::replace_kept`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeptGone {
    /// Sign 3: OpenClaw's Gateway no longer holds its conversation (its
    /// own session key, [`crate::harness::openclaw_holds_key`]), as after a
    /// `sessions.delete`; the pane says "No session found".
    GatewaySession,
    /// Sign 4: a turn handed over (or the turn a hand-over waits on) is
    /// not moving, with why.
    NotMoving(String),
    /// A turn open while its session's status says it ended, with that
    /// status.
    DeadButWorking(String),
}

impl KeptGone {
    /// Why, for the journal and the run's failure.
    pub fn why(&self) -> String {
        match self {
            KeptGone::GatewaySession => "OpenClaw no longer holds its conversation".into(),
            KeptGone::NotMoving(why) => why.clone(),
            KeptGone::DeadButWorking(status) => {
                format!("its turn is open but OpenClaw's session is {status}")
            }
        }
    }
}

/// #7906 sign 4: reads where the Gateway's record is in one session's
/// turn ([`PaneHarness::openclaw_turns`]); `None` when not read.
pub type TurnReader<'a> = Box<dyn Fn() -> Option<TurnRecord> + Send + 'a>;

/// #7906 sign 4: one turn's progress over its looks at the Gateway's
/// record, by a clock the caller passes in. Never a verdict from nothing
/// read, and never from `show`'s tools, `ps`'s `lastActive` or pane text,
/// which don't move during a long exec.
#[derive(Clone, Debug)]
pub struct TurnWatch {
    /// Harness's `Taken` (or the first refusal).
    since: Instant,
    /// The record at the hand-over; [`TURN_START_WITHIN`] needs it.
    base: Option<(String, i64)>,
    start_within: Option<Duration>,
    stall: Duration,
    dead_within: Duration,
    last: Option<(String, i64)>,
    changed: Instant,
    started: bool,
    not_running: Option<Instant>,
}

impl TurnWatch {
    /// A turn taken at `since` with the record at `base`.
    pub fn taken(
        since: Instant,
        base: Option<(String, i64)>,
        start_within: Duration,
        stall: Duration,
    ) -> Self {
        Self {
            since,
            base,
            start_within: Some(start_within),
            stall,
            dead_within: DEAD_WORKING,
            last: None,
            changed: since,
            started: false,
            not_running: None,
        }
    }

    /// A hand-over refused `NOT_IDLE` since `since`: the turn it waits on
    /// is judged by [`TURN_STALL`] and [`DEAD_WORKING`] only.
    pub fn refused(since: Instant, stall: Duration) -> Self {
        Self {
            start_within: None,
            started: true,
            ..Self::taken(since, None, Duration::ZERO, stall)
        }
    }

    /// [`DEAD_WORKING`], shorter in tests.
    pub fn dead_within(mut self, within: Duration) -> Self {
        self.dead_within = within;
        self
    }

    /// One look at `now`: `record` as read (`None`: not read), `working`
    /// whether the turn is open (`harness wait --after` still held, or
    /// `NOT_IDLE`). Why it is gone, if so.
    pub fn look(
        &mut self,
        record: Option<&TurnRecord>,
        working: bool,
        now: Instant,
    ) -> Option<KeptGone> {
        let record = record?;
        let at = (record.session.clone(), record.seq);
        if self.last.as_ref() != Some(&at) {
            // The first read counts as a change: nothing is known before.
            self.changed = now;
        }
        if self.base.as_ref().is_some_and(|base| *base != at) {
            self.started = true;
        }
        self.last = Some(at);
        if let (Some(within), Some(_), false) = (self.start_within, &self.base, self.started)
            && now.saturating_duration_since(self.since) >= within
        {
            return Some(KeptGone::NotMoving(format!(
                "nothing of its turn reached OpenClaw within {} s of being taken",
                within.as_secs()
            )));
        }
        let running = record.status.as_deref() == Some("running");
        // Only once the turn reached OpenClaw: until then the status is
        // the last turn's, and 4a's bound is the one that applies.
        match (&record.status, working && !running && self.started) {
            (Some(status), true) => {
                let since = *self.not_running.get_or_insert(now);
                if now.saturating_duration_since(since) >= self.dead_within {
                    return Some(KeptGone::DeadButWorking(status.clone()));
                }
            }
            _ => self.not_running = None,
        }
        let still = now.saturating_duration_since(self.changed);
        let unanswered = matches!(record.last_role.as_deref(), Some("user" | "toolResult"));
        (working && running && unanswered && still >= self.stall).then(|| {
            KeptGone::NotMoving(format!(
                "its turn did not move for {} s, the model's answer never came",
                still.as_secs()
            ))
        })
    }
}

/// How often [`OneSession::keep_open`] looks again once the pane is open
/// (each look asks the gateway): to see the default engine change.
pub const KEEP_OPEN_WHILE_OPEN: Duration = Duration::from_secs(30);
/// How often [`OneSession::keep_open`] looks whether the pane can be opened.
pub const KEEP_OPEN_EVERY: Duration = Duration::from_secs(2);
/// With no agent ready (a fresh or signed-out lamp, OpenClaw not set up),
/// the lamp is looked at again after this, doubled each time up to
/// [`NOT_READY_MOST`], or at once when a trigger fires (Harness's daemon
/// started, the lamp's machine or default engine changed).
pub const NOT_READY_FIRST: Duration = Duration::from_secs(60);

/// The kept session left with no task this long is dropped as at a
/// sign-out (closed, its OpenClaw session deleted, its token removed) and a
/// fresh one opened: OpenClaw's kept session is never rotated by count, so
/// this bounds how long one conversation lives.
pub const KEPT_IDLE_MAX: Duration = Duration::from_secs(2 * 60 * 60);
/// The longest wait between two looks with no agent ready.
pub const NOT_READY_MOST: Duration = Duration::from_secs(600);
/// What a run refused with a full queue is told, answered as lamp-tasks'
/// `busy` (so lamp-skills' retry, #7239, still applies).
pub const BUSY: &str =
    "the lamp's agent session is busy and its queue is full; one task runs at a time";

/// The one header line each task's turn starts with.
pub fn header(id: &str) -> String {
    format!("New task {id}:")
}

/// What kind of work waits for the pane: spoken tasks go ahead of skill
/// runs in the queue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Spoken,
    Skill,
}

/// Where the pane is, for a run: the lamp's own machine, the default
/// engine, and the pane's folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub machine: String,
    pub engine: EngineKind,
    pub folder: String,
}

impl Place {
    /// The same machine and engine: the only fields that move the pane.
    /// Another folder keeps it.
    pub fn same_spot(&self, other: &Place) -> bool {
        self.machine == other.machine && self.engine == other.engine
    }
}

/// Harness as the pane needs it; [`Cli`] on a lamp, a fake in tests.
pub trait PaneHarness {
    fn open(&self, launch: &Launch) -> LaunchOutcome;
    fn type_into(&self, machine: &str, agent: &str, words: &str) -> Typed;
    /// `harness wait --any` on the pane: the state it ended in, `None`
    /// when Harness's own hold passed.
    fn wait_end(
        &self,
        machine: &str,
        agent: &str,
        cancel: &AtomicBool,
    ) -> Result<Option<String>, String>;
    /// `show` of the pane; `Err("gone")` when Harness no longer has it.
    fn observe(&self, machine: &str, agent: &str) -> Result<PaneSeen, String>;
    fn close_now(&self, machine: &str, agent: &str) -> Result<Closed, String>;
    /// `harness resume` of a stopped or gone pane: its state after.
    fn resume(&self, machine: &str, agent: &str) -> Result<String, String>;
    /// The agents `ps` lists live in `folder` on `machine`.
    fn live_in(&self, machine: &str, folder: &str) -> Result<Vec<String>, String>;
    /// `harness capabilities`: whether Harness keeps program sessions
    /// (openharness#4393); `None` when it could not be asked. The defaults
    /// below are an older Harness's, which has none.
    fn program_sessions(&self) -> Option<bool> {
        Some(false)
    }
    /// `harness ensure` of the kept session `name` ([`Cli::ensure_kept`]).
    fn ensure_kept(
        &self,
        _engine: EngineKind,
        _name: &str,
        _token: &Path,
        _folder: &str,
        _fresh: bool,
    ) -> Result<Ensured, String> {
        Err("unsupported".into())
    }
    /// An acknowledged `harness reply` into it ([`Cli::reply_kept`]).
    fn reply_kept(&self, _name: &str, _token: &Path, _words: &str) -> Took {
        Took::NotSent("unsupported".into())
    }
    /// `harness wait --after` an input it took ([`Cli::wait_kept`]).
    fn wait_kept(
        &self,
        _name: &str,
        _token: &Path,
        _input: &str,
        _cancel: &AtomicBool,
    ) -> Result<Option<String>, String> {
        Err("unsupported".into())
    }
    /// `harness wait --after` an input a pane of `harness new` took, no
    /// token ([`Cli::wait_after`]).
    fn wait_after(
        &self,
        _machine: &str,
        _agent: &str,
        _input: &str,
        _cancel: &AtomicBool,
    ) -> Result<Option<String>, String> {
        Err("unsupported".into())
    }
    /// `harness close` of it with the token ([`Cli::close_kept`]), `--now`
    /// when `now` (a turn cancelled or at its deadline, the session maybe
    /// still working); its conversation stays in the engine's store.
    fn close_kept(&self, _name: &str, _token: &Path, _now: bool) -> Result<(), String> {
        Err("unsupported".into())
    }
    /// Whether OpenClaw's own record still holds the session `key`
    /// exactly (the kept record's `engineSession`,
    /// [`crate::harness::openclaw_holds_key`], read only, no CLI start);
    /// `None` when it can't be read, which is never taken as gone.
    fn session_held(&self, _key: &str) -> Option<bool> {
        None
    }
    /// #7906 sign 4: a reader of where the Gateway's record is in the
    /// session `key`'s turn ([`Cli::openclaw_turn`], matched exactly as
    /// [`PaneHarness::session_held`] is), callable from the thread that
    /// watches a turn beside its `harness wait`; `None` with no record to
    /// read.
    fn openclaw_turns<'a>(&'a self, _key: &str) -> Option<TurnReader<'a>> {
        None
    }
}

/// Whether `show`'s state (or its error) says the pane is gone: reopened,
/// never waited on. `offline`, `unknown`, `starting` and `installing` are
/// waited on instead.
fn gone_state(state: &str) -> bool {
    matches!(state, "stopped" | "failed" | "gone" | "closed")
}

impl PaneHarness for Cli {
    fn open(&self, launch: &Launch) -> LaunchOutcome {
        self.launch(launch)
    }
    fn type_into(&self, machine: &str, agent: &str, words: &str) -> Typed {
        self.reply_to(machine, agent, words)
    }
    fn wait_end(
        &self,
        machine: &str,
        agent: &str,
        cancel: &AtomicBool,
    ) -> Result<Option<String>, String> {
        self.wait(machine, &[agent.to_owned()], cancel)
            .map(|ended| ended.into_iter().next().map(|ended| ended.state))
    }
    fn observe(&self, machine: &str, agent: &str) -> Result<PaneSeen, String> {
        Cli::observe_pane(self, machine, agent)
    }
    fn close_now(&self, machine: &str, agent: &str) -> Result<Closed, String> {
        Cli::close_pane_now(self, machine, agent, crate::harness::CLOSE_BUDGET)
    }
    fn resume(&self, machine: &str, agent: &str) -> Result<String, String> {
        Cli::resume_pane(self, machine, agent)
    }
    fn live_in(&self, machine: &str, folder: &str) -> Result<Vec<String>, String> {
        Cli::live_in(self, machine, folder)
    }
    fn program_sessions(&self) -> Option<bool> {
        Cli::program_sessions(self)
    }
    fn ensure_kept(
        &self,
        engine: EngineKind,
        name: &str,
        token: &Path,
        folder: &str,
        fresh: bool,
    ) -> Result<Ensured, String> {
        Cli::ensure_kept(self, engine, name, token, folder, fresh)
    }
    fn reply_kept(&self, name: &str, token: &Path, words: &str) -> Took {
        Cli::reply_kept(self, name, token, words)
    }
    fn wait_kept(
        &self,
        name: &str,
        token: &Path,
        input: &str,
        cancel: &AtomicBool,
    ) -> Result<Option<String>, String> {
        Cli::wait_kept(self, name, token, input, cancel)
    }
    fn close_kept(&self, name: &str, token: &Path, now: bool) -> Result<(), String> {
        Cli::close_kept(self, name, token, now)
    }
    fn wait_after(
        &self,
        machine: &str,
        agent: &str,
        input: &str,
        cancel: &AtomicBool,
    ) -> Result<Option<String>, String> {
        Cli::wait_after(self, machine, agent, input, cancel)
    }
    fn session_held(&self, key: &str) -> Option<bool> {
        Cli::openclaw_holds(self, key)
    }
    fn openclaw_turns<'a>(&'a self, key: &str) -> Option<TurnReader<'a>> {
        let key = key.to_owned();
        Some(Box::new(move || Cli::openclaw_turn(self, &key)))
    }
}

/// The pane as it was opened.
#[derive(Clone, Debug)]
struct Pane {
    agent: String,
    place: Place,
    /// Turns typed into it, its open's own not counted.
    turns: u64,
    /// Tokens estimated typed and read, 4 characters a token
    /// ([`ROTATE_TOKENS`]).
    tokens: u64,
    /// The lamp's kept session (`harness ensure`, openharness#4393), not
    /// a pane of `harness new`.
    kept: bool,
    /// The input the kept session last took (`harness reply`'s `inputId`),
    /// for `harness wait --after`.
    input: Option<String>,
    /// The kept session's engine session (`harness ensure`'s
    /// `engineSession`), deleted from OpenClaw's store when it is closed.
    session: Option<String>,
    /// Its open, or the last turn typed into it ([`KEPT_IDLE_MAX`]).
    used: Instant,
    /// Near its model's context ([`near_context_limit`], #7906): replaced
    /// by a fresh session at the next open, as at a rotation.
    full: bool,
    /// Its open in this process already knows the Gateway holds it, or
    /// may not yet (#7906 sign 3): `ensure` made it new, or the kept
    /// record was read held just before `ensure`. Its first hand-over is
    /// not looked at again.
    checked_at_open: bool,
    /// The last turn's hand-over and the Gateway's record then (#7906
    /// sign 4).
    handed: Option<(Instant, Option<(String, i64)>)>,
}
impl Pane {
    fn new(agent: &str, place: &Place, kept: bool) -> Self {
        Pane {
            agent: agent.to_owned(),
            place: place.clone(),
            turns: 0,
            tokens: 0,
            kept,
            input: None,
            session: None,
            used: Instant::now(),
            full: false,
            checked_at_open: false,
            handed: None,
        }
    }

    /// Usable as it is at `place`: there, no rotation due and not near its
    /// context's limit.
    fn fresh_at(&self, place: &Place) -> bool {
        self.place.same_spot(place)
            && !self.full
            && !rotation_due(self.place.engine, self.turns, self.tokens)
    }
}

#[derive(Default)]
struct Inner {
    pane: Option<Pane>,
    /// The lamp signed out of Harness at the last look (#6407: the ship
    /// lamp runs signed out for good); kept with the pane's id.
    signed_out: bool,
    /// The work holding the pane now (a run's id, or the open's).
    holder: Option<String>,
    waiting: VecDeque<(String, Kind)>,
    /// Opens failed in a row, and when the next may be tried.
    failed_opens: u32,
    next_open: Option<Instant>,
    /// Why the pane is to be reopened, for the next open's journal line.
    reopen_why: Option<String>,
    /// The lamp's place as last read ([`OneSession::knows`]).
    home: Option<Place>,
    /// The lamp's machine and default engine as last looked at
    /// ([`lamp_key`]), for [`OneSession::waits_for_place`]; `None` before
    /// any look said.
    lamp_key: Option<String>,
    /// The last `harness new` was uncertain or pending: Harness may have
    /// made the pane, so the next open looks for it in its folder before
    /// any other `new`.
    unsure_open: bool,
    /// The holder is a spoken task the store still has: released by the
    /// store once that task is spent ([`OneSession::release`]).
    held_spoken: bool,
    /// The pane to be removed for good, and why (a sign-out): done at the
    /// next [`OneSession::tend`], which has Harness to close it with.
    drop_due: Option<String>,
    /// The due removal's close is `--now`: its session may still be
    /// working (a turn cancelled, cut at its deadline or unanswered), and
    /// Harness refuses a plain close then (#7245).
    drop_now: bool,
    /// The due removal keeps the kept session's token: a stuck sign's or a
    /// cancel's close that failed (#7906). A new token on the same name
    /// could be refused `NAME_TAKEN`; the next `ensure` is `--fresh`.
    drop_keeps_token: bool,
    /// A removal under way ([`OneSession::drop_pane_as`]): the pane taken,
    /// its close, its OpenClaw session's delete and its token's removal
    /// not all done yet.
    dropping: bool,
    /// The spoken task whose turn ended unanswered (cancelled, or an ask
    /// no one answered): it holds the pane until the drop closes it.
    closing_for: Option<String>,
    /// The spoken tasks the store keeps queued for the pane, in order:
    /// a skill run never goes before them.
    spoken_queue: Vec<String>,
    /// The spoken task whose words found the kept session not running,
    /// and how many times ([`STARTING_RESUMES_MOST`], #7895).
    unstarted: Option<(String, u32)>,
    /// Counted at each [`OneSession::forget`]: [`OneSession::keep_open`]
    /// looks at the lamp again at once.
    woken: u64,
    /// What `harness capabilities` said of program sessions, asked at the
    /// first open with [`KEPT_SWITCH`] on and again after Harness's daemon
    /// started again (an update among the reasons); `None` before.
    programs: Option<bool>,
    /// `harness ensure` said the engine was not ready, in a row: the next
    /// open after [`NOT_READY_FIRST`], doubled up to [`NOT_READY_MOST`].
    not_ready_opens: u32,
    /// Stuck sessions replaced in a row ([`REPLACE_MOST`]).
    replaced_in_row: u32,
    /// The kept session given its one plain resume (#7906 step a), until a
    /// turn is answered: died again, it is replaced fresh.
    revived: Option<String>,
}

/// [`OneSession::keep_open`]'s own: when it looks at the lamp next, and
/// what it saw last.
#[derive(Default)]
pub struct Looks {
    next: Option<Instant>,
    gap: Duration,
    key: Option<Option<String>>,
    woken: u64,
    why: Option<String>,
}

/// A [`lamp_key`] ("<machine> <engine>") with both halves known, else
/// `None`: a key read unknown is never a move.
fn known_key(key: Option<&String>) -> Option<(&str, &str)> {
    key.and_then(|key| key.split_once(' '))
        .filter(|(machine, engine)| !machine.is_empty() && !engine.is_empty())
}

/// Whether a default engine's runs share the one pane: OpenClaw only. A
/// Hermes default keeps today's per-run path (#7246, until H1 is shown on
/// a Hermes lamp).
pub fn shares(engine: EngineKind) -> bool {
    engine == EngineKind::Openclaw
}

/// Deletes an OpenClaw session from OpenClaw's own store by its key.
pub type SessionDelete = Box<dyn Fn(&str) -> Result<(), String> + Send + Sync>;
/// Where the lamp's pane is to be: `None`, nowhere (no Harness set up);
/// `Some((cli, None))`, a pane held is to be removed.
pub type PlaceFor = Box<dyn Fn() -> Option<(Cli, Option<Place>)> + Send>;

/// The lamp's one standing pane and its queue.
///
/// Compaction is the engine's own; a bound, if one is needed, follows #7246.
pub struct OneSession {
    inner: Mutex<Inner>,
    turned: Condvar,
    state_file: Option<PathBuf>,
    /// Deletes an OpenClaw session from OpenClaw's own store by its key
    /// ([`delete_openclaw_session`] on a lamp).
    delete_session: SessionDelete,
    queue_wait: Duration,
    look: Duration,
    open_idle_within: Duration,
    /// [`KEPT_IDLE_MAX`], shorter in tests.
    kept_idle_max: Duration,
    /// A spoken task's wait for the pane on the tasks-harness worker, and
    /// lamp-tasks' stop ([`OneSession::with_spoken_bound`]).
    spoken_bound: Option<(Duration, Arc<AtomicBool>)>,
    /// Harness refused a reply with `REPLY_OFF` (the person's
    /// `programs-reply` setting off, as on a fresh lamp): every task from
    /// then on goes the per-task way, until lamp-tasks starts again
    /// ([`OneSession::takes_reply`]).
    no_reply: AtomicBool,
    /// [`KEPT_SWITCH`] on: the kept session's token file ([`TOKEN_FILE`]).
    kept_token: Option<PathBuf>,
    /// Called with why each time a stuck session is replaced
    /// ([`OneSession::with_on_replaced`]).
    on_replaced: Option<Replaced>,
    /// OpenClaw's agent database, read for the pane's size before a turn
    /// ([`OneSession::start_fresh_if_near_limit`]); `None`: not read.
    openclaw_db: Option<PathBuf>,
    /// [`TURN_WATCH`], [`TURN_START_WITHIN`], [`TURN_STALL`]; shorter in
    /// tests ([`OneSession::with_turn_watch`]).
    turn_watch: (Duration, Duration, Duration),
    /// [`DEAD_WORKING`]; shorter in tests.
    dead_working: Duration,
}

/// What a stuck session's replacement calls with why (#7906's C: the
/// person's one told line is #7895's, not built here).
pub type Replaced = Box<dyn Fn(&str) + Send + Sync>;

/// Why a run can't have the pane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// The queue is full, or the run waited [`QUEUE_WAIT`]: [`BUSY`].
    Busy,
    /// The run's deadline or lamp-tasks' stop came while it waited.
    Ended,
}

/// How a turn's words reached the pane.
#[derive(Debug, PartialEq, Eq)]
pub enum Sent {
    /// Typed; the pane's `n`th turn.
    Typed(u64),
    /// Not typed into the pane, after one reopen, or the pane stayed busy
    /// until the run's end: the run fails, with why. Never a second
    /// session (the lead's design on #7246: one session, so one agent's
    /// memory, on the lamp).
    NotTyped(String),
    /// Harness may have typed them: never typed again, the run fails.
    Uncertain(String),
    /// Harness takes no reply here (`reply_off`: the person's
    /// `programs-reply` setting off): nothing typed, so the task goes the
    /// per-task way, its own `harness new` with its words as `--task`,
    /// as with the switch off ([`OneSession::takes_reply`]).
    NoReply,
}

/// The lamp's one session, set once by `runtime::serve` with the switch
/// on, so the owner loop can let the pane go when Harness's daemon is
/// started again ([`forget_lamp`]).
pub static LAMP: std::sync::OnceLock<Arc<OneSession>> = std::sync::OnceLock::new();

/// [`OneSession::forget`] on the lamp's one session, if there is one.
pub fn forget_lamp(why: &str) {
    if let Some(one) = LAMP.get() {
        one.forget(why);
    }
}

/// Harness's sign-in read signed out after signed in
/// ([`crate::skill_task::harness_sign_in_read`]): the lamp's pane removed
/// at the next look, closed `--now`, its OpenClaw session and kept id
/// deleted. A factory reset needs nothing here: it wipes the store folder
/// (the kept id), `harnesses/lamp-tasks`, `.harness/cli/data` and
/// OpenClaw's state (`crates/setup/fixtures/reset-plan-product.json:16-22`,
/// `crates/setup/src/reset.rs:734`).
pub fn signed_out() {
    if let Some(one) = LAMP.get() {
        eprintln!("tasks: one session: the lamp signed out; its pane is removed");
        one.drop_when_free("the lamp signed out");
    }
}

/// The owner's "forget everything" (#6274) on the lamp's one session.
pub fn forgotten() {
    if let Some(one) = LAMP.get() {
        one.forget_conversation();
    }
}

/// Where the lamp's pane would be now, for [`OneSession::keep_open`]: only
/// with Harness set up (`slot`), the lamp's own target known, and its
/// default agent a gateway agent's engine (OpenClaw: a Hermes default's
/// runs stay per run in phase 1) whose gateway answers now; its folder
/// [`FOLDER`] in `project`, made 0700. Else why not.
/// How long [`lamp_place`] waits for the Gateway to serve its approvals
/// read: one read's bound (`gateway`'s 20 s) plus a margin.
pub const LAMP_PLACE_GATEWAY_WITHIN: Duration = Duration::from_secs(25);

/// With the kept session in use (`one` has [`KEPT_SWITCH`] on and Harness
/// keeps program sessions), the gateway's approvals read is skipped:
/// `harness ensure` and `reply` wait for the engine themselves and answer
/// `ENGINE_NOT_READY`, and that read alone took 17 s on a loaded lamp, past
/// its bound, so the kept session never opened (openharness#4393).
pub fn lamp_place(
    slot: &crate::skill_task::SkillHarnessSlot,
    project: &std::path::Path,
    one: Option<&OneSession>,
) -> Result<(Cli, Place), String> {
    use std::os::unix::fs::DirBuilderExt;
    let harness = slot.get().ok_or("Harness is not set up")?;
    // Opened as the person only: Harness closes an idle pane an agent
    // started after 30 min (see the module's notes).
    if !harness.config.as_person {
        return Err("Harness is called as an agent".into());
    }
    let lamp = (harness.lamp)().ok_or("the lamp's own machine is not known (signed out?)")?;
    let engine = (harness.default_engine)().unwrap_or(lamp.engine);
    if !shares(engine) {
        return Err(format!(
            "the default engine is {}, not OpenClaw",
            engine.argument()
        ));
    }
    let cli = Cli::new(harness.config.clone(), harness.session.clone())
        .map_err(|error| format!("Harness can't be asked: {error}"))?;
    let kept = one.is_some_and(|one| one.kept_token(&cli).is_some());
    // The Gateway's approvals read takes about 10 s on a lamp even when warm
    // (`gateway::Serving`), and a success is kept per Gateway start, so this
    // bound covers one read; it runs on the keep-open thread, never on a
    // hand-over's way. A 1 s bound could never see a read through.
    if !kept && let Some(mut gateway) = (harness.gateway)() {
        gateway
            .before_launch_until(Instant::now() + LAMP_PLACE_GATEWAY_WITHIN, &|| false)
            .map_err(|why| format!("OpenClaw's gateway is not ready: {why}"))?;
    }
    let folder = project.join(FOLDER);
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&folder)
        .map_err(|error| format!("its folder not made: {error}"))?;
    let folder = folder.to_str().ok_or("its folder is not a plain path")?;
    Ok((
        cli,
        Place {
            machine: lamp.machine_id,
            engine,
            folder: folder.to_owned(),
        },
    ))
}

/// What [`OneSession::keep_open`] watches cheaply for a change between
/// looks: Harness set up, the lamp's machine and its default engine.
pub fn lamp_key(slot: &crate::skill_task::SkillHarnessSlot) -> Option<String> {
    let harness = slot.get()?;
    let lamp = (harness.lamp)();
    let engine = (harness.default_engine)().or(lamp.as_ref().map(|lamp| lamp.engine));
    Some(format!(
        "{} {}",
        lamp.map_or_else(String::new, |lamp| lamp.machine_id),
        engine.map_or("", EngineKind::argument)
    ))
}

/// Whether the store's `launch` is a spoken task for the lamp's pane:
/// the lamp's own machine and default engine, as the pane open now is,
/// and no session the person named.
pub fn for_the_pane(one: &OneSession, launch: &Launch) -> Option<Place> {
    // Harness takes no reply here: every task the per-task way.
    if !one.takes_reply() {
        return None;
    }
    let place = one.place().or_else(|| one.lock().home.clone())?;
    (shares(place.engine)
        && launch.session.is_none()
        && launch.target.machine_id == place.machine
        && launch.target.engine == place.engine)
        .then_some(place)
}

/// Spoken task `name` found the session not up past
/// [`STARTING_RESUMES_MOST`] (#7895): `not_sent` [`NOT_STARTED`], which the
/// store ends once and Task news tells once; its count spent with it.
fn not_started(one: &OneSession, name: &str) -> LaunchOutcome {
    one.lock().unstarted = None;
    eprintln!(
        "tasks: {name}: the lamp's session did not come up after {STARTING_RESUMES_MOST} tries; not sent ({NOT_STARTED})"
    );
    LaunchOutcome::NotSent {
        code: NOT_STARTED.into(),
    }
}

/// A spoken task typed into the lamp's pane (#7246): its place taken in
/// the queue (spoken first; the store holds it queued while the pane is
/// busy), its words typed after its header line once the pane's last were
/// taken, and the pane held until the store has its answer. `Joined`: the
/// store watches the pane as any session the person already had, and
/// never closes it. `None`: the per-run way, only when the task is not
/// for the lamp's shared engine (or the lamp's place was never known: no
/// agent ready, A3's guard), or Harness takes no reply here (`reply_off`,
/// [`OneSession::takes_reply`]). A pane not open yet is opened in the queue;
/// one that could not take it fails the task `not_sent`, never a second
/// session.
pub fn spoken_launch(
    one: &OneSession,
    harness: &dyn PaneHarness,
    launch: &Launch,
) -> Option<LaunchOutcome> {
    let place = for_the_pane(one, launch)?;
    // The pane's opens failed past the bound while it waited (#7895).
    if one.not_up_spent(&launch.name) {
        return Some(not_started(one, &launch.name));
    }
    let began = Instant::now();
    let (within, stop) = match &one.spoken_bound {
        Some((within, stop)) => ((*within).min(one.queue_wait), Some(stop)),
        None => (one.queue_wait, None),
    };
    let until = began + within;
    let ended = || stop.is_some_and(|stop| stop.load(Ordering::Acquire));
    let mut turn = match one.enter(&launch.name, Kind::Spoken, until, &ended) {
        Ok(turn) => turn,
        Err(Refused::Busy | Refused::Ended) => {
            return Some(LaunchOutcome::NotSent {
                code: "busy".into(),
            });
        }
    };
    // Never an open or a rotation here (the Reviewer-lead's #3 on #7258):
    // with no fresh pane, `busy`, put back, and the one-session thread
    // opens it.
    turn.opens = false;
    match turn.send(harness, &place, &launch.prompt, until, &ended) {
        Sent::Typed(_) => {
            // Typed: never the per-run way after this, even if the pane
            // was let go meanwhile, or the task would run twice.
            let Some(agent) = one.agent() else {
                return Some(LaunchOutcome::Uncertain {
                    code: "one_session_let_go".into(),
                });
            };
            // The store reads the pane from now on: an idle read before
            // the agent took these words would be the last task's answer.
            // So `Joined` only once the words were taken.
            // `Joined` only once its words were seen taken; else (the
            // bound, lamp-tasks' stop) uncertain, never typed again.
            // Bounded on the worker (Heron's P2), but from the send, not
            // the launch's start: on the lamp the send alone (a `show`
            // and the `reply`) takes 5-7 s, which spent the 5 s spoken
            // bound before any look (c0a4, 16 Nov 16:56). Four spoken
            // bounds (20 s on the lamp), still far below the old 120 s.
            let taken_until = Instant::now() + within.saturating_mul(4);
            let past = || ended() || Instant::now() >= taken_until;
            // The kept session's reply returns only once the engine took
            // the words (openharness#4393): no look needed.
            if !one.took_input(&agent)
                && !one.wait_taken_while(harness, &place.machine, &agent, &launch.name, &past)
            {
                eprintln!(
                    "tasks: {}: its words were not seen taken in {} ms",
                    launch.name,
                    began.elapsed().as_millis()
                );
                turn.keep();
                return Some(LaunchOutcome::Uncertain {
                    code: "one_session_not_taken".into(),
                });
            }
            eprintln!(
                "tasks: {}: turn accepted in {} ms ({})",
                launch.name,
                began.elapsed().as_millis(),
                nth(one.turns())
            );
            turn.keep();
            Some(LaunchOutcome::Joined {
                agent_id: agent,
                session: None,
            })
        }
        Sent::Uncertain(code) => {
            turn.keep();
            Some(LaunchOutcome::Uncertain { code })
        }
        // Its resumes spent (#7895): failed, so the store ends it and Task
        // news tells the asker once; never put back again.
        Sent::NotTyped(why) if why == STARTING_SPENT => Some(not_started(one, &launch.name)),
        // `busy`, as a full queue: never a second session.
        Sent::NotTyped(_) => Some(LaunchOutcome::NotSent {
            code: "busy".into(),
        }),
        // Harness takes no reply here: the task's own `harness new`, as
        // with the switch off (nothing was typed).
        Sent::NoReply => None,
    }
}

/// The OpenClaw session Harness's OpenClaw pane `agent` keeps in
/// OpenClaw's own store.
pub fn openclaw_session_key(agent: &str) -> String {
    format!("agent:main:harness-{agent}")
}

/// How long the gateway's `sessions.delete` may take.
const DELETE_WITHIN: Duration = Duration::from_secs(15);

/// `openclaw gateway call sessions.delete` of `key` with its transcript,
/// run as OpenClaw's own calls are ([`crate::gateway::APPROVALS_ENV`]),
/// killed past [`DELETE_WITHIN`].
pub fn delete_openclaw_session(key: &str) -> Result<(), String> {
    use std::process::{Command, Stdio};
    let params = serde_json::json!({"key": key, "deleteTranscript": true}).to_string();
    let mut child = Command::new(crate::gateway::APPROVALS_READ[0])
        .args([
            "gateway",
            "call",
            "sessions.delete",
            "--json",
            "--timeout",
            "10000",
            "--params",
        ])
        .arg(params)
        .env_clear()
        .envs(crate::gateway::APPROVALS_ENV)
        .current_dir("/")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("openclaw not started: {error}"))?;
    let until = Instant::now() + DELETE_WITHIN;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(format!("openclaw ended {status}")),
            Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("openclaw gave no answer in time".into());
            }
            Err(error) => return Err(format!("openclaw not waited for: {error}")),
        }
    }
}

fn short(agent: &str) -> &str {
    agent.get(..8).unwrap_or(agent)
}

impl OneSession {
    /// The lamp's: the production bounds, its pane kept in
    /// [`STATE_FILE`] in lamp-tasks' state folder `dir`.
    pub fn lamp(dir: &std::path::Path) -> Self {
        Self::new(QUEUE_WAIT, TURN_LOOK, OPEN_IDLE_WITHIN)
            .with_state_file(dir.join(STATE_FILE))
            .with_openclaw_db(crate::harness::openclaw_record_path(Path::new(
                crate::runtime::OPENCLAW_STATE_DIR,
            )))
    }

    pub fn new(queue_wait: Duration, look: Duration, open_idle_within: Duration) -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
            turned: Condvar::new(),
            state_file: None,
            delete_session: Box::new(delete_openclaw_session),
            queue_wait,
            look,
            open_idle_within,
            kept_idle_max: KEPT_IDLE_MAX,
            spoken_bound: None,
            no_reply: AtomicBool::new(false),
            kept_token: None,
            on_replaced: None,
            openclaw_db: None,
            turn_watch: (TURN_WATCH, TURN_START_WITHIN, TURN_STALL),
            dead_working: DEAD_WORKING,
        }
    }

    /// A kept turn's progress read every `every`, stuck past `start_within`
    /// or `stall` ([`TurnWatch`]).
    pub fn with_turn_watch(
        mut self,
        every: Duration,
        start_within: Duration,
        stall: Duration,
    ) -> Self {
        self.turn_watch = (every, start_within, stall);
        self.dead_working = stall.min(DEAD_WORKING);
        self
    }

    fn dead_within(&self) -> Duration {
        self.dead_working
    }

    /// `replaced` called with why each time a stuck session is replaced:
    /// the hook for #7895's one told line to the person.
    pub fn with_on_replaced(mut self, replaced: Replaced) -> Self {
        self.on_replaced = Some(replaced);
        self
    }

    /// The pane's size is read from OpenClaw's agent database at `db`
    /// ([`crate::harness::openclaw_record_path`]) before each turn (#7906).
    pub fn with_openclaw_db(mut self, db: PathBuf) -> Self {
        self.openclaw_db = Some(db);
        self
    }

    /// Before a turn (#7906): the open pane's conversation measured in
    /// OpenClaw's own record ([`crate::harness::openclaw_session_size`];
    /// unreadable, the pane's own token count instead) and, past
    /// [`near_context_limit`] for a make (`make`) or another task, marked
    /// full, so the next open replaces it as at a rotation: the kept
    /// session with `harness ensure --fresh` and its old OpenClaw session
    /// deleted, a pane of `harness new` closed and opened anew. Whether it
    /// was marked. One read of a local SQLite file, never under the lock.
    pub fn start_fresh_if_near_limit(&self, place: &Place, make: bool) -> bool {
        let (agent, tokens) = {
            let inner = self.lock();
            match inner.pane.as_ref() {
                Some(pane)
                    if pane.place.same_spot(place)
                        && pane.place.engine == EngineKind::Openclaw
                        && !pane.full =>
                {
                    (pane.agent.clone(), pane.tokens)
                }
                _ => return false,
            }
        };
        let began = Instant::now();
        let size = match &self.openclaw_db {
            Some(db) => {
                crate::harness::openclaw_session_size(db, &self.kept_session_key(&agent), &agent)
            }
            None => Err("no OpenClaw database".into()),
        };
        let read_ms = began.elapsed().as_millis();
        let size = match size {
            Ok(size) => Some(size),
            Err(why) => {
                if self.openclaw_db.is_some() {
                    eprintln!(
                        "tasks: one session: {}'s size not read ({why}); its own ~{tokens} tokens weighed",
                        short(&agent)
                    );
                }
                None
            }
        };
        if !near_context_limit(size, tokens, make) {
            return false;
        }
        let mut inner = self.lock();
        let Some(pane) = inner.pane.as_mut().filter(|pane| pane.agent == agent) else {
            return false;
        };
        pane.full = true;
        eprintln!(
            "tasks: one session: {} is near its context ({}) before {}: a fresh session (read in {read_ms} ms)",
            short(&agent),
            size.map_or_else(
                || format!("~{tokens} tokens counted"),
                |size| format!("{} messages, ~{} tokens", size.messages, size.bytes / 4)
            ),
            if make { "a make" } else { "a task" }
        );
        true
    }

    /// [`KEPT_SWITCH`] on: the pane is the lamp's kept session, its token
    /// in `token` ([`TOKEN_FILE`]), once Harness keeps program sessions.
    pub fn with_kept_session(mut self, token: PathBuf) -> Self {
        self.kept_token = Some(token);
        self
    }

    /// The kept session's token file, when [`KEPT_SWITCH`] is on and
    /// Harness keeps program sessions: asked once (`harness capabilities`),
    /// and again after Harness's daemon started again. An older Harness, or
    /// one that could not be asked: `None`, and the pane as before.
    fn kept_token(&self, harness: &dyn PaneHarness) -> Option<PathBuf> {
        let file = self.kept_token.as_ref()?;
        let known = self.lock().programs;
        let programs = match known {
            Some(programs) => programs,
            None => {
                let asked = harness.program_sessions();
                if let Some(programs) = asked {
                    self.lock().programs = Some(programs);
                    eprintln!(
                        "tasks: one session: {}",
                        if programs {
                            "Harness keeps program sessions; the pane is the kept session (openharness#4393)"
                        } else {
                            "Harness keeps no program sessions; the pane as before"
                        }
                    );
                }
                asked.unwrap_or(false)
            }
        };
        programs.then(|| file.clone())
    }

    /// The kept session's name and token when `agent` is it.
    fn kept_now(&self, agent: &str) -> Option<(String, PathBuf)> {
        let token = self.kept_token.clone()?;
        let inner = self.lock();
        let pane = inner.pane.as_ref()?;
        (pane.kept && pane.agent == agent).then(|| (name_for(pane.place.engine), token))
    }

    /// The pane `agent` took this turn's words with an acknowledged reply
    /// (openharness#4393): its input id is kept.
    fn took_input(&self, agent: &str) -> bool {
        self.lock()
            .pane
            .as_ref()
            .is_some_and(|pane| pane.agent == agent && pane.input.is_some())
    }

    /// The kept session's token file removed (a sign-out, an engine
    /// switch): the next kept session gets a token of its own.
    fn forget_token(&self) {
        if let Some(file) = &self.kept_token
            && let Err(error) = std::fs::remove_file(file)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!(
                "tasks: one session: {} not removed: {error}",
                file.display()
            );
        }
    }

    /// The kept record ([`STATE_FILE`]) says the lamp's kept session, not a
    /// pane of `harness new`.
    fn kept_is_program(&self) -> bool {
        let Some(file) = &self.state_file else {
            return false;
        };
        std::fs::read_to_string(file)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .is_some_and(|kept| kept["kept"].as_bool() == Some(true))
    }

    /// The kept session `agent`'s OpenClaw session key: `harness ensure`'s
    /// `engineSession`, from the pane or the kept record, else Harness's
    /// usual `agent:main:harness-<id>`.
    fn kept_session_key(&self, agent: &str) -> String {
        let from_pane = self
            .lock()
            .pane
            .as_ref()
            .filter(|pane| pane.agent == agent)
            .and_then(|pane| pane.session.clone());
        from_pane
            .or_else(|| {
                let text = std::fs::read_to_string(self.state_file.as_ref()?).ok()?;
                let kept: serde_json::Value = serde_json::from_str(&text).ok()?;
                (kept["agent"] == agent)
                    .then(|| kept["session"].as_str().map(str::to_owned))
                    .flatten()
            })
            .unwrap_or_else(|| openclaw_session_key(agent))
    }

    /// The kept session `agent` closed by us: its record forgotten, and
    /// its conversation deleted from OpenClaw's own store by its engine
    /// session, since Harness's close keeps it (`close --forget` is not
    /// built yet, openharness#4393).
    fn kept_closed(&self, agent: &str, engine: EngineKind) {
        let key = self.kept_session_key(agent);
        self.closed_by_us(agent, engine, false);
        if engine == EngineKind::Openclaw
            && let Err(code) = (self.delete_session)(&key)
        {
            eprintln!(
                "tasks: one session: the kept session {}'s OpenClaw session not deleted: {code}",
                short(agent)
            );
        }
    }

    /// Harness says the kept session is not ours (`NAME_TAKEN`,
    /// `NOT_YOURS`): the pane the old way until Harness's daemon starts
    /// again, and the kept record forgotten (lamp-tasks can't touch it).
    fn not_ours(&self, code: &str) {
        self.lock().programs = Some(false);
        if self.kept_is_program()
            && let Some(file) = &self.state_file
        {
            let _ = std::fs::remove_file(file);
        }
        eprintln!("tasks: one session: Harness says {code}; the pane the old way next");
    }

    /// Whether the pane may be typed into: false once Harness refused a
    /// reply with `REPLY_OFF` (the person's `programs-reply` setting off),
    /// until lamp-tasks starts again. Then every task goes the per-task
    /// way, as with the switch off, and no pane is opened. Signed in or
    /// out alike (#6407).
    pub fn takes_reply(&self) -> bool {
        !self.no_reply.load(Ordering::Acquire)
    }

    /// Harness refused a reply with `REPLY_OFF`: noted for the process's
    /// life, journaled the first time only. The pane, unused from now on,
    /// is removed as at a sign-out ([`OneSession::drop_when_free`]: closed
    /// `--now` at the keep-open thread's next look, nothing holding it),
    /// and no other is opened ([`OneSession::open_if_due_while`]).
    fn no_reply_here(&self) {
        if !self.no_reply.swap(true, Ordering::AcqRel) {
            eprintln!(
                "tasks: one session: Harness takes no reply here (reply_off); each task gets its own pane"
            );
            self.drop_when_free("Harness takes no reply here (reply_off)");
        }
    }

    /// A spoken task waits at most `within` for the pane in
    /// [`spoken_launch`], and not past `stop`: it runs on the tasks-harness
    /// worker, which serves every hand-over and read.
    pub fn with_spoken_bound(mut self, within: Duration, stop: Arc<AtomicBool>) -> Self {
        self.spoken_bound = Some((within, stop));
        self
    }

    /// The pane kept in `file` ([`STATE_FILE`]): read at an open, written
    /// at each `new`.
    pub fn with_state_file(mut self, file: PathBuf) -> Self {
        self.state_file = Some(file);
        self
    }

    /// OpenClaw sessions deleted by `delete` in place of the gateway (tests).
    pub fn with_session_delete(mut self, delete: SessionDelete) -> Self {
        self.delete_session = delete;
        self
    }

    /// The pane removed for good (#7246's privacy rules): the default
    /// engine changed away from it, or the lamp signed out. Closed
    /// `--now`, its conversation deleted from OpenClaw's own store
    /// (Harness's close keeps it there), and its kept id deleted, so no
    /// later start adopts it. A daemon restart is not this: Harness's
    /// restore brings the same pane back ([`OneSession::forget`]).
    pub fn drop_pane(&self, harness: &dyn PaneHarness, why: &str) {
        self.drop_pane_as(harness, why, true);
    }

    /// [`OneSession::drop_pane`]; for the kept session (openharness#4393)
    /// `harness close`, and its conversation deleted from OpenClaw's store
    /// by its engine session, and with
    /// `token` its token file too (a sign-out, an engine switch; not a turn
    /// that ended unanswered). A kept session not closed is tried again at
    /// the next look, never let go with its conversation still there.
    fn drop_pane_as(&self, harness: &dyn PaneHarness, why: &str, token: bool) {
        self.lock().dropping = true;
        self.drop_pane_now(harness, why, token);
        self.lock().dropping = false;
        // A task waiting in [`OneSession::enter`] for the removal goes on.
        self.turned.notify_all();
    }

    /// [`OneSession::drop_pane_as`]'s removal itself.
    fn drop_pane_now(&self, harness: &dyn PaneHarness, why: &str, token: bool) {
        let pane = self.lock().pane.take();
        let kept = self.kept();
        let program = pane
            .as_ref()
            .map_or_else(|| self.kept_is_program(), |pane| pane.kept);
        let (agent, machine, engine) = match (&pane, kept) {
            (Some(pane), _) => (
                pane.agent.clone(),
                pane.place.machine.clone(),
                pane.place.engine.argument().to_owned(),
            ),
            (None, Some(kept)) => kept,
            (None, None) => return,
        };
        if program
            && let Some(file) = &self.kept_token
            && let Some(kind) = engine_named(&engine)
        {
            let now = std::mem::take(&mut self.lock().drop_now);
            if let Err(code) = harness.close_kept(&name_for(kind), file, now) {
                eprintln!(
                    "tasks: one session: the kept session {} not closed: {code}; tried again next",
                    short(&agent)
                );
                let mut inner = self.lock();
                inner.drop_due = Some(why.to_owned());
                inner.drop_now |= now;
                return;
            }
            self.kept_closed(&agent, kind);
            if token {
                self.forget_token();
            }
            eprintln!(
                "tasks: one session: dropped the kept session {} ({why}); its conversation forgotten",
                short(&agent)
            );
            return;
        }
        match harness.close_now(&machine, &agent) {
            Ok(_) => {}
            Err(code) => eprintln!("tasks: one session: {} not closed: {code}", short(&agent)),
        }
        if engine == EngineKind::Openclaw.argument()
            && let Err(code) = (self.delete_session)(&openclaw_session_key(&agent))
        {
            eprintln!(
                "tasks: one session: {}'s OpenClaw session not deleted: {code}",
                short(&agent)
            );
        }
        if let Some(file) = &self.state_file
            && let Err(error) = std::fs::remove_file(file)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!(
                "tasks: one session: {} not removed: {error}",
                file.display()
            );
        }
        eprintln!("tasks: one session: dropped {} ({why})", short(&agent));
    }

    /// Whether the pane kept on disk is for another machine or engine
    /// than `key` ([`lamp_key`]: "<machine> <engine>"), both known.
    fn kept_elsewhere(&self, key: Option<&String>) -> bool {
        let Some((machine, engine)) = known_key(key) else {
            return false;
        };
        self.kept().is_some_and(|(_, kept_machine, kept_engine)| {
            kept_machine != machine || kept_engine != engine
        })
    }

    /// The owner asked to forget: the pane removed at the next look once
    /// free, as at a sign-out (the kept session closed, its OpenClaw
    /// session deleted, its token removed), and a new one opened right
    /// after it in the same look, so the next task recalls nothing and
    /// waits for no open.
    pub fn forget_conversation(&self) {
        eprintln!(
            "tasks: one session: the kept session dropped (the owner asked to forget); a new one opens"
        );
        let mut inner = self.lock();
        inner.drop_due = Some("the owner asked to forget".to_owned());
        inner.next_open = None;
        inner.failed_opens = 0;
        inner.not_ready_opens = 0;
        inner.woken = inner.woken.wrapping_add(1);
    }

    /// The pane to be removed for good at the next look (a sign-out:
    /// [`signed_out`]), with nothing held.
    pub fn drop_when_free(&self, why: &str) {
        self.lock().drop_due = Some(why.to_owned());
    }

    /// A removal asked for ([`OneSession::drop_when_free`]) done now, if
    /// nothing holds the pane.
    pub fn drop_if_due(&self, harness: &dyn PaneHarness) -> bool {
        let Some((why, unanswered)) = self.take_due() else {
            return false;
        };
        // A turn that ended unanswered keeps the kept session's token: only
        // a sign-out removes it.
        self.drop_pane_as(harness, &why, !unanswered);
        let mut inner = self.lock();
        if let Some(id) = inner.closing_for.take()
            && inner.holder.as_deref() == Some(id.as_str())
        {
            inner.holder = None;
            inner.held_spoken = false;
        }
        drop(inner);
        self.turned.notify_all();
        true
    }

    /// The removal asked for, taken if nothing holds the pane, and marked
    /// under way in the same lock: a task waiting in [`OneSession::enter`]
    /// that wakes between the two never sees neither (the drop due gone,
    /// the drop not yet marked) and is never typed into the old pane.
    fn take_due(&self) -> Option<(String, bool)> {
        let mut inner = self.lock();
        if inner.holder.is_some() && inner.holder != inner.closing_for {
            return None;
        }
        let why = inner.drop_due.take()?;
        inner.dropping = true;
        let keeps = std::mem::take(&mut inner.drop_keeps_token);
        Some((why, inner.closing_for.is_some() || keeps))
    }

    /// The pane kept: its agent, machine and engine, if any.
    fn kept(&self) -> Option<(String, String, String)> {
        let text = std::fs::read_to_string(self.state_file.as_ref()?).ok()?;
        let kept: serde_json::Value = serde_json::from_str(&text).ok()?;
        let field = |name: &str| kept[name].as_str().map(str::to_owned);
        Some((field("agent")?, field("machine")?, field("engine")?))
    }

    /// The kept pane was opened signed in. A file without the field (an
    /// older one) counts as signed in, so it is removed at a sign-out.
    fn kept_signed_in(&self) -> bool {
        let Some(file) = &self.state_file else {
            return false;
        };
        std::fs::read_to_string(file)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .is_some_and(|kept| kept["signed_in"].as_bool() != Some(false))
    }

    /// The pane `agent` at `place` kept, so the next start adopts it.
    fn keep(&self, agent: &str, place: &Place) {
        self.keep_as(agent, place, false);
    }

    /// [`OneSession::keep`]. The same pane already recorded keeps the
    /// sign-in it was opened under (an adopt right after a start, before
    /// the first sign-in read, says nothing of it), unless `signed_in`: a
    /// turn typed signed in ([`OneSession::used`]).
    fn keep_as(&self, agent: &str, place: &Place, signed_in: bool) {
        let Some(file) = &self.state_file else {
            return;
        };
        let recorded = self
            .kept()
            .is_some_and(|(kept, _, _)| kept == agent)
            .then(|| self.kept_signed_in());
        let (signed_in, program, session) = {
            let inner = self.lock();
            let pane = inner
                .pane
                .as_ref()
                .filter(|pane| pane.kept && pane.agent == agent);
            (
                signed_in || recorded.unwrap_or(!inner.signed_out),
                pane.is_some(),
                pane.and_then(|pane| pane.session.clone()),
            )
        };
        let text = serde_json::json!({
            "agent": agent,
            "machine": place.machine,
            "engine": place.engine.argument(),
            "signed_in": signed_in,
            "kept": program,
            "session": session,
        })
        .to_string();
        if let Err(error) = crate::hermes_config::replace(file, &text) {
            eprintln!(
                "tasks: one session: {} not kept in {}: {error}",
                short(agent),
                file.display()
            );
        }
    }

    /// A turn typed into the kept pane `agent` while signed in: a record
    /// that still says opened signed out is rewritten signed in, so a
    /// sign-out while lamp-tasks is down removes it at the next start (the
    /// lead's sign-out rule: a signed-in account's conversation goes at
    /// sign-out). Signed out, or already signed in: nothing written.
    fn used(&self, agent: &str, place: &Place) {
        if self.lock().signed_out
            || self.kept_signed_in()
            || self.kept().is_none_or(|(kept, _, _)| kept != agent)
        {
            return;
        }
        self.keep_as(agent, place, true);
    }

    /// The store's spoken tasks queued for the pane, in order (each owner
    /// turn): a skill run waits behind them, and each is told its place.
    pub fn set_spoken_queue(&self, names: Vec<String>) {
        let mut inner = self.lock();
        if inner.spoken_queue != names {
            inner.spoken_queue = names;
            drop(inner);
            self.turned.notify_all();
        }
    }

    /// The place of the store's spoken task `name` in the pane's queue,
    /// 1 the next, while something else holds the pane.
    pub fn queued_place(&self, name: &str) -> Option<usize> {
        let inner = self.lock();
        let at = inner
            .spoken_queue
            .iter()
            .position(|queued| queued == name)?;
        Some(
            at + 1
                + inner
                    .waiting
                    .iter()
                    .filter(|(_, kind)| *kind == Kind::Spoken)
                    .count(),
        )
    }

    /// Whether a spoken task may be launched into the pane now: nothing
    /// holds it.
    pub fn free(&self) -> bool {
        self.lock().holder.is_none()
    }

    /// The spoken task holding the pane until the store has its answer.
    pub fn spoken_holder(&self) -> Option<String> {
        let inner = self.lock();
        inner.held_spoken.then(|| inner.holder.clone()).flatten()
    }

    /// The spoken task `id`'s hold given up: the store has its answer, or
    /// the task is gone.
    pub fn release(&self, id: &str) {
        let mut inner = self.lock();
        if inner.held_spoken && inner.holder.as_deref() == Some(id) {
            inner.holder = None;
            inner.held_spoken = false;
            drop(inner);
            eprintln!("tasks: one session: {id}'s turn is done; the queue goes on");
            self.turned.notify_all();
        }
    }

    /// The spoken task `id`'s turn ended without its answer: cancelled,
    /// or an ask no one answered (Heron's H1 and H3 on #7258). The pane
    /// is still at it, or stuck in `needs-input`, so it stays held until
    /// the one-session thread closes it `--now` and deletes its
    /// conversation ([`OneSession::drop_if_due`]); the next look opens
    /// it again.
    pub fn release_unanswered(&self, id: &str) {
        let mut inner = self.lock();
        if inner.held_spoken && inner.holder.as_deref() == Some(id) && inner.closing_for.is_none() {
            inner.closing_for = Some(id.to_owned());
            inner.drop_due = Some(format!("{id}'s turn ended unanswered"));
            inner.drop_now = true;
            eprintln!("tasks: one session: {id}'s turn ended unanswered; the pane is closed next");
        }
    }

    /// A spoken task's answer, read by the store, counted toward the
    /// pane's rotation ([`ROTATE_TOKENS`]).
    pub fn count_answer(&self, answer: &str) {
        if let Some(pane) = self.lock().pane.as_mut() {
            pane.tokens += tokens_in(answer);
        }
    }

    /// A pane lamp-tasks closed itself: its kept id forgotten, so no later
    /// open adopts or resumes it; with `delete`, its OpenClaw session
    /// deleted from OpenClaw's own store too (Harness's close keeps it).
    fn closed_by_us(&self, agent: &str, engine: EngineKind, delete: bool) {
        if let Some(file) = &self.state_file
            && self.kept().is_some_and(|(kept, _, _)| kept == agent)
            && let Err(error) = std::fs::remove_file(file)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!(
                "tasks: one session: {} not removed: {error}",
                file.display()
            );
        }
        if delete
            && engine == EngineKind::Openclaw
            && let Err(code) = (self.delete_session)(&openclaw_session_key(agent))
        {
            eprintln!(
                "tasks: one session: {}'s OpenClaw session not deleted: {code}",
                short(agent)
            );
        }
    }

    /// The turns typed into the pane now.
    pub fn turns(&self) -> u64 {
        self.lock().pane.as_ref().map_or(0, |pane| pane.turns)
    }

    /// Until the words just typed for `id` were taken: the pane works on
    /// them, or has answered them (`told.taken`), within the first turn's
    /// bound; the end of `turn accepted`.
    pub fn wait_taken(&self, harness: &dyn PaneHarness, machine: &str, agent: &str, id: &str) {
        let _ = self.wait_taken_while(harness, machine, agent, id, &|| false);
    }

    /// [`OneSession::wait_taken`], ended by `ended` (lamp-tasks' stop):
    /// whether the pane was seen with this task's words taken.
    pub fn wait_taken_while(
        &self,
        harness: &dyn PaneHarness,
        machine: &str,
        agent: &str,
        id: &str,
        ended: &dyn Fn() -> bool,
    ) -> bool {
        let mine = header(id);
        let within = Instant::now() + self.open_idle_within;
        while Instant::now() < within && !ended() {
            match harness.observe(machine, agent) {
                Ok(seen)
                    if seen.taken
                        && (seen.seen.state != "idle"
                            || seen
                                .seen
                                .ask
                                .as_deref()
                                .is_some_and(|ask| ask.starts_with(&mine))) =>
                {
                    return true;
                }
                Err(code) if code == "gone" => return false,
                Ok(_) | Err(_) => std::thread::sleep(self.look),
            }
        }
        false
    }

    /// The pane's agent when one is open at `place` and no rotation is due.
    pub fn fresh(&self, place: &Place) -> Option<String> {
        let inner = self.lock();
        let pane = inner.pane.as_ref()?;
        pane.fresh_at(place).then(|| pane.agent.clone())
    }

    /// Whether the pane is open, and no rotation is due: a spoken task
    /// waits in the store until it is ([`spoken_launch`] never opens).
    pub fn has_fresh_pane(&self) -> bool {
        self.lock()
            .pane
            .as_ref()
            .is_some_and(|pane| pane.fresh_at(&pane.place))
    }

    /// Whether `launch` may be for the lamp's pane, whose place is not
    /// known yet (H5 on #7258): an OpenClaw hand-over naming no session,
    /// to the lamp's machine and default engine as last looked at, or with
    /// none looked at yet. It waits in the store rather than going the
    /// per-run way, a second session.
    /// Signed in or out alike: signed out, Harness still gives the lamp
    /// its own machine once its daemon is up (#6407). Never once Harness
    /// took no reply (`reply_off`, [`OneSession::takes_reply`]): then the
    /// per-task way.
    pub fn waits_for_place(&self, launch: &Launch) -> bool {
        let inner = self.lock();
        // Harness takes no reply here: the per-task way, never held.
        if !self.takes_reply() || inner.pane.is_some() || inner.home.is_some() {
            return false;
        }
        shares(launch.target.engine)
            && launch.session.is_none()
            && inner.lamp_key.as_ref().is_none_or(|key| {
                *key == format!(
                    "{} {}",
                    launch.target.machine_id,
                    launch.target.engine.argument()
                )
            })
    }

    /// Where the pane is, when one is open.
    pub fn place(&self) -> Option<Place> {
        self.lock().pane.as_ref().map(|pane| pane.place.clone())
    }

    /// The lamp's place as last read ([`lamp_place`]), kept with no pane
    /// open, so a spoken task for the lamp still waits for the one pane.
    pub fn knows(&self, place: &Place) {
        self.lock().home = Some(place.clone());
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The pane's agent id now, if one is open.
    pub fn agent(&self) -> Option<String> {
        self.lock().pane.as_ref().map(|pane| pane.agent.clone())
    }

    /// How many wait for the pane now.
    pub fn waiting(&self) -> usize {
        self.lock().waiting.len()
    }

    /// The pane is gone with `why` (Harness's daemon started again, #7141's
    /// restart after a sign-in among them): the next open makes a new one.
    /// Nothing is closed here: the daemon that held it is gone.
    pub fn forget(&self, why: &str) {
        let mut inner = self.lock();
        if inner.pane.take().is_some() {
            inner.reopen_why = Some(why.to_owned());
            eprintln!("tasks: one session: let go: {why}");
        }
        inner.next_open = None;
        inner.failed_opens = 0;
        inner.not_ready_opens = 0;
        // A daemon started again may be a newer Harness: asked again.
        inner.programs = None;
        inner.woken = inner.woken.wrapping_add(1);
    }

    /// The pane for `id`: at once when free and none waits, else in the
    /// queue's order (spoken tasks first) until it is free, at most
    /// [`QUEUE_WAIT`] or `until`, whichever is first. Full:
    /// [`Refused::Busy`].
    pub fn enter(
        &self,
        id: &str,
        kind: Kind,
        until: Instant,
        ended: &dyn Fn() -> bool,
    ) -> Result<Turn<'_>, Refused> {
        let mut inner = self.lock();
        let behind_spoken = |inner: &Inner| kind == Kind::Skill && !inner.spoken_queue.is_empty();
        // A removal asked for (a forget, a sign-out) or under way: no task
        // is typed into that pane; it waits for the removal (and the new
        // pane, which its turn opens if the look has not yet). After a
        // `reply_off` no task uses the pane: none waits.
        let removing =
            |inner: &Inner| (inner.drop_due.is_some() || inner.dropping) && self.takes_reply();
        if inner.holder.is_none()
            && inner.waiting.is_empty()
            && !behind_spoken(&inner)
            && !removing(&inner)
        {
            inner.holder = Some(id.to_owned());
            return Ok(Turn {
                one: self,
                id: id.to_owned(),
                opens: true,
                sent: Mutex::new(None),
                retried: AtomicBool::new(false),
                restarted: AtomicBool::new(false),
                make: false,
            });
        }
        // Only skill runs count toward the cap: a spoken task goes ahead of
        // them, never refused for them.
        let skills = inner
            .waiting
            .iter()
            .filter(|(_, kind)| *kind == Kind::Skill)
            .count();
        if kind == Kind::Skill && skills >= QUEUE_MOST {
            eprintln!("tasks: one session: {id} refused busy, {skills} skill runs already wait");
            return Err(Refused::Busy);
        }
        let at = match kind {
            Kind::Spoken => inner
                .waiting
                .iter()
                .position(|(_, kind)| *kind == Kind::Skill)
                .unwrap_or(inner.waiting.len()),
            Kind::Skill => inner.waiting.len(),
        };
        inner.waiting.insert(at, (id.to_owned(), kind));
        eprintln!(
            "tasks: one session: {id} queued behind {}",
            inner.holder.as_deref().unwrap_or("the queue")
        );
        let most = match kind {
            Kind::Spoken | Kind::Skill => self.queue_wait,
        };
        let given_up = (Instant::now() + most).min(until);
        loop {
            let first = inner.waiting.front().is_some_and(|(first, _)| first == id);
            if first && inner.holder.is_none() && !behind_spoken(&inner) && !removing(&inner) {
                inner.waiting.pop_front();
                inner.holder = Some(id.to_owned());
                return Ok(Turn {
                    one: self,
                    id: id.to_owned(),
                    opens: true,
                    sent: Mutex::new(None),
                    retried: AtomicBool::new(false),
                    restarted: AtomicBool::new(false),
                    make: false,
                });
            }
            let now = Instant::now();
            let stop = ended();
            if stop || now >= given_up {
                inner.waiting.retain(|(waiting, _)| waiting != id);
                // The next in line may be free now.
                self.turned.notify_all();
                return Err(if stop || now >= until {
                    Refused::Ended
                } else {
                    eprintln!("tasks: one session: {id} waited its most, refused busy");
                    Refused::Busy
                });
            }
            let rest = given_up.saturating_duration_since(now).min(self.look);
            inner = self
                .turned
                .wait_timeout(inner, rest)
                .map(|(inner, _)| inner)
                .unwrap_or_else(|e| e.into_inner().0);
        }
    }

    /// Opens the pane if none is open and none holds it, within the
    /// backoff: [`OneSession::keep_open`]'s step, the open at start.
    pub fn open_if_due(&self, harness: &dyn PaneHarness, place: &Place) {
        self.open_if_due_while(harness, place, &|| false);
    }

    /// [`OneSession::open_if_due`], its first-turn wait ended by `stopped`.
    pub fn open_if_due_while(
        &self,
        harness: &dyn PaneHarness,
        place: &Place,
        stopped: &dyn Fn() -> bool,
    ) {
        {
            let mut inner = self.lock();
            // A rotation due is done here too, on the one-session thread,
            // never on a spoken task's (the tasks-harness worker's). Work
            // waiting does not hold an open back: a spoken task waits for
            // this one.
            let stale = inner
                .pane
                .as_ref()
                .is_some_and(|pane| !pane.fresh_at(place));
            // Harness took no reply (`reply_off`): no pane opened again
            // until lamp-tasks starts again.
            if !self.takes_reply()
                || (inner.pane.is_some() && !stale)
                || inner.holder.is_some()
                || inner.drop_due.is_some()
                || inner.next_open.is_some_and(|next| Instant::now() < next)
            {
                return;
            }
            inner.holder = Some("open".into());
        }
        let opened = self.ensure(
            harness,
            place,
            Instant::now() + self.open_idle_within,
            stopped,
        );
        let mut inner = self.lock();
        inner.holder = None;
        let first = inner.spoken_queue.first().cloned();
        drop(inner);
        if let Err(why) = &opened {
            eprintln!("tasks: one session: not opened: {why}");
            // A spoken task waits in the store for the pane: each failed
            // open counts toward its bound, so it never waits for good
            // through the backoff (#7895).
            if let Some(first) = first
                && self.not_up(&first)
            {
                eprintln!(
                    "tasks: one session: {first} waited past {STARTING_RESUMES_MOST} failed opens; handed over to end"
                );
            }
        }
        self.turned.notify_all();
    }

    /// The lamp's open at start: [`OneSession::look`] every
    /// [`KEEP_OPEN_EVERY`] while `stop` is not set, the pane opened once
    /// `place` can say where (Harness set up, the lamp's machine and
    /// default engine known) and `cli` can be made.
    pub fn keep_open<H: PaneHarness>(
        self: Arc<Self>,
        place: Box<dyn Fn() -> Result<(H, Place), String> + Send>,
        key: Box<dyn Fn() -> Option<String> + Send>,
        harness: Box<dyn Fn() -> Option<H> + Send>,
        stop: Arc<AtomicBool>,
    ) {
        let mut looks = Looks::default();
        while !stop.load(Ordering::Acquire) {
            let key = key();
            let signed_out =
                crate::skill_task::harness_sign_in() == crate::skill_task::HarnessHere::SignedOut;
            self.remove_if_due(&looks, key.as_ref(), signed_out, &*harness);
            self.look(&mut looks, Instant::now(), key, &*place);
            let until = Instant::now() + KEEP_OPEN_EVERY;
            while Instant::now() < until && !stop.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }

    /// [`OneSession::keep_open`]'s removals for good, with nothing held: a
    /// sign-out ([`signed_out`]), or the lamp's machine or default engine
    /// changed since the pane was opened (`key` against the look's).
    pub fn remove_if_due<H: PaneHarness>(
        &self,
        looks: &Looks,
        key: Option<&String>,
        signed_out: bool,
        harness: &dyn Fn() -> Option<H>,
    ) {
        let (held, open, due) = {
            let mut inner = self.lock();
            inner.signed_out = signed_out;
            let idle = inner
                .pane
                .as_ref()
                .is_some_and(|pane| pane.kept && pane.used.elapsed() >= self.kept_idle_max);
            if idle && inner.holder.is_none() && inner.drop_due.is_none() {
                eprintln!(
                    "tasks: one session: the kept session dropped (no task for {} min); a new one opens",
                    self.kept_idle_max.as_secs() / 60
                );
                inner.drop_due = Some("left with no task".to_owned());
                inner.next_open = None;
            }
            (
                inner.holder.is_some() && inner.holder != inner.closing_for,
                inner.pane.is_some(),
                inner.drop_due.is_some(),
            )
        };
        let moved = if open {
            // Only a real move: the old and the new machine and engine
            // both known and different. A key read unknown for one look
            // (Harness not set up yet, the machine list failed) keeps the
            // pane.
            looks
                .key
                .as_ref()
                .and_then(|was| known_key(was.as_ref()))
                .zip(known_key(key))
                .is_some_and(|(was, now)| was != now)
        } else {
            // None in memory (a lamp-tasks restart): the pane kept on disk
            // for another machine or engine than the lamp's now, once
            // both are known, is removed too.
            // ...or opened signed in and the lamp signed out now (a
            // sign-out while lamp-tasks was down). One opened signed out
            // and still signed out (the ship lamp, #6407) is adopted.
            self.kept_elsewhere(key)
                || (signed_out && self.kept().is_some() && self.kept_signed_in())
        };
        if held || !(due || moved) {
            return;
        }
        let Some(harness) = harness() else {
            return;
        };
        if !self.drop_if_due(&harness) && moved {
            self.drop_pane(&harness, "the lamp's machine or default engine changed");
        }
    }

    /// One look of [`OneSession::keep_open`]: none while a pane is held or
    /// an open's backoff runs; else `place` asked only when due, at once
    /// after a trigger (the first look, [`OneSession::forget`], `key`
    /// changed), else [`NOT_READY_FIRST`] after it said no agent is ready,
    /// doubled to [`NOT_READY_MOST`]. Each new why journaled once.
    pub fn look<H: PaneHarness>(
        &self,
        looks: &mut Looks,
        now: Instant,
        key: Option<String>,
        place: &dyn Fn() -> Result<(H, Place), String>,
    ) {
        let woken = {
            let mut inner = self.lock();
            inner.lamp_key = key.clone();
            let rotation_due = inner
                .pane
                .as_ref()
                .is_some_and(|pane| !pane.fresh_at(&pane.place));
            if !self.takes_reply()
                || (inner.pane.is_some() && !rotation_due)
                || inner.holder.is_some()
                || inner.next_open.is_some_and(|next| now < next)
            {
                return;
            }
            inner.woken
        };
        let changed = looks.key.as_ref() != Some(&key) || looks.woken != woken;
        if !changed && looks.next.is_some_and(|next| now < next) {
            return;
        }
        looks.key = Some(key);
        looks.woken = woken;
        match place() {
            Ok((harness, at)) => {
                self.knows(&at);
                looks.next = None;
                looks.gap = Duration::ZERO;
                looks.why = None;
                self.open_if_due(&harness, &at);
            }
            Err(why) => {
                looks.gap = if changed || looks.gap.is_zero() {
                    NOT_READY_FIRST
                } else {
                    looks.gap.saturating_mul(2).min(NOT_READY_MOST)
                };
                looks.next = Some(now + looks.gap);
                if looks.why.as_deref() != Some(why.as_str()) {
                    eprintln!("tasks: one session: not opened: {why}");
                    looks.why = Some(why);
                }
            }
        }
    }

    /// [`OneSession::keep_open`]'s step: a pane at another machine or
    /// engine removed for good ([`OneSession::drop_pane`]), then opened if
    /// due.
    pub fn tend(&self, harness: &dyn PaneHarness, place: &Place, stopped: &dyn Fn() -> bool) {
        if self.drop_if_due(harness) {
            return;
        }
        let moved = {
            let inner = self.lock();
            // Only the machine or the engine moves the pane; any other
            // field of `Place` keeps it.
            inner.holder.is_none()
                && inner.pane.as_ref().is_some_and(|pane| {
                    pane.place.machine != place.machine || pane.place.engine != place.engine
                })
        };
        if moved {
            self.drop_pane(harness, "the lamp's machine or default engine changed");
        }
        self.open_if_due_while(harness, place, stopped);
    }

    /// The pane at `place`, opened when there is none or it is stale (the
    /// machine or engine changed). Held by the caller's turn.
    fn ensure(
        &self,
        harness: &dyn PaneHarness,
        place: &Place,
        until: Instant,
        ended: &dyn Fn() -> bool,
    ) -> Result<String, String> {
        let stale = {
            let mut inner = self.lock();
            match &inner.pane {
                Some(pane) if pane.fresh_at(place) => {
                    return Ok(pane.agent.clone());
                }
                Some(pane) => {
                    if pane.place.same_spot(place) && !pane.full {
                        eprintln!(
                            "tasks: one session: rotated after {} turns, ~{} tokens",
                            pane.turns, pane.tokens
                        );
                    }
                    let why = if pane.place.same_spot(place) {
                        // The rotation (Hermes' own bounds), or near its
                        // context's limit (#7906): a fresh pane.
                        "its rotation"
                    } else if pane.place.machine != place.machine {
                        "the lamp's machine changed"
                    } else {
                        "the default engine changed"
                    };
                    inner.reopen_why = Some(why.into());
                    inner.pane.take()
                }
                None => None,
            }
        };
        // The kept session rotated is left to `harness ensure --fresh`,
        // which closes it and starts a new conversation; moved (another
        // machine or engine), it is closed, its conversation deleted and its
        // token removed.
        let stale = match stale {
            Some(pane) if pane.kept => {
                if !pane.place.same_spot(place)
                    && let Some(file) = self.kept_token.clone()
                {
                    if let Err(code) =
                        harness.close_kept(&name_for(pane.place.engine), &file, false)
                    {
                        eprintln!(
                            "tasks: one session: the old kept session {} not closed: {code}",
                            short(&pane.agent)
                        );
                    }
                    self.kept_closed(&pane.agent, pane.place.engine);
                    self.forget_token();
                }
                None
            }
            stale => stale,
        };
        // The stale one (rotated, or another machine or engine) is closed
        // where it was, best effort, and its conversation deleted.
        if let Some(pane) = stale {
            if let Err(why) = harness.close_now(&pane.place.machine, &pane.agent) {
                eprintln!(
                    "tasks: one session: the old pane {} not closed: {why}",
                    short(&pane.agent)
                );
            }
            self.closed_by_us(&pane.agent, pane.place.engine, true);
        }
        self.open(harness, place, until, ended)
    }

    fn open(
        &self,
        harness: &dyn PaneHarness,
        place: &Place,
        until: Instant,
        ended: &dyn Fn() -> bool,
    ) -> Result<String, String> {
        if let Some(token) = self.kept_token(harness) {
            return self.open_kept(harness, place, &token, until, ended, 2);
        }
        let began = Instant::now();
        let kept = self.kept();
        // The pane kept by its id (a lamp-tasks restart, or a reboot that
        // Harness's restore brought it back from under the same id): live,
        // adopted; stopped or gone, resumed once; else a fresh `new`.
        if let Some((agent, machine, engine)) = kept.clone()
            && self.lock().reopen_why.as_deref() != Some("its rotation")
            && machine == place.machine
            && engine == place.engine.argument()
        {
            let adopted = match harness.observe(&place.machine, &agent) {
                // Stuck on an ask no one holds (Heron's H1 on #7258):
                // closed and deleted below, a new pane opened.
                Ok(seen) if seen.seen.state == "needs-input" => None,
                // #7906 sign 2: its engine exited, the pane a plain shell.
                Ok(seen) if seen.shell => None,
                Ok(seen) if !gone_state(&seen.seen.state) => Some(("adopted", seen.seen.state)),
                Ok(_) | Err(_) => match harness.resume(&place.machine, &agent) {
                    Ok(state) if !gone_state(&state) => Some(("resumed", state)),
                    Ok(state) => {
                        eprintln!(
                            "tasks: one session: {} resumed {state}; a new pane",
                            short(&agent)
                        );
                        None
                    }
                    Err(code) => {
                        eprintln!(
                            "tasks: one session: {} not resumed: {code}; a new pane",
                            short(&agent)
                        );
                        None
                    }
                },
            };
            if let Some((how, state)) = adopted {
                let mut inner = self.lock();
                inner.reopen_why = None;
                eprintln!("tasks: one session: {how} {} ({state})", short(&agent));
                inner.pane = Some(Pane::new(&agent, place, false));
                inner.failed_opens = 0;
                inner.next_open = None;
                return Ok(agent);
            }
        }
        // A kept pane not taken back (gone and not resumed, resumed gone,
        // not read, or for another machine or engine) is closed and its
        // OpenClaw conversation deleted before the new one's id is kept
        // over it: else its words would stay, and a live one would run
        // beside the new pane (the Reviewer-lead's HOLD 1 on #7258).
        if let Some((old, machine, engine)) = kept {
            match harness.close_now(&machine, &old) {
                // Harness no longer has it (`UNKNOWN_HARNESS`, as after a
                // restart lost every pane): already gone, so dropped below
                // as a closed one is, and the new pane opened (#7246).
                Err(code) if code == crate::harness::CLOSE_UNKNOWN_HARNESS => eprintln!(
                    "tasks: one session: {} already gone (UNKNOWN_HARNESS); a new pane",
                    short(&old)
                ),
                Err(code) => {
                    eprintln!("tasks: one session: {} not closed: {code}", short(&old));
                    // On this machine it may still be live (a daemon coming
                    // up): backed off and tried again, never a `new` beside
                    // it (Heron's P2 on #7258). Gone, or on an old machine
                    // id (a sign-in or sign-out changed it), it is dropped
                    // as before.
                    if code != "gone" && machine == place.machine {
                        return Err(self.open_failed(&format!("{} not closed", short(&old))));
                    }
                }
                Ok(_) => {}
            }
            let engine = if engine == EngineKind::Openclaw.argument() {
                EngineKind::Openclaw
            } else {
                place.engine
            };
            self.closed_by_us(&old, engine, true);
            eprintln!(
                "tasks: one session: dropped {} (not taken back)",
                short(&old)
            );
        }
        let name = name_for(place.engine);
        let launch = Launch {
            name,
            prompt: OPEN_PROMPT.into(),
            target: Target {
                machine_id: place.machine.clone(),
                project: place.folder.clone(),
                engine: place.engine,
            },
            session: None,
        };
        // An uncertain `new` before: its pane looked for first, never a
        // second `new` beside it (the Reviewer-lead's HOLD 2 on #7258).
        let unsure = std::mem::take(&mut self.lock().unsure_open);
        let found = if unsure {
            self.found_open(harness, place)?
        } else {
            None
        };
        let outcome = match found {
            Some(agent_id) => LaunchOutcome::Created { agent_id },
            None => harness.open(&launch),
        };
        let agent = match outcome {
            LaunchOutcome::Created { agent_id } => {
                // Kept at once: a lamp-tasks stopped meanwhile adopts it.
                self.keep(&agent_id, place);
                agent_id
            }
            LaunchOutcome::NotSent { code } => {
                return Err(self.open_failed(&format!("harness new: {code}")));
            }
            LaunchOutcome::Uncertain { .. } | LaunchOutcome::Pending { .. } => {
                // Harness may have made it: looked for in its folder, now
                // and at the next open, and kept when found.
                match self.found_open(harness, place) {
                    Ok(Some(agent_id)) => {
                        self.keep(&agent_id, place);
                        agent_id
                    }
                    Ok(None) | Err(_) => {
                        self.lock().unsure_open = true;
                        return Err(self.open_failed("harness new: uncertain, looked for next"));
                    }
                }
            }
            LaunchOutcome::Joined { .. } | LaunchOutcome::JoinedOn { .. } => {
                return Err(self.open_failed("harness new: joined another session"));
            }
        };
        // Its first turn ended, so the first run finds it idle; past the
        // bound it is kept anyway.
        // Bounded by the caller's run (its deadline) and lamp-tasks' stop.
        let cancel = AtomicBool::new(false);
        let until = until.min(Instant::now() + self.open_idle_within);
        while Instant::now() < until && !ended() {
            match harness.wait_end(&place.machine, &agent, &cancel) {
                Ok(Some(state)) if state == "idle" => break,
                Ok(Some(state)) => {
                    let _ = harness.close_now(&place.machine, &agent);
                    return Err(self.open_failed(&format!("its first turn ended {state}")));
                }
                Ok(None) => {}
                Err(code) => {
                    eprintln!("tasks: one session: harness wait: {code}");
                    std::thread::sleep(self.look);
                }
            }
        }
        let mut inner = self.lock();
        if let Some(why) = inner.reopen_why.take() {
            eprintln!("tasks: one session: reopened after {why}");
        }
        eprintln!(
            "tasks: one session: opened {} ({}) in {} ms",
            short(&agent),
            place.engine.argument(),
            began.elapsed().as_millis()
        );
        inner.pane = Some(Pane::new(&agent, place, false));
        inner.failed_opens = 0;
        inner.next_open = None;
        inner.home = Some(place.clone());
        Ok(agent)
    }

    /// The lamp's kept session (openharness#4393): `harness ensure` by its
    /// name ([`name_for`]) and token, live, resumed (a pane that died, a
    /// reboot) or new; `--fresh` at a rotation. A pane the old way kept
    /// holds the name, so it is closed and deleted first. A new
    /// conversation is given [`OPEN_PROMPT`] and its turn waited for, as a
    /// `harness new`'s first turn is. `ENGINE_NOT_READY` backs off as with
    /// no agent ready; `NAME_TAKEN` or `NOT_YOURS` sends the pane the old
    /// way until Harness's daemon starts again.
    fn open_kept(
        &self,
        harness: &dyn PaneHarness,
        place: &Place,
        token: &Path,
        until: Instant,
        ended: &dyn Fn() -> bool,
        settles: u8,
    ) -> Result<String, String> {
        let began = Instant::now();
        // #7906: a token with no kept record means lamp-tasks closed that
        // kept session itself (a deadline, a cancel, an unanswered turn, a
        // stuck sign) and deleted its OpenClaw conversation. `ensure` finds
        // a session by its name and token, not by the forgotten id, so
        // without `--fresh` it resumes that deleted conversation for ever.
        let mut closed_by_us = token.exists() && !self.kept_is_program();
        // ...and a kept record whose OpenClaw conversation the Gateway no
        // longer holds (deleted under it, #7906 sign 3): forgotten before
        // `ensure`, never resumed, since Harness resumes it with no Gateway
        // check. Read once here; the first hand-over does not read again.
        let mut checked = false;
        if !closed_by_us && place.engine == EngineKind::Openclaw {
            match self.kept_held(harness) {
                Some((agent, Some(false))) => {
                    eprintln!(
                        "tasks: one session: {} for the kept session {}; forgotten, a fresh one (#7906)",
                        KeptGone::GatewaySession.why(),
                        short(&agent)
                    );
                    self.closed_by_us(&agent, EngineKind::Openclaw, false);
                    closed_by_us = true;
                }
                Some(_) => checked = true,
                None => {}
            }
        }
        if let Some((old, machine, engine)) = self.kept()
            && !self.kept_is_program()
        {
            match harness.close_now(&machine, &old) {
                Err(code)
                    if code != crate::harness::CLOSE_UNKNOWN_HARNESS
                        && code != "gone"
                        && machine == place.machine =>
                {
                    eprintln!("tasks: one session: {} not closed: {code}", short(&old));
                    return Err(self.open_failed(&format!("{} not closed", short(&old))));
                }
                Ok(_) | Err(_) => {}
            }
            self.closed_by_us(&old, engine_named(&engine).unwrap_or(place.engine), true);
            eprintln!(
                "tasks: one session: dropped {} (opened before the kept session)",
                short(&old)
            );
        }
        if let Err(why) = token_file(token) {
            return Err(self.open_failed(&format!("its token file: {why}")));
        }
        let fresh = closed_by_us || self.lock().reopen_why.as_deref() == Some("its rotation");
        // At a rotation the old conversation's key, deleted once `--fresh`
        // made a new one (Harness's close keeps it in OpenClaw's store).
        let old = fresh
            .then(|| {
                self.kept()
                    .map(|(agent, _, _)| (self.kept_session_key(&agent), agent))
            })
            .flatten();
        let name = name_for(place.engine);
        // `ENGINE_NOT_READY` (the old engine still exiting after a crash,
        // a gateway still starting): tried again every few seconds within a
        // short bound, then the long backoff.
        let gap = ENGINE_RETRY.min(self.look.saturating_mul(50));
        let short_until = until.min(Instant::now() + ENGINE_RETRY_WITHIN);
        let mut ensured = harness.ensure_kept(place.engine, &name, token, &place.folder, fresh);
        while matches!(&ensured, Err(code) if code == "engine_not_ready")
            && Instant::now() + gap < short_until
            && !ended()
        {
            std::thread::sleep(gap);
            ensured = harness.ensure_kept(place.engine, &name, token, &place.folder, fresh);
        }
        let ensured = match ensured {
            Ok(ensured) => ensured,
            Err(code) if code == "engine_not_ready" => {
                return Err(self.not_ready("harness ensure: engine_not_ready"));
            }
            Err(code) if code == "name_taken" || code == "not_yours" => {
                self.not_ours(&code);
                return Err(self.open_failed(&format!(
                    "harness ensure: {code}; the pane the old way next"
                )));
            }
            Err(code) => {
                if code == "bad_token_file" {
                    self.forget_token();
                }
                return Err(self.open_failed(&format!("harness ensure: {code}")));
            }
        };
        let agent = ensured.agent_id.clone();
        let mut pane = Pane::new(&agent, place, true);
        pane.session = ensured.engine_session.clone();
        pane.checked_at_open = checked || ensured.result == "new";
        self.lock().pane = Some(pane);
        if let Some((key, old)) = old
            && old != agent
            && place.engine == EngineKind::Openclaw
            && let Err(code) = (self.delete_session)(&key)
        {
            eprintln!(
                "tasks: one session: the rotated kept session {}'s OpenClaw session not deleted: {code}",
                short(&old)
            );
        }
        // Kept at once: the record says the kept session, so a sign-out
        // while lamp-tasks is down still removes it.
        self.keep(&agent, place);
        if ensured.result == "new" {
            // openharness#4484: a `new` with any `whyCode`
            // (`ENGINE_SESSION_GONE`, `HARNESS_GONE`, `ENGINE_EXITED`, or
            // one not known yet): Harness itself replaced the session. The
            // record now names the new one (kept above); no extra close or
            // `--fresh`.
            if let Some(code) = ensured.why_code.as_deref() {
                eprintln!(
                    "tasks: one session: Harness replaced the kept session ({code}); the old record forgotten, {} kept",
                    short(&agent)
                );
            }
            self.first_words(harness, &name, token, until, ended);
        } else if settles > 0
            && let Some((why, died)) = self.settled(harness, place, &ensured, began, until, ended)
        {
            // #7906 signs 1-2 at an open: the lightest reopen first (a plain
            // resume of a died one whose conversation OpenClaw still holds),
            // else a fresh one.
            self.revive_or_replace(harness, &place.machine, &agent, &why, died);
            {
                let inner = self.lock();
                if inner.next_open.is_some() || inner.drop_due.is_some() {
                    return Err(format!("{why}; a fresh one is opened next"));
                }
            }
            return self.open_kept(harness, place, token, until, ended, settles - 1);
        }
        let mut inner = self.lock();
        if let Some(why) = inner.reopen_why.take() {
            eprintln!("tasks: one session: reopened after {why}");
        }
        eprintln!(
            "tasks: one session: kept session {} {} ({}, {}){} in {} ms",
            ensured.result,
            short(&agent),
            place.engine.argument(),
            ensured.state,
            ensured
                .why
                .as_deref()
                .map_or_else(String::new, |why| format!(", {why}")),
            began.elapsed().as_millis()
        );
        inner.failed_opens = 0;
        inner.not_ready_opens = 0;
        inner.next_open = None;
        inner.home = Some(place.clone());
        Ok(agent)
    }

    /// A new kept session's first words ([`OPEN_PROMPT`]), taken and
    /// waited for within `until`, so the first task finds it idle. Not a
    /// turn; a failure is journaled only, and the first task waits instead.
    fn first_words(
        &self,
        harness: &dyn PaneHarness,
        name: &str,
        token: &Path,
        until: Instant,
        ended: &dyn Fn() -> bool,
    ) {
        // A fresh OpenClaw session refuses its first reply ENGINE_NOT_READY
        // for a while (~35 s seen): tried again within the open's bound,
        // never counted as stuck.
        let gap = ENGINE_RETRY.min(self.look.saturating_mul(50));
        let within = until.min(Instant::now() + self.open_idle_within);
        let mut took = harness.reply_kept(name, token, OPEN_PROMPT);
        while took == Took::EngineNotReady && Instant::now() + gap < within && !ended() {
            std::thread::sleep(gap);
            took = harness.reply_kept(name, token, OPEN_PROMPT);
        }
        let input = match took {
            Took::Taken(input) => input,
            other => {
                eprintln!(
                    "tasks: one session: the kept session's first words not taken: {other:?}"
                );
                return;
            }
        };
        let cancel = AtomicBool::new(false);
        let until = until.min(Instant::now() + self.open_idle_within);
        while Instant::now() < until && !ended() {
            match harness.wait_kept(name, token, &input, &cancel) {
                Ok(Some(state)) if state == "idle" => return,
                Ok(Some(state)) => {
                    eprintln!("tasks: one session: the kept session's first turn ended {state}");
                    return;
                }
                Ok(None) => {}
                Err(code) => {
                    eprintln!("tasks: one session: harness wait --after: {code}");
                    std::thread::sleep(self.look);
                }
            }
        }
    }

    /// #7906, a kept session `ensure` found (`live` or `resumed`) looked at
    /// before any task is typed into it: why it is gone or stuck, if it is.
    /// Sign 2, its pane a plain shell or gone; sign 1, still
    /// `starting` past `open_idle_within` from the open's start. One `show`
    /// for a healthy one, at
    /// an open only, never per turn; `None` too once `until` or `ended`
    /// comes first (no verdict).
    fn settled(
        &self,
        harness: &dyn PaneHarness,
        place: &Place,
        ensured: &crate::harness::Ensured,
        began: Instant,
        until: Instant,
        ended: &dyn Fn() -> bool,
    ) -> Option<(String, bool)> {
        let agent = ensured.agent_id.as_str();
        // openharness#4484 (proposed): a real `stopped` is replaced at
        // once. `starting` alone is never trusted (today Harness says it of
        // every answer but `live`): `show` decides.
        if ensured.state == "stopped" {
            return Some(("Harness's ensure says it is stopped".into(), true));
        }
        // A resume Harness could not confirm is never kept on no verdict.
        let unconfirmed = ensured.confirmed == Some(false);
        // From the open's start (`began`), as its own first-turn wait.
        let bound = began + self.open_idle_within;
        loop {
            match self.pane_sign(harness, &place.machine, agent) {
                Seen::Died(why) => return Some((why, true)),
                Seen::Starting => {}
                // No verdict: past the bound, kept as it is, unless Harness
                // could not confirm its resume.
                Seen::Unknown if Instant::now() >= bound && unconfirmed => {
                    return Some((
                        "a resume Harness could not confirm, with no verdict from show".into(),
                        false,
                    ));
                }
                Seen::Unknown if Instant::now() >= bound => return None,
                Seen::Unknown => {}
                Seen::Moving => return None,
            }
            if Instant::now() >= bound {
                return Some((
                    format!("still starting after {} s", self.open_idle_within.as_secs()),
                    false,
                ));
            }
            if ended() || Instant::now() >= until {
                return None;
            }
            std::thread::sleep(self.look);
        }
    }

    /// One `show` of the pane `agent` for #7906's signs 1 and 2.
    fn pane_sign(&self, harness: &dyn PaneHarness, machine: &str, agent: &str) -> Seen {
        match harness.observe(machine, agent) {
            Err(code) if code == "gone" => Seen::Died("Harness no longer has it".into()),
            Ok(seen) if seen.shell => {
                Seen::Died("its engine exited; the pane is a plain shell".into())
            }
            Ok(seen) if gone_state(&seen.seen.state) => {
                Seen::Died(format!("Harness says it is {}", seen.seen.state))
            }
            Ok(seen) if matches!(seen.seen.state.as_str(), "idle" | "working" | "needs-input") => {
                Seen::Moving
            }
            // Only `starting` read by `show` itself can time out into stuck.
            Ok(seen) if seen.seen.state == "starting" => Seen::Starting,
            // `installing` (an OTA's OpenClaw install), `offline`,
            // `unknown`, or a `show` not read (a parse error, a daemon
            // timeout): never a sign; looked at again.
            Ok(_) | Err(_) => Seen::Unknown,
        }
    }

    /// #7906: the pane `agent` is gone or stuck (`why`), the one path every
    /// stuck sign takes. Closed `--now`,
    /// its id forgotten in the state file and its OpenClaw conversation
    /// deleted, and let go: the next open is a fresh one (the kept session
    /// `ensure --fresh`, its token kept; a pane of `harness new`, a `new`),
    /// never a resume of it. A close that fails is tried again `--now` at
    /// the next look, as a cancelled turn's is. Past [`REPLACE_MOST`] in a
    /// row with no turn answered, the next open waits its backoff.
    pub fn replace_kept(&self, harness: &dyn PaneHarness, machine: &str, agent: &str, why: &str) {
        eprintln!(
            "tasks: one session: {} is gone or stuck ({why}); closed, forgotten and replaced with a fresh session (#7906)",
            short(agent)
        );
        let engine = self
            .lock()
            .pane
            .as_ref()
            .filter(|pane| pane.agent == agent)
            .map_or(EngineKind::Openclaw, |pane| pane.place.engine);
        match self.kept_now(agent) {
            Some((name, token)) => match harness.close_kept(&name, &token, true) {
                Ok(()) => self.kept_closed(agent, engine),
                Err(code) => {
                    eprintln!(
                        "tasks: one session: the stuck kept session {} not closed: {code}; tried again next",
                        short(agent)
                    );
                    let mut inner = self.lock();
                    inner.drop_due = Some(why.to_owned());
                    inner.drop_now = true;
                    inner.drop_keeps_token = true;
                }
            },
            None => {
                if let Err(code) = harness.close_now(machine, agent) {
                    eprintln!("tasks: one session: {} not closed: {code}", short(agent));
                }
                self.closed_by_us(agent, engine, true);
            }
        }
        self.gone(agent, why);
        let in_row = {
            let mut inner = self.lock();
            inner.replaced_in_row = inner.replaced_in_row.saturating_add(1);
            inner.replaced_in_row
        };
        // From the replacements in a row, not `failed_opens`, which the
        // next `ensure` that answers resets: 5 s, doubled, at most 300 s.
        if in_row >= REPLACE_MOST {
            let after = OPEN_RETRY_FIRST
                .saturating_mul(1 << (in_row - REPLACE_MOST).min(16))
                .min(OPEN_RETRY_MOST);
            self.lock().next_open = Some(Instant::now() + after);
            eprintln!(
                "tasks: one session: {in_row} stuck sessions in a row; the next open in {} s",
                after.as_secs()
            );
        }
        if let Some(replaced) = &self.on_replaced {
            replaced(why);
        }
    }

    /// A turn answered: stuck replacements no longer in a row.
    fn answered(&self) {
        let mut inner = self.lock();
        inner.replaced_in_row = 0;
        inner.revived = None;
    }

    /// #7906, the owner's ladder, the lightest reopen first. (a) A kept
    /// session whose engine or pane died (`died`) while OpenClaw's record
    /// still holds its conversation (or can't be read): let go with no
    /// close and no delete, so the next `ensure` is a plain resume that
    /// keeps its memory, once. (b) Otherwise, or died again after that
    /// resume: [`OneSession::replace_kept`], a fresh conversation. (c) The
    /// backoff of [`REPLACE_MOST`]. Returns whether it was (a).
    pub fn revive_or_replace(
        &self,
        harness: &dyn PaneHarness,
        machine: &str,
        agent: &str,
        why: &str,
        died: bool,
    ) -> bool {
        let revive = died
            && self.kept_now(agent).is_some()
            && self.lock().revived.as_deref() != Some(agent)
            && harness.session_held(&self.kept_session_key(agent)) != Some(false);
        if !revive {
            self.replace_kept(harness, machine, agent, why);
            return false;
        }
        eprintln!(
            "tasks: one session: {} died ({why}); OpenClaw still holds its conversation, so a plain resume first (#7906)",
            short(agent)
        );
        self.lock().revived = Some(agent.to_owned());
        self.gone(agent, why);
        true
    }

    /// `harness ensure` said the engine is not ready: the next open after
    /// [`NOT_READY_FIRST`], doubled at each in a row up to
    /// [`NOT_READY_MOST`], as with no agent ready.
    fn not_ready(&self, why: &str) -> String {
        let mut inner = self.lock();
        inner.not_ready_opens = inner.not_ready_opens.saturating_add(1);
        let after = NOT_READY_FIRST
            .saturating_mul(1 << (inner.not_ready_opens - 1).min(16))
            .min(NOT_READY_MOST);
        inner.next_open = Some(Instant::now() + after);
        format!("{why}; tried again in {} s", after.as_secs())
    }

    /// The pane an uncertain `new` may have made: the agent `ps` lists
    /// live in its folder (the pane's own, [`FOLDER`]). `Err` when `ps`
    /// can't say, so no other `new` is made yet.
    fn found_open(
        &self,
        harness: &dyn PaneHarness,
        place: &Place,
    ) -> Result<Option<String>, String> {
        match harness.live_in(&place.machine, &place.folder) {
            Ok(live) => {
                let found = live.into_iter().next();
                if let Some(agent) = &found {
                    eprintln!(
                        "tasks: one session: found {} an uncertain new made",
                        short(agent)
                    );
                }
                Ok(found)
            }
            Err(code) => {
                self.lock().unsure_open = true;
                Err(self.open_failed(&format!("harness ps after an uncertain new: {code}")))
            }
        }
    }

    fn open_failed(&self, why: &str) -> String {
        let mut inner = self.lock();
        inner.failed_opens = inner.failed_opens.saturating_add(1);
        let after = OPEN_RETRY_FIRST
            .saturating_mul(1 << (inner.failed_opens - 1).min(16))
            .min(OPEN_RETRY_MOST);
        inner.next_open = Some(Instant::now() + after);
        format!("{why}; tried again in {} s", after.as_secs())
    }

    /// Spoken task `id` found the session not up once more: its words
    /// found it not running, or an open (`harness ensure`, `new`) failed
    /// while it waited first for the pane. Whether that is past
    /// [`STARTING_RESUMES_MOST`] (#7895).
    fn not_up(&self, id: &str) -> bool {
        let mut inner = self.lock();
        let count = match &mut inner.unstarted {
            Some((was, count)) if was == id => {
                *count += 1;
                *count
            }
            slot => {
                *slot = Some((id.to_owned(), 1));
                1
            }
        };
        count > STARTING_RESUMES_MOST
    }

    /// Whether spoken task `id` found the session not up past
    /// [`STARTING_RESUMES_MOST`]: the store hands it over at once, and it
    /// ends [`NOT_STARTED`] ([`spoken_launch`]).
    pub fn not_up_spent(&self, id: &str) -> bool {
        matches!(&self.lock().unstarted, Some((was, count)) if was == id && *count > STARTING_RESUMES_MOST)
    }

    /// The pane gone with `why`: let go, so the next open makes a new one.
    fn gone(&self, agent: &str, why: &str) {
        let mut inner = self.lock();
        if inner.pane.as_ref().is_some_and(|pane| pane.agent == agent) {
            inner.pane = None;
            inner.reopen_why = Some(why.to_owned());
            inner.next_open = None;
        }
    }

    /// #7906 sign 3, the seam before a turn is handed to the kept session
    /// `agent`: `Some` when OpenClaw's Gateway no longer holds its
    /// conversation by its own session key ([`PaneHarness::session_held`],
    /// the one Gateway check), so the pane would only say "No session
    /// found". One read-only look, no CLI start. `None` for a pane not
    /// kept, not OpenClaw's, a record that can't be read, or one whose
    /// open in this process already looked (or `ensure` made it new) and
    /// that has had no turn yet.
    pub fn kept_gone_at_hand_over(
        &self,
        harness: &dyn PaneHarness,
        agent: &str,
    ) -> Option<KeptGone> {
        let checked = self.lock().pane.as_ref().is_some_and(|pane| {
            pane.kept
                && pane.agent == agent
                && pane.place.engine == EngineKind::Openclaw
                && !(pane.checked_at_open && pane.turns == 0)
        });
        (checked && harness.session_held(&self.kept_session_key(agent)) == Some(false))
            .then_some(KeptGone::GatewaySession)
    }

    /// The kept session `agent` took a turn now, the Gateway's record at
    /// `base` just before (#7906 sign 4's start bound).
    fn handed_over(&self, agent: &str, base: Option<(String, i64)>) {
        if let Some(pane) = self.lock().pane.as_mut()
            && pane.agent == agent
        {
            pane.handed = Some((Instant::now(), base));
        }
    }

    /// The kept record's agent and whether the Gateway holds its session
    /// key (`None`: unread), for an OpenClaw kept record of a program
    /// session; `None` with no such record, or one `ensure` made new in
    /// this open before its first turn.
    fn kept_held(&self, harness: &dyn PaneHarness) -> Option<(String, Option<bool>)> {
        let (agent, _, engine) = self.kept()?;
        if !self.kept_is_program() || engine != EngineKind::Openclaw.argument() {
            return None;
        }
        let unprompted = self.lock().pane.as_ref().is_some_and(|pane| {
            pane.agent == agent && pane.checked_at_open && pane.turns == 0
        });
        if unprompted {
            return None;
        }
        let held = harness.session_held(&self.kept_session_key(&agent));
        Some((agent, held))
    }

    /// #7906 sign 3, the seam before `harness ensure` (which resumes by
    /// name and token and never checks the Gateway): `Some` when the kept
    /// record ([`STATE_FILE`]) names an OpenClaw kept session whose
    /// conversation the Gateway no longer holds, so the open is `--fresh`.
    /// `None` with no such record, one made new in this open before its
    /// first turn, or a record that can't be read.
    pub fn kept_gone_before_ensure(&self, harness: &dyn PaneHarness) -> Option<KeptGone> {
        matches!(self.kept_held(harness), Some((_, Some(false)))).then_some(KeptGone::GatewaySession)
    }

    /// #7906 sign 4: a reader of the Gateway's record of the kept session
    /// `agent`'s turn by its own session key, for an OpenClaw pane; each
    /// read one read-only SQLite read, no CLI start.
    fn records<'a>(&self, harness: &'a dyn PaneHarness, agent: &str) -> Option<TurnReader<'a>> {
        let openclaw =
            self.lock().pane.as_ref().is_some_and(|pane| {
                pane.agent == agent && pane.place.engine == EngineKind::Openclaw
            });
        if openclaw {
            harness.openclaw_turns(&self.kept_session_key(agent))
        } else {
            None
        }
    }

    /// #7906 signs 3 and 4: the kept session `agent` is gone or stuck
    /// (`gone`): replaced the one way every stuck sign is
    /// ([`OneSession::replace_kept`]: closed `--now`, forgotten, its token
    /// kept, counted toward the backoff), so the next open is `--fresh`.
    pub fn kept_gone(&self, harness: &dyn PaneHarness, agent: &str, gone: &KeptGone) {
        let machine = self
            .lock()
            .pane
            .as_ref()
            .filter(|pane| pane.agent == agent)
            .map(|pane| pane.place.machine.clone())
            .unwrap_or_default();
        self.replace_kept(harness, &machine, agent, &gone.why());
    }
}

/// #7906's told line: said once to the asker of a task whose stuck session
/// was replaced and that was done again in the new one ([`Turn::restarted`]).
pub const RESTARTED_LINE: &str =
    "The agent got stuck, so I restarted it and did your request again.";

/// The pane held by one run's turn; let go when dropped, and the next in
/// the queue goes.
pub struct Turn<'a> {
    one: &'a OneSession,
    id: String,
    /// Whether [`Turn::send`] may open or rotate the pane. A spoken task's
    /// may not: it runs on the tasks-harness worker, so with no fresh pane
    /// it is put back and the one-session thread opens or rotates it.
    opens: bool,
    /// The last words sent, where and within when, for one put-back after
    /// the session died under the turn (#7906, [`Turn::stuck`]).
    sent: Mutex<Option<(Place, String, Instant)>>,
    /// That put-back used.
    retried: AtomicBool,
    /// Its session was replaced while this task waited or was in flight,
    /// and the task went on in the new one ([`Turn::restarted`], #7906's
    /// told line).
    restarted: AtomicBool,
    /// A make's turn ([`Turn::as_make`]): the pane's size bound is
    /// [`MAKE_FRESH_TOKENS`], not [`TASK_FRESH_TOKENS`] (#7906).
    make: bool,
}

/// What one `show` says of a pane for #7906's signs.
enum Seen {
    /// Its engine or pane died (a plain shell, `stopped`, gone), with why:
    /// a plain resume may bring it back ([`OneSession::revive_or_replace`]).
    Died(String),
    /// `show` read `starting`: stuck past the bound.
    Starting,
    /// No verdict (`installing`, `offline`, `unknown`, not read): never
    /// times out into stuck.
    Unknown,
    Moving,
}

/// How a turn ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Ended {
    /// The pane went idle on this task's turn, with its answer.
    Answered(String),
    /// The turn ended otherwise (the pane stopped, failed, asks the person
    /// something), with why; the pane is let go or closed.
    Failed(String),
    /// Cancelled or at its deadline: the pane was closed `--now`.
    Cancelled,
}

impl Turn<'_> {
    /// #7906, the owner's "never stuck without the person knowing": whether
    /// this task's session was replaced while it waited or was in flight
    /// and the task went on in the new one. Its asker then hears
    /// [`RESTARTED_LINE`] once, before its answer, on the path its answer
    /// is told by; a task given up is told its own failed line instead.
    pub fn restarted(&self) -> bool {
        self.restarted.load(Ordering::Acquire)
    }

    /// This turn is a make's when `make` (#7906): typed only into a session
    /// with room for it ([`OneSession::start_fresh_if_near_limit`]).
    pub fn as_make(mut self, make: bool) -> Self {
        self.make = make;
        self
    }

    /// `words` typed into the pane after this task's header line, the
    /// pane opened first when there is none. A pane Harness says is not
    /// running is reopened once and typed again; another failure is
    /// [`Sent::NotTyped`]. A busy pane is waited for until idle, within
    /// `until`.
    pub fn send(
        &self,
        harness: &dyn PaneHarness,
        place: &Place,
        words: &str,
        until: Instant,
        ended: &dyn Fn() -> bool,
    ) -> Sent {
        // Harness took no reply before: not tried again.
        if !self.one.takes_reply() {
            return Sent::NoReply;
        }
        let text = format!("{}\n{words}", header(&self.id));
        *self.sent.lock().unwrap_or_else(|e| e.into_inner()) =
            Some((place.clone(), words.to_owned(), until));
        let mut reopened = false;
        let mut sized = false;
        // #7906 sign 1: since when the pane has refused the words (busy,
        // its engine not ready, not idle): past `open_idle_within`, one
        // `show` says whether it is stuck, and a stuck one is replaced once.
        let mut refused_since: Option<Instant> = None;
        let mut replaced = false;
        // #7906: the kept session looked at once per agent before its first
        // hand-over (sign 3), and a refused hand-over's wait watched (sign 4).
        let mut checked: Option<String> = None;
        let mut refused: Option<(TurnWatch, Instant)> = None;
        loop {
            let agent = if self.opens {
                self.one.ensure(harness, place, until, ended)
            } else {
                self.one
                    .fresh(place)
                    .ok_or_else(|| "no fresh pane yet; the one-session thread opens it".to_owned())
            };
            let agent = match agent {
                Ok(agent) => agent,
                Err(why) if !reopened && self.opens => {
                    reopened = true;
                    eprintln!("tasks: one session: {}: open failed: {why}", self.id);
                    continue;
                }
                Err(why) => return self.fallback(&why),
            };
            // Near its context's limit (#7906): a fresh session first, once
            // for this turn, also for a pane just adopted after a restart;
            // a spoken task's is opened by the one-session thread, as at a
            // rotation.
            if !sized {
                sized = true;
                if self.one.start_fresh_if_near_limit(place, self.make) {
                    continue;
                }
            }
            // The kept session (openharness#4393): an acknowledged reply,
            // counted only once the engine took it.
            if let Some((name, token)) = self.one.kept_now(&agent) {
                let gone = if checked.as_deref() == Some(agent.as_str()) {
                    None
                } else {
                    checked = Some(agent.clone());
                    self.one.kept_gone_at_hand_over(harness, &agent)
                };
                if let Some(gone) = gone {
                    if replaced {
                        return self
                            .fallback(&format!("the kept session ensured again: {}", gone.why()));
                    }
                    replaced = true;
                    self.restarted.store(true, Ordering::Release);
                    self.one.kept_gone(harness, &agent, &gone);
                    continue;
                }
                let base = self
                    .one
                    .records(harness, &agent)
                    .and_then(|read| read())
                    .map(|record| (record.session, record.seq));
                let took = harness.reply_kept(&name, &token, &text);
                match took {
                    Took::Taken(input) => {
                        self.one.lock().unstarted = None;
                        let turns = self.counted(&agent, place, &text, Some(input));
                        self.one.handed_over(&agent, base);
                        return Sent::Typed(turns);
                    }
                    // Busy, or its engine not ready yet: the task stays in
                    // lamp-tasks' own queue, tried again until its end, or
                    // until what it waits on stops moving (#7906 sign 4).
                    Took::NotIdle | Took::EngineNotReady => {
                        if ended() || Instant::now() >= until {
                            return self
                                .fallback("the kept session stayed busy until the run's end");
                        }
                        let busy = matches!(took, Took::NotIdle);
                        if let Some(gone) = self.refused_stuck(harness, &agent, busy, &mut refused)
                        {
                            if replaced {
                                return self.fallback(&format!(
                                    "the kept session ensured again: {}",
                                    gone.why()
                                ));
                            }
                            replaced = true;
                            self.restarted.store(true, Ordering::Release);
                            refused = None;
                            self.one.kept_gone(harness, &agent, &gone);
                            continue;
                        }
                        if self.stuck_while_refused(
                            harness,
                            place,
                            &agent,
                            &mut refused_since,
                            &mut replaced,
                        ) {
                            continue;
                        }
                        std::thread::sleep(self.one.look);
                    }
                    // Not running, or not taken in Harness's bound: ensured
                    // again once (a died pane comes back resumed).
                    Took::NotRunning | Took::Timeout if !reopened => {
                        reopened = true;
                        // A spoken task is put back while the session is
                        // ensured again, so a session that comes back
                        // `resumed` but never takes words (`starting`, `No
                        // session found` in OpenClaw's gateway) would take
                        // it back forever (#7895): bounded, and the session
                        // not let go for it once spent.
                        if !self.opens && self.one.not_up(&self.id) {
                            return self.fallback(STARTING_SPENT);
                        }
                        self.one.gone(
                            &agent,
                            "Harness did not take the words into the kept session",
                        );
                    }
                    Took::NotRunning | Took::Timeout => {
                        return self.fallback("the kept session ensured again did not take them");
                    }
                    Took::NotSent(code) if code == "reply_off" => {
                        self.one.no_reply_here();
                        return Sent::NoReply;
                    }
                    // Not ours: the pane the old way, opened at once once.
                    Took::NotSent(code) if code == "reply_not_yours" && !reopened => {
                        reopened = true;
                        self.one.not_ours("NOT_YOURS");
                        self.one
                            .gone(&agent, "Harness says the kept session is not ours");
                    }
                    Took::NotSent(code) => return self.fallback(&format!("harness reply: {code}")),
                    Took::Uncertain(code) => {
                        self.one.used(&agent, place);
                        return Sent::Uncertain(code);
                    }
                }
                continue;
            }
            // Typed only into a pane idle with its last words taken.
            match harness.observe(&place.machine, &agent) {
                // #7906 sign 2: `wait` reads a shell idle; never typed into.
                Ok(seen) if seen.shell && !replaced => {
                    replaced = true;
                    self.restarted.store(true, Ordering::Release);
                    self.one.replace_kept(
                        harness,
                        &place.machine,
                        &agent,
                        "its engine exited; the pane is a plain shell",
                    );
                    continue;
                }
                Ok(seen) if seen.shell => {
                    return self.fallback("the new pane is a plain shell too");
                }
                Ok(seen) if seen.seen.state == "idle" && seen.taken => {}
                Ok(seen) if gone_state(&seen.seen.state) && !reopened => {
                    reopened = true;
                    self.one
                        .gone(&agent, &format!("Harness said it is {}", seen.seen.state));
                    continue;
                }
                Err(code) if code == "gone" && !reopened => {
                    reopened = true;
                    self.one.gone(&agent, "Harness no longer has the pane");
                    continue;
                }
                Ok(seen) if gone_state(&seen.seen.state) => {
                    return self.fallback("the reopened pane is gone");
                }
                Err(code) if code == "gone" => return self.fallback("the reopened pane is gone"),
                // Working, untaken words, offline, starting: waited for.
                Ok(_) | Err(_) => {
                    if ended() || Instant::now() >= until {
                        return self.fallback("the pane was not ready until the run's end");
                    }
                    if self.stuck_while_refused(
                        harness,
                        place,
                        &agent,
                        &mut refused_since,
                        &mut replaced,
                    ) {
                        continue;
                    }
                    std::thread::sleep(self.one.look);
                    continue;
                }
            }
            match harness.type_into(&place.machine, &agent, &text) {
                Typed::Typed => {
                    let turns = self.counted(&agent, place, &text, None);
                    return Sent::Typed(turns);
                }
                // An acknowledged reply (openharness#4393): the turn's end
                // is read with `wait --after` this input.
                Typed::Taken(input) => {
                    let turns = self.counted(&agent, place, &text, Some(input));
                    return Sent::Typed(turns);
                }
                // Not taken in Harness's bound, nothing typed: again.
                Typed::Cancelled => {
                    if ended() || Instant::now() >= until {
                        return self
                            .fallback("the pane did not take the words until the run's end");
                    }
                    std::thread::sleep(self.one.look);
                }
                Typed::NotIdle => {
                    // Busy with another input (a person's, or the open's
                    // first turn): waited for, never typed over.
                    if ended() || Instant::now() >= until {
                        return self.fallback("the pane stayed busy until the run's end");
                    }
                    std::thread::sleep(self.one.look);
                }
                Typed::NotRunning if !reopened => {
                    reopened = true;
                    self.one
                        .gone(&agent, "Harness said the pane is not running");
                }
                Typed::NotRunning => return self.fallback("the reopened pane is not running"),
                // The person's `programs-reply` setting off: the per-task
                // way for this task and every later one.
                Typed::NotSent(code) if code == "reply_off" => {
                    self.one.no_reply_here();
                    return Sent::NoReply;
                }
                Typed::NotSent(code) => return self.fallback(&format!("harness reply: {code}")),
                Typed::Uncertain(code) => {
                    // The words may have reached the pane.
                    self.one.used(&agent, place);
                    return Sent::Uncertain(code);
                }
            }
        }
    }

    /// #7906 sign 1 while the words are refused: once refused for
    /// `open_idle_within`, one `show`; a pane gone, a plain shell or still
    /// `starting` is replaced (once per send) and `true` is returned, so
    /// the send opens a fresh one (or, for a spoken task, is put back for
    /// the one-session thread to). A healthy busy pane costs no `show`
    /// before then.
    fn stuck_while_refused(
        &self,
        harness: &dyn PaneHarness,
        place: &Place,
        agent: &str,
        refused_since: &mut Option<Instant>,
        replaced: &mut bool,
    ) -> bool {
        let since = *refused_since.get_or_insert_with(Instant::now);
        if *replaced || since.elapsed() < self.one.open_idle_within {
            return false;
        }
        *refused_since = Some(Instant::now());
        let why = match self.one.pane_sign(harness, &place.machine, agent) {
            Seen::Moving => return false,
            // No verdict: looked at again in a few seconds, never replaced.
            Seen::Unknown => {
                let again = UNKNOWN_LOOK_AGAIN.min(self.one.open_idle_within);
                *refused_since = Instant::now()
                    .checked_sub(self.one.open_idle_within - again)
                    .or(*refused_since);
                return false;
            }
            Seen::Died(why) => (why, true),
            Seen::Starting => (
                format!(
                    "still starting after {} s",
                    self.one.open_idle_within.as_secs()
                ),
                false,
            ),
        };
        *replaced = true;
        self.restarted.store(true, Ordering::Release);
        self.one
            .revive_or_replace(harness, &place.machine, agent, &why.0, why.1);
        true
    }

    /// #7906 sign 4 while the kept session refuses the hand-over: the
    /// Gateway's record of the turn it waits on read every [`TURN_WATCH`]
    /// ([`TurnWatch::refused`]; `busy`, `NOT_IDLE`: Harness says that turn
    /// is open). A turn that moves, or runs a tool, is never cut.
    fn refused_stuck(
        &self,
        harness: &dyn PaneHarness,
        agent: &str,
        busy: bool,
        refused: &mut Option<(TurnWatch, Instant)>,
    ) -> Option<KeptGone> {
        let (every, _, stall) = self.one.turn_watch;
        let now = Instant::now();
        // The first look one `every` after the first refusal.
        let (watch, next) = refused.get_or_insert_with(|| {
            (
                TurnWatch::refused(now, stall).dead_within(self.one.dead_within()),
                now + every,
            )
        });
        if now < *next {
            return None;
        }
        *next = now + every;
        let record = self.one.records(harness, agent).and_then(|read| read());
        watch.look(record.as_ref(), busy, Instant::now())
    }

    /// A turn typed: counted toward the rotation, with the input the kept
    /// session took, if any.
    fn counted(&self, agent: &str, place: &Place, text: &str, input: Option<String>) -> u64 {
        let mut inner = self.one.lock();
        let turns = inner.pane.as_mut().map_or(1, |pane| {
            pane.turns += 1;
            pane.used = Instant::now();
            pane.tokens += tokens_in(text);
            pane.input = input;
            pane.turns
        });
        drop(inner);
        self.one.used(agent, place);
        if turns > 1 {
            eprintln!(
                "tasks: one session: reused ({}) for {}",
                nth(turns),
                self.id
            );
        }
        turns
    }

    fn fallback(&self, why: &str) -> Sent {
        eprintln!("tasks: one session: {} not typed: {why}", self.id);
        Sent::NotTyped(why.to_owned())
    }

    /// The turn's end: `harness wait` on the pane, then `show`; a pane
    /// idle on another ask (this task's words not taken yet) is waited on
    /// again, so each task keeps only its own turn's answer. `cancel` set
    /// (the deadline, lamp-tasks' stop) closes the pane `--now`, since
    /// Harness has no interrupt verb, and the next open makes a new one.
    /// `before_close` runs before any close (the run's token ended first,
    /// V2's order on #6510).
    pub fn wait_end(
        &self,
        harness: &dyn PaneHarness,
        machine: &str,
        cancel: &AtomicBool,
        before_close: &dyn Fn(),
    ) -> Ended {
        let Some(agent) = self.one.agent() else {
            return Ended::Failed("the pane was let go".into());
        };
        if let Some((name, token)) = self.one.kept_now(&agent) {
            return self.wait_kept(
                harness,
                machine,
                &agent,
                Some((&name, &token)),
                cancel,
                before_close,
            );
        }
        // An acknowledged reply took this turn's words: `wait --after`.
        if self.one.took_input(&agent) {
            return self.wait_kept(harness, machine, &agent, None, cancel, before_close);
        }
        let mine = header(&self.id);
        loop {
            if cancel.load(Ordering::Acquire) {
                before_close();
                return self.cancelled(harness, machine, &agent);
            }
            match harness.wait_end(machine, &agent, cancel) {
                Ok(None) => {}
                Ok(Some(state)) if state == "idle" => match harness.observe(machine, &agent) {
                    // #7906 sign 2: `wait` read a plain shell idle.
                    Ok(PaneSeen {
                        shell: true, seen, ..
                    }) => {
                        return self.stuck(
                            harness,
                            machine,
                            &agent,
                            ("its engine exited; the pane is a plain shell", true),
                            seen.tools,
                            cancel,
                            before_close,
                        );
                    }
                    Ok(PaneSeen { seen, .. })
                        if seen.state == "idle"
                            && seen
                                .ask
                                .as_deref()
                                .is_some_and(|ask| ask.starts_with(&mine)) =>
                    {
                        if let Some(pane) = self.one.lock().pane.as_mut() {
                            pane.tokens += tokens_in(&seen.answer);
                        }
                        self.one.answered();
                        return Ended::Answered(seen.answer);
                    }
                    Ok(PaneSeen { seen, .. }) if gone_state(&seen.state) => {
                        self.one
                            .gone(&agent, &format!("Harness said it is {}", seen.state));
                        return Ended::Failed(format!(
                            "the run ended as Harness says {}",
                            seen.state
                        ));
                    }
                    Err(code) if code == "gone" => {
                        self.one.gone(&agent, "Harness no longer has the pane");
                        return Ended::Failed("the run ended as Harness says gone".into());
                    }
                    Ok(_) | Err(_) => self.rest(cancel),
                },
                Ok(Some(state)) if state == "needs-input" => {
                    // The turn's task ends as a run's own agent's ask does
                    // today; the pane is closed `--now` and opened again,
                    // so an ask no one answers never holds every later turn.
                    let _ = harness.close_now(machine, &agent);
                    self.one.closed_by_us(&agent, EngineKind::Openclaw, true);
                    self.one.gone(&agent, "its turn asked something");
                    eprintln!(
                        "tasks: one session: {}'s turn asks something; closed --now, opened again next",
                        self.id
                    );
                    return Ended::Failed(
                        "the lamp's default agent stopped to ask something, and a skill's run has no one to answer, so it was ended".into(),
                    );
                }
                Ok(Some(state)) => {
                    self.one
                        .gone(&agent, &format!("Harness said it is {state}"));
                    return Ended::Failed(format!("the run ended as Harness says {state}"));
                }
                Err(code) if code == "cancelled" => {
                    before_close();
                    return self.cancelled(harness, machine, &agent);
                }
                Err(code) => {
                    eprintln!("tasks: one session: harness wait: {code}");
                    self.rest(cancel);
                }
            }
        }
    }

    /// [`Turn::wait_end`] after an acknowledged reply (openharness#4393),
    /// into the kept session (`kept`: its name and token) or a pane of
    /// `harness new`: `harness wait --after` the input this turn's reply
    /// was taken as, so its idle is this turn's end, then `show` for its
    /// answer.
    fn wait_kept(
        &self,
        harness: &dyn PaneHarness,
        machine: &str,
        agent: &str,
        kept: Option<(&str, &Path)>,
        cancel: &AtomicBool,
        before_close: &dyn Fn(),
    ) -> Ended {
        let input = self
            .one
            .lock()
            .pane
            .as_ref()
            .and_then(|pane| pane.input.clone());
        let Some(input) = input else {
            return Ended::Failed("the kept session took no input for this turn".into());
        };
        // #7906 sign 4: counted from Harness's `Taken`, across Harness's
        // holds.
        let (_, start_within, stall) = self.one.turn_watch;
        let handed = self
            .one
            .lock()
            .pane
            .as_ref()
            .and_then(|pane| pane.handed.clone());
        let (since, base) = handed.unwrap_or_else(|| (Instant::now(), None));
        let watch = Mutex::new(
            TurnWatch::taken(since, base, start_within, stall).dead_within(self.one.dead_within()),
        );
        loop {
            if cancel.load(Ordering::Acquire) {
                before_close();
                return self.cancelled(harness, machine, agent);
            }
            let waited = match kept {
                Some((name, token)) => {
                    match self.watched_wait(harness, agent, (name, token), &input, cancel, &watch) {
                        Ok(waited) => waited,
                        // #7906 sign 4: handed over, not moving: the one
                        // replace path, with its put-back (no tool ran:
                        // typed again once; a tool ran: not replayed).
                        Err(gone) => {
                            let tools = match harness.observe(machine, agent) {
                                Ok(seen) => seen.seen.tools,
                                // Unread: taken as having run one, so it
                                // is never replayed blindly.
                                Err(_) => 1,
                            };
                            return self.stuck(
                                harness,
                                machine,
                                agent,
                                (&gone.why(), false),
                                tools,
                                cancel,
                                before_close,
                            );
                        }
                    }
                }
                None => harness.wait_after(machine, agent, &input, cancel),
            };
            match waited {
                Ok(None) => {}
                Ok(Some(state)) if state == "idle" => match harness.observe(machine, agent) {
                    Ok(PaneSeen {
                        shell: true, seen, ..
                    }) => {
                        return self.stuck(
                            harness,
                            machine,
                            agent,
                            ("its engine exited; the pane is a plain shell", true),
                            seen.tools,
                            cancel,
                            before_close,
                        );
                    }
                    Ok(PaneSeen { seen, .. }) => {
                        if let Some(pane) = self.one.lock().pane.as_mut() {
                            pane.tokens += tokens_in(&seen.answer);
                        }
                        self.one.answered();
                        return Ended::Answered(seen.answer);
                    }
                    Err(code) if code == "gone" => {
                        self.one.gone(agent, "Harness no longer has the pane");
                        return Ended::Failed("the run ended as Harness says gone".into());
                    }
                    Err(_) => self.rest(cancel),
                },
                Ok(Some(state)) if state == "needs-input" => {
                    match kept {
                        Some((name, token)) => {
                            let _ = harness.close_kept(name, token, false);
                            self.one.kept_closed(agent, EngineKind::Openclaw);
                        }
                        None => {
                            let _ = harness.close_now(machine, agent);
                            self.one.closed_by_us(agent, EngineKind::Openclaw, true);
                        }
                    }
                    self.one.gone(agent, "its turn asked something");
                    eprintln!(
                        "tasks: one session: {}'s turn asks something; closed and its conversation deleted, opened again next",
                        self.id
                    );
                    return Ended::Failed(
                        "the lamp's default agent stopped to ask something, and a skill's run has no one to answer, so it was ended".into(),
                    );
                }
                Ok(Some(state)) => {
                    self.one.gone(agent, &format!("Harness said it is {state}"));
                    return Ended::Failed(format!("the run ended as Harness says {state}"));
                }
                Err(code) if code == "cancelled" => {
                    before_close();
                    return self.cancelled(harness, machine, agent);
                }
                Err(code) => {
                    eprintln!("tasks: one session: harness wait --after: {code}");
                    self.rest(cancel);
                }
            }
        }
    }

    /// #7906: the turn's session died under it. The owner's rule: never
    /// lose the task. Reopened the lightest way
    /// ([`OneSession::revive_or_replace`]) and, once, while the turn shows
    /// no tool run (`tools`, `show`'s count: no side effect yet), its words
    /// typed again into the session reopened, still holding the queue (the
    /// tasks behind it keep their order) and its run's token. A turn that
    /// had run a tool is not replayed: it ends saying so, so the person can
    /// ask again. A second death ends it with its one line.
    #[allow(clippy::too_many_arguments)]
    fn stuck(
        &self,
        harness: &dyn PaneHarness,
        machine: &str,
        agent: &str,
        (why, died): (&str, bool),
        tools: u64,
        cancel: &AtomicBool,
        before_close: &dyn Fn(),
    ) -> Ended {
        let sent = self.sent.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let again = tools == 0
            && !cancel.load(Ordering::Acquire)
            && !self.retried.swap(true, Ordering::AcqRel);
        if let (true, Some((place, words, until))) = (again, sent) {
            self.one
                .revive_or_replace(harness, machine, agent, why, died);
            self.restarted.store(true, Ordering::Release);
            eprintln!(
                "tasks: one session: {}: its session died ({why}); its words typed again once into the reopened one (#7906)",
                self.id
            );
            let ended = || cancel.load(Ordering::Acquire);
            return match self.send(harness, &place, &words, until, &ended) {
                Sent::Typed(_) => self.wait_end(harness, &place.machine, cancel, before_close),
                _ => Ended::Failed(format!(
                    "the lamp's agent session got stuck ({why}); it was restarted, but the task could not be handed to it again"
                )),
            };
        }
        // Ended here: the run's token first (V2's order on #6510).
        before_close();
        self.one
            .revive_or_replace(harness, machine, agent, why, died);
        Ended::Failed(if tools > 0 {
            format!(
                "the lamp's agent session got stuck ({why}) after the task had started work, so it was not run again; ask again"
            )
        } else {
            format!(
                "the lamp's agent session got stuck again ({why}); it was restarted, but the task was not done"
            )
        })
    }

    /// #7906 sign 4: `harness wait --after` the kept session's input, the
    /// Gateway's record read beside it every [`TURN_WATCH`] by `watch`
    /// (the wait still held: the turn is open). `Err` once it is gone, the
    /// wait cut then; the turn's end, `cancel` or Harness's hold come back
    /// as the wait's own answer, as soon as they come. No CLI call is
    /// added: one wait, read-only SQLite reads beside it.
    fn watched_wait(
        &self,
        harness: &dyn PaneHarness,
        agent: &str,
        (name, token): (&str, &Path),
        input: &str,
        cancel: &AtomicBool,
        watch: &Mutex<TurnWatch>,
    ) -> Result<Result<Option<String>, String>, KeptGone> {
        let every = self.one.turn_watch.0;
        let Some(read) = self.one.records(harness, agent) else {
            return Ok(harness.wait_kept(name, token, input, cancel));
        };
        let cut = AtomicBool::new(false);
        let done = AtomicBool::new(false);
        let verdict = Mutex::new(None);
        let waited = std::thread::scope(|scope| {
            let (cut, done, verdict) = (&cut, &done, &verdict);
            scope.spawn(move || {
                let mut next = Instant::now() + every;
                while !done.load(Ordering::Acquire) {
                    if cancel.load(Ordering::Acquire) {
                        cut.store(true, Ordering::Release);
                        return;
                    }
                    if Instant::now() >= next {
                        let record = read();
                        let now = Instant::now();
                        next = now + every;
                        let gone = watch
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .look(record.as_ref(), true, now);
                        if let Some(gone) = gone {
                            *verdict
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(gone);
                            cut.store(true, Ordering::Release);
                            return;
                        }
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
            });
            let waited = harness.wait_kept(name, token, input, cut);
            done.store(true, Ordering::Release);
            waited
        });
        match verdict
            .into_inner()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
        {
            Some(gone) => Err(gone),
            None => Ok(waited),
        }
    }

    fn rest(&self, cancel: &AtomicBool) {
        let until = Instant::now() + self.one.look;
        while Instant::now() < until && !cancel.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn cancelled(&self, harness: &dyn PaneHarness, machine: &str, agent: &str) -> Ended {
        // The kept session: closed `--now` (its turn may still be working,
        // and Harness refuses a plain close then), its conversation
        // deleted, ensured anew next.
        let closed = match self.one.kept_now(agent) {
            Some((name, token)) => {
                let closed = harness
                    .close_kept(&name, &token, true)
                    .map(|()| Closed::Closed);
                match &closed {
                    Ok(_) => self.one.kept_closed(agent, EngineKind::Openclaw),
                    // Not closed (Harness busy, or no answer): the record
                    // and the conversation are kept, and the close `--now`
                    // is tried again at the next look as a due removal
                    // (First's and El's P2 on #7737), so the next make
                    // never resumes a session still working.
                    Err(_) => {
                        let mut inner = self.one.lock();
                        inner.drop_due = Some(format!("{}'s turn was cancelled", self.id));
                        inner.drop_now = true;
                        inner.drop_keeps_token = true;
                    }
                }
                closed
            }
            None => {
                let closed = harness.close_now(machine, agent);
                self.one.closed_by_us(agent, EngineKind::Openclaw, true);
                closed
            }
        };
        self.one
            .gone(agent, &format!("{}'s turn was cancelled", self.id));
        match closed {
            Ok(_) => eprintln!(
                "tasks: one session: closed --now after {}'s turn was cancelled; opened again next",
                self.id
            ),
            Err(why) => eprintln!(
                "tasks: one session: not closed after {}'s turn was cancelled: {why}",
                self.id
            ),
        }
        Ended::Cancelled
    }
}

impl Turn<'_> {
    /// A spoken task's turn kept past this call: the pane stays held until
    /// the store has the task's answer ([`OneSession::release`]).
    pub fn keep(self) {
        let mut inner = self.one.lock();
        if inner.holder.as_deref() == Some(self.id.as_str()) {
            inner.held_spoken = true;
        }
    }
}

impl Drop for Turn<'_> {
    fn drop(&mut self) {
        let mut inner = self.one.lock();
        if inner.held_spoken {
            return;
        }
        if inner.holder.as_deref() == Some(self.id.as_str()) {
            inner.holder = None;
        }
        drop(inner);
        self.one.turned.notify_all();
    }
}

/// `text`'s tokens, estimated at 4 characters a token.
fn tokens_in(text: &str) -> u64 {
    (text.chars().count() as u64).div_ceil(4)
}

/// `n` as "<n>th turn".
pub fn nth(n: u64) -> String {
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix} turn")
}

#[cfg(test)]
mod tests;
