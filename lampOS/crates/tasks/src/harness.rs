//! Bounded adapter for the provisioned Harness CLI: the inspected 0e6b16d
//! contract, with Harness 0.4.2's `--machine` for `new`, `show`, `ps` and
//! `wait` (Harness's #1333, "read and wait on its harnesses"). Call only
//! after the caller has durably recorded its attempt.
//! A returned launch or transcript is never proof of task completion or speech.
//! No mutation is retried, and killing this CLI cannot cancel remote work.
use rustix::{
    event::{PollFd, PollFlags, Timespec, poll},
    fs::{OFlags, fcntl_getfl, fcntl_setfl},
    process::{Pid, Signal, WaitId, WaitIdOptions, kill_process_group, waitid},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs,
    io::{self, Read},
    os::{
        fd::AsFd,
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, PermissionsExt},
            process::CommandExt,
        },
    },
    path::{Path, PathBuf},
    process::{Child, ChildStderr, ChildStdout, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

pub const MAX_PROMPT_BYTES: usize = 2000;
/// The most a task takes in a request line once JSON-escaped (P3 on #3239):
/// a control character escapes to 6 bytes, so 2,000 of them would overflow
/// the 8,192-byte line. Twice the raw bound, so that quotes, backslashes,
/// newlines and tabs, at 2 bytes each, still fit.
pub const MAX_ENCODED_PROMPT_BYTES: usize = 2 * MAX_PROMPT_BYTES;

/// Whether a new task can be taken: at most [`MAX_PROMPT_BYTES`] as
/// written, and at most [`MAX_ENCODED_PROMPT_BYTES`] as the request line
/// carries it. The engine asks this where a task is prepared, and the voice
/// before it sends anything. A stored task is held to the raw bound only, so
/// a store an earlier build wrote still opens.
pub fn prompt_fits(prompt: &str) -> bool {
    prompt.len() <= MAX_PROMPT_BYTES
        && serde_json::to_string(prompt)
            .is_ok_and(|encoded| encoded.len() - 2 <= MAX_ENCODED_PROMPT_BYTES)
}
pub const MAX_ANSWER_BYTES: usize = 16 * 1024;
pub const MAX_ASK_BYTES: usize = 4 * 1024;
const STDOUT_LIMIT: usize = 64 * 1024;
/// `ps` lists every harness on a machine with its progress line: more than
/// one agent's answer, so it is given more room, still bounded.
pub const PS_STDOUT_LIMIT: usize = 256 * 1024;
/// No more harnesses than this are taken from one `ps` reply.
pub const MOST_LISTED: usize = 256;
/// A progress line longer than this is cut, at a whole character. Harness
/// 0.4.2 makes one of at most three events of 140 characters each.
pub const MOST_ABOUT_BYTES: usize = 2048;
const STDERR_LIMIT: usize = 16 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(10);
const DRAIN_LIMIT: Duration = Duration::from_millis(100);
const REAP_LIMIT: Duration = Duration::from_millis(200);
const ENVIRONMENT: &[&str] = &[
    "HOME",
    "PATH",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "TMPDIR",
    "TMP",
    "TEMP",
    "USER",
    "LOGNAME",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_CACHE_HOME",
    "XDG_RUNTIME_DIR",
    // The service user's own systemd user bus, which `harness start` and
    // `stop` reach through `systemctl --user` (Harness 0.5.42, #5385).
    "DBUS_SESSION_BUS_ADDRESS",
    "ADAPTER_DATA_DIR",
    "HARNESS_AUTH_DIR",
    "PORT",
    // The update hold (`LAMP_HARNESS_HOLD_UPDATES`, crate::hold, #5229).
    crate::hold::DISABLE,
    crate::hold::ENV_FILE,
    // Where Harness finds the lamp's own Hermes' sessions
    // (`LAMP_HERMES_ON_LAMP`, crate::hold::HERMES_HOME, #5978).
    crate::hold::HERMES_HOME,
];

/// The agents a target may name. `opencode` since Harness 0.4.2 names it
/// in `harness new --help` ("claude (default), codex, opencode, …"); its ask
/// mode passes OpenCode no flag, so OpenCode asks as it does by default.
/// `devin` (#1854) is Devin's own terminal program on the computer, read as
/// the others are (openharness `cli/src/engines/devin/`). `devin-cloud` is
/// Devin Cloud, which Harness 0.4.27's `new` starts (openharness#1444): it
/// runs in Devin's cloud, under the computer that holds the person's Devin
/// connection, with no folder and no modes, and uses the secrets the person
/// keeps in Devin (a Slack token among them); the lamp holds none.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EngineKind {
    Codex,
    Claude,
    Opencode,
    Devin,
    #[serde(rename = "devin-cloud")]
    DevinCloud,
    /// Hermes (Nous Research's agent), which Harness launches with its first
    /// prompt as `hermes chat -q` and no permission modes (openharness
    /// `cli/src/lib/engineLaunch.ts`). Harness's other agents (Cursor,
    /// Copilot, Grok, Amp, Kilo …) take no first prompt there
    /// (`FIRST_PROMPT_ARGS` null, `PROMPT_UNSUPPORTED`), so no work can be
    /// handed to them and none is a target.
    Hermes,
}

impl EngineKind {
    pub(crate) fn argument(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Opencode => "opencode",
            Self::Devin => "devin",
            Self::DevinCloud => "devin-cloud",
            Self::Hermes => "hermes",
        }
    }

    /// The engine's name on the bus (`tasks/status`), whose check takes
    /// letters, digits and `_` only (`msgs::tasks`): `devin_cloud` for Devin
    /// Cloud, whose name for Harness has a dash (the reviewer on #2240).
    pub fn bus_name(self) -> String {
        self.argument().replace('-', "_")
    }

    /// Whether the agent runs in its maker's cloud, not on the computer: no
    /// folder is sent, its id is Harness's `cloud_…` one, and Harness closes
    /// none of them.
    pub fn cloud(self) -> bool {
        matches!(self, Self::DevinCloud)
    }

    /// Whether the agent has Harness's permission modes. Devin and Hermes have
    /// none: Harness leaves the mode out for them and refuses one they are
    /// sent (`INVALID_PERMISSION_MODE`; openharness `cli/src/lib/newCommand.ts`,
    /// `PERMISSION_MODES`), so they open in their own default.
    fn has_modes(self) -> bool {
        !matches!(self, Self::Devin | Self::DevinCloud | Self::Hermes)
    }

    /// Whether Harness starts this agent with no first message: Devin on
    /// the computer ("Devin cannot start with a first message", Harness
    /// 0.5.6, 29 Oct). Its task is typed in with `harness reply` once it is
    /// idle ([`Cli::first_message`]).
    fn typed_after(self) -> bool {
        matches!(self, Self::Devin)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub machine_id: String,
    pub project: String,
    pub engine: EngineKind,
}

/// Strict when stored too: a task's `prepared_digest` is the person's yes
/// to this exact launch, so a later field must never be passed over; a new
/// one bumps the tasks state's `schema` (Jarvis on #1512).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Launch {
    pub name: String,
    pub prompt: String,
    pub target: Target,
    /// The session the person named for this work, its name as they said it
    /// ("docs", "docs update"), #5535: with `serve --reply-named` the words go into that
    /// session on the target's computer, never a new one
    /// ([`Cli::reply_named`]). Absent when none was named, and then the
    /// launch, its digest and the stored task are as before; a stored task
    /// that has one needs schema 5.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LaunchOutcome {
    Created {
        agent_id: String,
    },
    /// The task's words were typed into a session the person already had,
    /// which Harness's router chose (`serve --route`) or the person named
    /// (`serve --reply-named`): its agent is that session, which the lamp
    /// never closes.
    Joined {
        agent_id: String,
        /// Which of the person's sessions it is, as they would know it, for
        /// one they named ([`Cli::reply_named`]); none for the router's.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session: Option<JoinedSession>,
    },
    /// `LAMP_HERMES_ON_LAMP` (#5978): as [`Self::Joined`], into a session
    /// the person named that was found on another machine than the
    /// launch's own: a hand-over to the lamp's own target looks among the
    /// computers' sessions too ([`SessionMachines`]). The task is followed
    /// there ([`JoinedOn`]).
    JoinedOn {
        agent_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session: Option<JoinedSession>,
        on: JoinedOn,
    },
    Pending {
        creation_id: String,
    },
    NotSent {
        code: String,
    },
    Uncertain {
        code: String,
    },
}

/// The machine a named session was found on, when it is not the launch's
/// own (#5978): its target's alias, its machine, and the engine that
/// target names. The task is shown, watched and closed there, and told as
/// that machine's.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct JoinedOn {
    pub alias: String,
    pub machine_id: String,
    pub engine: EngineKind,
}

/// The machines a named hand-over to the lamp's own target looks among
/// for the session (#5978, `LAMP_HERMES_ON_LAMP`): the lamp's own first,
/// then each computer target's, by alias, with its engine. Kept by
/// lamp-tasks from its targets as it reads them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionMachines {
    pub lamp: Option<String>,
    pub computers: Vec<(String, String, EngineKind)>,
}

impl SessionMachines {
    /// From the targets: the lamp's own ([`crate::runtime::LAMP_TARGET`],
    /// Hermes) and every other on a computer (not a cloud agent), each
    /// machine once.
    pub fn from_targets(targets: &BTreeMap<String, Target>) -> Self {
        let lamp = targets
            .get(crate::runtime::LAMP_TARGET)
            .filter(|target| target.engine == EngineKind::Hermes)
            .map(|target| target.machine_id.clone());
        let mut computers: Vec<(String, String, EngineKind)> = Vec::new();
        for (alias, target) in targets {
            if Some(&target.machine_id) == lamp.as_ref()
                || target.engine.cloud()
                || computers.iter().any(|(_, id, _)| *id == target.machine_id)
            {
                continue;
            }
            computers.push((alias.clone(), target.machine_id.clone(), target.engine));
        }
        Self { lamp, computers }
    }
}

/// The person's own session a named hand-over was typed into, as they
/// would know it: its name, or its folder's last part when it has none,
/// never a lamp-made `lamp-<hex>` one, and the first thing it was asked, cut
/// at a word. Kept on the task so the voice says which session took the
/// work and a wrong join is heard at once (the Reviewer-lead's P2 on #5784
/// at 71bf964baa). At least one is there.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct JoinedSession {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask: Option<String>,
}

/// How closing one harness this lamp started went (#1390).
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Closed {
    /// Saved and stopped; its conversation is kept, so the person can
    /// resume it.
    Closed,
    /// Read just before, and already stopped or failed: nothing to close.
    Gone,
    /// Read just before, and no longer idle on the turn the lamp told: the
    /// person went on with it at the computer, so it is theirs now and is
    /// never closed.
    Left,
    /// Refused, not to be asked again (exit 5: `NOT_YOURS`, `SPAWN_LIMIT`
    /// or `TURNED_OFF`).
    Refused,
}

/// One harness as `harness ps --json` lists it: only what progress needs.
/// `about` is the agent's own words; they stay in the tasks part.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    pub agent_id: String,
    pub state: String,
    pub last_active_unix_ms: Option<u64>,
    pub about: Option<String>,
}

/// One harness `harness wait --any` came back for: its agent id and how it
/// ended (`idle`, `needs-input`, `stopped`, `gone`, `closed` or `failed`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ended {
    pub agent_id: String,
    pub state: String,
}

/// How long one `harness wait` is held before it is started again, and how
/// much longer than that the adapter allows it before it is killed.
pub const WAIT_HELD: Duration = Duration::from_secs(600);
const WAIT_SLACK: Duration = Duration::from_secs(30);
/// How long `harness start` is given: Harness itself waits up to 60 s for
/// the daemon it started to claim its pid file (`PLATFORM_START_WAIT_MS`),
/// longer than [`Config::read_timeout`] may be (the reviewer's P3 on #2309).
pub const DAEMON_START_TIMEOUT: Duration = Duration::from_secs(75);
/// How long the read just before a close ([`Cli::observe_to_close`]) is
/// given, beyond [`CliConfig::read_timeout`]'s 10 s. On a lamp on 3 Nov,
/// `harness show` against the person's Mac took 16 to 39 s: every read
/// before a close ran out at 10 s, so no harness was ever closed and they
/// were left open on the Mac. The read still decides alone whether to
/// close: a read that runs out is a failed close, never an idle agent.
/// Every other read keeps the 10 s bound.
pub const CLOSE_READ_TIMEOUT: Duration = Duration::from_secs(45);
/// The whole close, its read and `harness close` together
/// ([`Cli::close_within`]): the job lane is held no longer than one
/// [`CliConfig::launch_timeout`] by default, so a hand-over the person just
/// said yes to waits behind a close no longer than behind a launch.
pub const CLOSE_BUDGET: Duration = Duration::from_secs(60);
/// No more agents than this are waited on in one `wait`.
pub const MOST_WAITED: usize = 64;
/// How long one `harness watch` is held (its own `--timeout`, exit 124)
/// before it is started again, and how much longer the adapter allows it
/// before it is killed (`serve --push-watch`). A start again is safe: its
/// first line is how the harness stands then.
pub const WATCH_HELD: Duration = Duration::from_secs(600);
const WATCH_SLACK: Duration = Duration::from_secs(30);
/// The longest one line of `harness watch --json` may be: a `done` line
/// carries the last turn's summary.
pub const WATCH_LINE_LIMIT: usize = 64 * 1024;
/// `watch --chars`: how much of a summary each `done` line carries. None of
/// it is read.
pub const WATCH_CHARS: usize = 200;

/// Takes each whole stdout line as it comes (`watch`); `true` ends the run.
type LineSink<'a> = dyn FnMut(&[u8]) -> bool + 'a;

/// One event `harness watch --json` prints (openharness `watch`): a line
/// only when the event or the turn changes, Harness looking every 3 s.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchEvent {
    Started,
    Working,
    /// Waits for the person (`needs_person`).
    NeedsPerson,
    /// The turn ended, after two idle looks. `watch` goes on: a pane
    /// harness stays idle, so the caller ends it.
    Done,
    /// Idle, never seen working with a turn unanswered.
    Idle,
    Failed,
    /// Stopped; `watch` exits 4 after it.
    Stopped,
}

impl WatchEvent {
    fn of(event: &str) -> Option<Self> {
        Some(match event {
            "started" => Self::Started,
            "working" => Self::Working,
            "needs_person" => Self::NeedsPerson,
            "done" => Self::Done,
            "idle" => Self::Idle,
            "failed" => Self::Failed,
            "stopped" => Self::Stopped,
            _ => return None,
        })
    }
    /// The word a journal line says.
    pub fn name(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Working => "working",
            Self::NeedsPerson => "needs_person",
            Self::Done => "done",
            Self::Idle => "idle",
            Self::Failed => "failed",
            Self::Stopped => "stopped",
        }
    }
    /// Whether the agent has settled, for now or for good: its task is read
    /// at once.
    pub fn settled(self) -> bool {
        !matches!(self, Self::Started | Self::Working)
    }
}

/// How one `harness watch` ended ([`Cli::watch`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WatchEnd {
    /// `done` or `failed` came, and the caller's watch was killed then.
    Settled,
    /// The harness stopped (exit 4).
    Stopped,
    /// Its own `--timeout` passed (exit 124): started again at once.
    Timeout,
    /// Cancelled by the caller: the task left the watch, or the part stops.
    Cancelled,
    /// A usage error (exit 2): taken for a Harness without `watch`.
    Missing,
    /// Any other end: Harness's code when it gave one (`GONE`,
    /// `DISCONNECTED` …), else the adapter's (`watch_failed`, `timeout` …).
    Failed(String),
}
/// How long `harness machines engines` is given (`serve --engines-ready`):
/// the computer's Harness reads the one engine in its login shell, 5 s at
/// most there (openharness `engineProbe.ts`), over the machine link.
pub const ENGINES_PROBE_TIMEOUT: Duration = Duration::from_secs(8);
/// How long one look at a computer's engines may take (`serve
/// --engines-from-machines`, #5199): Harness's own probe waits 30 s over
/// the machine link (`engines_probe`), and the CLI a little more. Asked on
/// a lane of its own, never the one hand-overs take.
pub const ENGINES_LOOK_TIMEOUT: Duration = Duration::from_secs(35);
/// How long one computer's answer for one engine is kept when it refuses
/// nothing: a second hand-over within it asks nothing. A refusal is never
/// kept: the person may sign in at once and ask again.
pub const ENGINES_KEPT: Duration = Duration::from_secs(300);
/// The most a route may take, of the launch's bound: Harness's router
/// answers in 0.4 to 0.7 s and is bounded at 8 s; from a lamp, the read of
/// that computer's sessions and the typing in come on top.
pub const ROUTE_TIMEOUT: Duration = Duration::from_secs(20);
/// The longest session words a launch may carry (#5535): a name or a few
/// words said for one, never a task.
pub const MOST_SESSION_BYTES: usize = 128;
/// How often a Devin just started is looked at until it is idle, for its
/// first message ([`Cli::first_message`]).
const FIRST_MESSAGE_LOOK: Duration = Duration::from_millis(500);

/// Strict when stored too: an observation's `digest` is over what this
/// build would write of it, so a later field would not match it anyway; a
/// new one bumps the tasks state's `schema` (Jarvis on #1512). Harness's own
/// reply is read as `ShowReply`, not as this.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub agent_id: String,
    pub state: String,
    pub ask: Option<String>,
    pub answer: String,
    pub truncated: bool,
    pub tools: u64,
}

fn token(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn machine_id(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// An agent's id as Harness gives it: a UUID for one on a computer, or
/// `cloud_` and 16 hex digits for a cloud one (Harness 0.4.27).
pub fn agent_id(value: &str) -> bool {
    uuid(value)
        || value.strip_prefix("cloud_").is_some_and(|hex| {
            hex.len() == 16
                && hex
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
}

/// What Harness's `state` for agent `id` means here, with its `answer` when
/// it was read (`show`; `None` for `wait` and `ps`, which only wake and list).
/// A cloud agent is `needs-input` once it has answered and waits for a next
/// message, whether or not it asked anything, so it is read as `idle`, and
/// its answer is told; a question it asked is in that answer. A Devin Cloud
/// session that has finished ends `stopped` (lamp-75c9, 20 Oct 18:19: it
/// had replied in Slack), so a cloud agent `stopped` with an answer is read
/// as `idle` too; one stopped with none is as Harness says, so it is told as
/// stopped and never waited on. `working` and `failed` are as Harness says.
/// When openharness#1449 gives a way to tell a question from a finished
/// answer, read that here instead.
fn state_of(id: &str, state: String, answer: Option<&str>) -> String {
    let answered = answer.is_none_or(|answer| !answer.trim().is_empty());
    match state.as_str() {
        "needs-input" if id.starts_with("cloud_") => "idle".into(),
        "stopped" if id.starts_with("cloud_") && answered => "idle".into(),
        _ => state,
    }
}

fn uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        })
}

impl Target {
    pub fn validate(&self) -> Result<(), String> {
        if !machine_id(&self.machine_id) {
            return Err("invalid_machine".into());
        }
        // A cloud agent works in no folder: its target names none.
        if self.engine.cloud() {
            return if self.project.is_empty() {
                Ok(())
            } else {
                Err("invalid_project".into())
            };
        }
        let folder = self.project.starts_with('/') || self.project.starts_with("~/");
        let maximum = if folder { MOST_FOLDER_BYTES } else { 128 };
        if self.project.is_empty()
            || self.project.len() > maximum
            || self.project.trim() != self.project
            || self.project.chars().any(char::is_control)
            || self.project.starts_with(['-', '@'])
            || (!folder && (self.project.starts_with(['.', '~']) || self.project.contains('/')))
        {
            return Err("invalid_project".into());
        }
        Ok(())
    }
}

impl Launch {
    pub fn validate(&self) -> Result<(), String> {
        self.target.validate()?;
        if !token(&self.name, 80) || !self.name.as_bytes()[0].is_ascii_alphanumeric() {
            return Err("invalid_name".into());
        }
        // The pinned --task parser refuses a value beginning with --. Do not
        // rewrite the task or silently select another CLI permission mode.
        if self.prompt.trim().is_empty()
            || self.prompt.len() > MAX_PROMPT_BYTES
            || self.prompt.starts_with("--")
            || self.prompt.contains('\0')
        {
            return Err("invalid_prompt".into());
        }
        if let Some(session) = &self.session
            && (session.trim().is_empty()
                || session.len() > MOST_SESSION_BYTES
                || session.chars().any(char::is_control))
        {
            return Err("invalid_session".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct CliConfig {
    /// Where the service user's runtime folder is looked for, at each call
    /// ([`with_user_bus`]): `/run/user`, swapped only in tests.
    pub user_runtime_root: PathBuf,
    pub executable: PathBuf,
    pub launch_timeout: Duration,
    pub read_timeout: Duration,
    pub environment: BTreeMap<String, OsString>,
    pub cancel: Option<Arc<AtomicBool>>,
    /// Call Harness as the person, without marking the caller as an agent
    /// (`--harness-as-person`). Off unless set: marked, Harness refuses to
    /// start a harness until the person lets agents start agents (seen on
    /// lamp-4ace on 2026-10-14). The person's spoken yes to each exact task
    /// is still required either way.
    pub as_person: bool,
    /// A hand-over Harness refuses for a folder that isn't on the computer
    /// ends `not_sent` with `folder_missing`, so the voice can say so
    /// (`serve --folder-missing`, #2840). Off unless set: `uncertain` with
    /// `new_failed`, as any failed `new`.
    pub folder_missing: bool,
    /// Before each hand-over, the computer is asked whether the named
    /// engine is signed in and takes a first prompt (`harness machines
    /// engines`, openharness#1788), an answer that refuses nothing kept
    /// [`ENGINES_KEPT`]; a known no ends
    /// the task `not_sent` with `engine_signed_out` or `engine_no_prompt`,
    /// so the voice can say so (`serve --engines-ready`). Off unless set:
    /// nothing is asked, and the hand-over goes as before.
    pub engines_ready: bool,
    /// Before `new`, Harness's router is asked whether the task belongs to a
    /// session the person already has on the target's computer, and the
    /// words are typed into it there (`harness route --send`,
    /// [`Cli::route_first`]). Off unless set (`serve --route`): `new`, as
    /// before.
    pub route: bool,
    /// A task whose launch names a session (#5535) is typed into that
    /// session on the target's computer with `harness reply`, found among
    /// that computer's sessions by name or folder ([`Cli::reply_named`]);
    /// it is never started new. Off unless set (`serve --reply-named`): a
    /// launch that names a session is not sent (`reply_named_off`).
    pub reply_named: bool,
    /// A `~/…` target folder is sent as the absolute folder on that
    /// computer once its home is known, learned from the folder an earlier
    /// `new` there reports it started in ([`Cli::folder_of`], P4 step 1).
    /// Harness asks the computer for its home on each `~/…` folder
    /// (`fs_list_dir`, one more round trip through the relay); an absolute
    /// one is sent as it is. Off unless set (`serve --absolute-folder`): the
    /// folder is sent as the target names it, as before.
    pub absolute_folder: bool,
    /// `serve --new-folder` (subdarren-P3 part 1, off unless set): the file
    /// in the tasks folder where the folders this lamp has seen are kept,
    /// one per computer ([`NewFolder`]). A target whose folder is
    /// `~/harnesses/<name>`, the one Harness makes for the project name
    /// `<name>`, has its first hand-over sent as `--new <name>`, so Harness
    /// makes the folder and trusts it, and Claude Code doesn't stop at "Do you
    /// trust this folder?" with nobody at the computer (lamp-c0a4, 3 Nov).
    /// `None`: the folder, as before.
    pub new_folder: Option<PathBuf>,
    /// With [`Self::new_folder`] set, only a target naming this folder has
    /// it made; `None`: every `~/harnesses/<name>` folder. `serve
    /// --targets-from-machines` sets it to the folder its targets name
    /// ([`crate::runtime::MACHINES_PROJECT`]) when `--new-folder` is off, so
    /// a target it wrote always gets its folder (#5541) and a hand-written
    /// one goes as before.
    pub new_folder_only: Option<String>,
    /// Each dispatch's journal line ([`dispatch_line`]) is also kept here,
    /// for a test to read; it always goes to stderr, the unit's journal.
    pub journal: Option<Arc<Mutex<Vec<String>>>>,
    /// Each Harness call's journal line ([`call_line`]) is also kept here,
    /// for a test to read; it always goes to stderr, the unit's journal.
    pub calls: Option<Arc<Mutex<Vec<String>>>>,
    /// `LAMP_HERMES_ON_LAMP` (#5978): the machines a named hand-over to the
    /// lamp's own target looks among ([`SessionMachines`]). `None`: only
    /// the launch's own machine, as before.
    pub session_machines: Option<Arc<Mutex<SessionMachines>>>,
    /// After each `harness start` that succeeded, Harness's `/api/health`
    /// is asked, with no credential, in a thread of its own, and one line
    /// journaled ([`crate::hold::watch_health`], #5229). Set with the update
    /// hold (`LAMP_HARNESS_HOLD_UPDATES=on`); off unless set.
    pub health_after_start: bool,
}

/// Where systemd's logind makes each user's runtime folder.
pub const USER_RUNTIME_ROOT: &str = "/run/user";

/// The service user's systemd user manager, for every Harness call: Harness
/// 0.5.42's `harness start` runs `systemctl --user start harnessd.service`,
/// which fails with "Failed to connect to bus: No medium found" when the
/// caller has no `XDG_RUNTIME_DIR` (#5385, lamp-c0a4 on 5 Nov: after a
/// sign-in from the app, the daemon was never started). A system unit gives
/// its process neither variable, even with the user lingering, so each one
/// not already set is taken from the user's own runtime folder,
/// `/run/user/<uid>`, and its `bus`, only where they exist; one already set
/// is kept as it is. It is looked at for each call ([`Cli::child_environment`]),
/// not once at start: after a reboot lamp-tasks may start before logind has
/// made the lingering user's folder (#5393's reads).
pub fn with_user_bus(
    mut environment: BTreeMap<String, OsString>,
    root: &Path,
    uid: u32,
    exists: &dyn Fn(&Path) -> bool,
) -> BTreeMap<String, OsString> {
    if !environment.contains_key("XDG_RUNTIME_DIR") {
        let runtime = root.join(uid.to_string());
        if exists(&runtime) {
            environment.insert("XDG_RUNTIME_DIR".into(), runtime.into_os_string());
        }
    }
    if !environment.contains_key("DBUS_SESSION_BUS_ADDRESS")
        && let Some(runtime) = environment.get("XDG_RUNTIME_DIR")
    {
        let bus = Path::new(runtime).join("bus");
        if bus.is_absolute() && exists(&bus) {
            let mut address = OsString::from("unix:path=");
            address.push(bus.as_os_str());
            environment.insert("DBUS_SESSION_BUS_ADDRESS".into(), address);
        }
    }
    environment
}

impl CliConfig {
    pub fn new(executable: PathBuf) -> Self {
        Self {
            executable,
            launch_timeout: Duration::from_secs(60),
            read_timeout: Duration::from_secs(10),
            environment: ENVIRONMENT
                .iter()
                .filter_map(|name| std::env::var_os(name).map(|value| ((*name).into(), value)))
                .collect(),
            user_runtime_root: PathBuf::from(USER_RUNTIME_ROOT),
            cancel: None,
            as_person: false,
            folder_missing: false,
            engines_ready: false,
            route: false,
            reply_named: false,
            absolute_folder: false,
            new_folder: None,
            new_folder_only: None,
            journal: None,
            calls: None,
            session_machines: None,
            health_after_start: false,
        }
    }
}

/// Each computer's answers that refused nothing, by machine id and engine:
/// when each was had.
type Kept = BTreeMap<(String, &'static str), Instant>;

pub struct Cli {
    config: CliConfig,
    caller_session: String,
    operation: Mutex<()>,
    poisoned: AtomicBool,
    executable: ExecutableIdentity,
    /// Each computer's answer for each engine (`serve --engines-ready`).
    ready: Mutex<Kept>,
    /// Each computer's home folder, by machine id, as its own `new` reply
    /// showed it (`serve --absolute-folder`). Kept while the part runs; a
    /// `new` in it that fails forgets it, and `~/…` is sent again.
    homes: Mutex<BTreeMap<String, String>>,
    /// The folders seen on each computer (`serve --new-folder`).
    folders: Mutex<NewFolder>,
}

fn invalid(code: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, code)
}

#[derive(PartialEq, Eq)]
struct ExecutableIdentity {
    dev: u64,
    ino: u64,
    bytes: u64,
    modified: i64,
    nanos: i64,
    mode: u32,
    uid: u32,
}
fn executable_identity(path: &Path) -> io::Result<ExecutableIdentity> {
    let metadata = fs::symlink_metadata(path).map_err(|_| invalid("unavailable_executable"))?;
    if !metadata.is_file()
        || metadata.permissions().mode() & 0o111 == 0
        || metadata.permissions().mode() & 0o6022 != 0
        || ![0, rustix::process::geteuid().as_raw()].contains(&metadata.uid())
    {
        return Err(invalid("invalid_executable"));
    }
    Ok(ExecutableIdentity {
        dev: metadata.dev(),
        ino: metadata.ino(),
        bytes: metadata.len(),
        modified: metadata.mtime(),
        nanos: metadata.mtime_nsec(),
        mode: metadata.mode(),
        uid: metadata.uid(),
    })
}

impl Cli {
    /// The environment each Harness child gets: the checked one, with the
    /// service user's runtime folder and bus looked up now ([`with_user_bus`]).
    pub fn child_environment(&self) -> BTreeMap<String, OsString> {
        with_user_bus(
            self.config.environment.clone(),
            &self.config.user_runtime_root,
            rustix::process::geteuid().as_raw(),
            &|place: &Path| place.exists(),
        )
    }

    pub fn new(mut config: CliConfig, caller_session: String) -> io::Result<Self> {
        if !token(&caller_session, 80) {
            return Err(invalid("invalid_caller_session"));
        }
        if !config.executable.is_absolute()
            || config.executable.as_os_str().len() > 4096
            || config.executable.as_os_str().as_bytes().contains(&0)
        {
            return Err(invalid("invalid_executable"));
        }
        config.executable = config
            .executable
            .canonicalize()
            .map_err(|_| invalid("unavailable_executable"))?;
        let executable = executable_identity(&config.executable)?;
        if config.launch_timeout.is_zero()
            || config.launch_timeout > Duration::from_secs(60)
            || config.read_timeout.is_zero()
            || config.read_timeout > Duration::from_secs(10)
        {
            return Err(invalid("invalid_timeout"));
        }
        let mut total = 0usize;
        for (name, value) in &config.environment {
            total = total.saturating_add(name.len() + value.len());
            if !ENVIRONMENT.contains(&name.as_str())
                || value.len() > 4096
                || value.as_bytes().contains(&0)
            {
                return Err(invalid("invalid_environment"));
            }
        }
        if total > 32 * 1024 {
            return Err(invalid("invalid_environment"));
        }
        let home = config
            .environment
            .get("HOME")
            .ok_or_else(|| invalid("missing_home"))?;
        if !Path::new(home).is_absolute() {
            return Err(invalid("invalid_home"));
        }
        let path = config
            .environment
            .get("PATH")
            .ok_or_else(|| invalid("missing_path"))?;
        if path.is_empty() || std::env::split_paths(path).any(|path| !path.is_absolute()) {
            return Err(invalid("invalid_path"));
        }
        if let Some(port) = config.environment.get("PORT")
            && port
                .to_str()
                .and_then(|value| value.parse::<u16>().ok())
                .is_none_or(|port| port == 0)
        {
            return Err(invalid("invalid_port"));
        }
        if config
            .new_folder
            .as_ref()
            .is_some_and(|file| !file.is_absolute() || file.file_name().is_none())
        {
            return Err(invalid("invalid_new_folder"));
        }
        let folders = Mutex::new(NewFolder::load(config.new_folder.as_deref()));
        Ok(Self {
            config,
            caller_session,
            operation: Mutex::new(()),
            poisoned: AtomicBool::new(false),
            executable,
            ready: Mutex::new(BTreeMap::new()),
            homes: Mutex::new(BTreeMap::new()),
            folders,
        })
    }

    fn cancelled(&self, also: Option<&AtomicBool>) -> bool {
        self.config
            .cancel
            .as_ref()
            .is_some_and(|cancel| cancel.load(Ordering::Acquire))
            || also.is_some_and(|cancel| cancel.load(Ordering::Acquire))
    }

    /// Whether `target`'s folder is seen on its computer (`serve
    /// --new-folder`).
    fn folder_seen(&self, target: &Target) -> bool {
        self.folders
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(target)
    }

    /// Mark `target`'s folder seen on its computer, and keep it.
    fn folder_made(&self, target: &Target) {
        self.folders
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(target);
    }

    /// Hand `launch` over, and say how it went in one journal line
    /// ([`dispatch_line`]), whatever the outcome.
    pub fn launch(&self, launch: &Launch) -> LaunchOutcome {
        let began = Instant::now();
        let mut seen = Seen::default();
        let outcome = self.launch_seen(launch, &mut seen);
        let line = dispatch_line(launch, &outcome, &seen, began.elapsed());
        eprintln!("{line}");
        if let Some(journal) = &self.config.journal {
            journal.lock().unwrap_or_else(|e| e.into_inner()).push(line);
        }
        outcome
    }

    fn launch_seen(&self, launch: &Launch, seen: &mut Seen) -> LaunchOutcome {
        let not_sent = |code: &str| LaunchOutcome::NotSent { code: code.into() };
        let uncertain = |code: &str| LaunchOutcome::Uncertain { code: code.into() };
        if let Err(code) = launch.validate() {
            return not_sent(&code);
        }
        // A session named with the switch off: never a new one in its place.
        if launch.session.is_some() && !self.config.reply_named {
            return not_sent("reply_named_off");
        }
        let Ok(_guard) = self.operation.try_lock() else {
            return not_sent("busy");
        };
        let preflight = self.run(
            &["machines".into(), "--json".into()],
            self.config.read_timeout,
        );
        seen.took(&preflight);
        if let Ok(reply) = &preflight {
            seen.machines = machine_words(&reply.stdout);
        }
        seen.machines
            .insert(launch.target.machine_id.to_lowercase());
        let preflight = match preflight {
            Ok(reply) => reply,
            Err(error) => {
                return not_sent(&if error.code == "cancelled" {
                    "cancelled".into()
                } else {
                    format!("preflight_{}", error.code)
                });
            }
        };
        if !preflight.success {
            return not_sent("preflight_failed");
        }
        if let Err(code) = machines(&preflight.stdout, &launch.target.machine_id) {
            return not_sent(code);
        }
        // The session the person named is already there: its engine is not
        // asked about, nothing is routed, and nothing is started.
        if let Some(session) = &launch.session {
            return self.reply_named(launch, session, seen);
        }
        if self.config.engines_ready
            && let Some(code) = self.engine_refusal(&launch.target, seen)
        {
            return not_sent(code);
        }
        // This is a preflight, not a reservation. Harness resolves the account
        // list again and can fall back to a name if the ID disappears meanwhile.
        // The machine is named with `--machine` (Harness 0.4.2); the `@id`
        // form before it is refused.
        let mut args: Vec<OsString> = vec![
            "new".into(),
            launch.target.engine.argument().into(),
            "--machine".into(),
            launch.target.machine_id.clone().into(),
        ];
        // A cloud agent is given no folder (openharness#1444).
        let folder = (!launch.target.engine.cloud()).then(|| self.folder_of(&launch.target));
        let project_at = folder.as_ref().map(|folder| {
            args.push(folder.sent.clone().into());
            args.len() - 1
        });
        // `serve --new-folder`: a `~/harnesses/<name>` folder this lamp has
        // not seen on that computer is asked for as `--new <name>`, so
        // Harness makes it and trusts it ([`NewFolder`]).
        let new_name = project_at
            .filter(|_| self.config.new_folder.is_some())
            .filter(|_| {
                self.config
                    .new_folder_only
                    .as_ref()
                    .is_none_or(|only| *only == launch.target.project)
            })
            .and_then(|at| Some((at, made_name(&launch.target.project)?)))
            .filter(|_| !self.folder_seen(&launch.target));
        if let Some((at, name)) = new_name {
            args.splice(at..=at, ["--new".into(), name.into()]);
        }
        args.extend(["--name".into(), launch.name.clone().into()]);
        // Harness's default, approved by the owner on 2026-10-15: Claude
        // Code works in its own project folder without asking, and still
        // stops for anything risky. With `ask` it asked before every file
        // it wrote, with nobody at the computer to answer: a two-line poem
        // waited on "Do you want to create coffee_poem.txt?" while the
        // lamp said "still working". An agent with no modes (Devin) is
        // sent none, as Harness itself leaves it out (#1854).
        if launch.target.engine.has_modes() {
            args.extend(["--mode".into(), "auto".into()]);
        }
        args.push("--json".into());
        if !launch.target.engine.typed_after() {
            args.extend(["--task".into(), launch.prompt.clone().into()]);
        }
        // One `launch_timeout` for the whole hand-over: any route, then the
        // `new`, a retry included, and Devin's first message after it (the
        // Reviewer-lead's P3 on #4422: the two bounds no longer add).
        let began = Instant::now();
        if self.config.route
            && !launch.target.engine.cloud()
            && let Some(routed) = self.route_first(launch, seen)
        {
            return routed;
        }
        let until = began + self.config.launch_timeout;
        if let Some((_, name)) = new_name {
            eprintln!(
                "tasks: {}: the folder for '{name}' is new to this lamp on that computer, so Harness is asked to make it",
                launch.name
            );
        }
        let asked = Instant::now();
        let mut reply = self.run(&args, until.saturating_duration_since(Instant::now()));
        seen.took(&reply);
        // How long `new` took, and how its folder was sent, never the
        // folder itself (a home folder names the person): P4's before and
        // after, on one build.
        eprintln!(
            "tasks: {}: new answered in {} ms, the folder sent {}",
            launch.name,
            asked.elapsed().as_millis(),
            if new_name.is_some() {
                "as a new project (--new), so Harness makes it"
            } else {
                folder
                    .as_ref()
                    .map_or("none (a cloud agent)", |folder| folder.kind)
            },
        );
        // A `--new <name>` (`serve --new-folder`) names no folder to send
        // absolute, so its failure forgets no home; an ok one still shows
        // the computer's home, its cwd being `<home>/harnesses/<name>`.
        if let Some(folder) = &folder {
            let learned = folder.learned && new_name.is_none();
            self.learn_home(&launch.target, learned, &reply);
        }
        let left = until.saturating_duration_since(Instant::now());
        // `--new <name>` made the folder, or Harness says it is there
        // already: either way it is on that computer from now on, and later
        // hand-overs name it. A folder already there is asked once more as
        // the folder itself, as it was before `--new-folder`, in what is
        // left of the bound. Any other answer marks nothing, so the next
        // hand-over asks `--new` again.
        if let (Some((at, name)), Ok(answered)) = (new_name, &reply) {
            if answered.success {
                self.folder_made(&launch.target);
            } else if project_exists(answered) {
                self.folder_made(&launch.target);
                if !left.is_zero() {
                    eprintln!(
                        "tasks: {}: the folder for '{name}' is on that computer already, so it starts there",
                        launch.name
                    );
                    // The folder as `folder_of` chose it: absolute once the
                    // computer's home is known (`serve --absolute-folder`).
                    let plain = folder.as_ref().map_or_else(
                        || launch.target.project.clone(),
                        |folder| folder.sent.clone(),
                    );
                    args.splice(at..at + 2, [plain.into()]);
                    reply = self.run(&args, left);
                    seen.took(&reply);
                    if let Some(folder) = &folder {
                        self.learn_home(&launch.target, folder.learned, &reply);
                    }
                }
            }
        } else if let (Some(at), Ok(refused)) = (project_at, &reply)
            && !refused.success
            && !left.is_zero()
            && let Some(folder) = name_taken(&refused.stdout, &launch.target.project)
        {
            // A bare name gets its folder, `~/harnesses/<name>`, from Harness on
            // the first `new`, and every later `new` on that name is refused
            // for it, before anything starts (Harness 0.4.55; Mobile on
            // lamp-0c89, 23 Oct). On that refusal alone, asked once more in
            // the folder it names, in what is left of the bound; nothing else
            // is ever asked twice.
            eprintln!(
                "tasks: {}: the name '{}' is taken by a folder of that name on that computer, so it starts there",
                launch.name, launch.target.project
            );
            args[at] = folder.into();
            reply = self.run(&args, left);
            seen.took(&reply);
        }
        match reply {
            Ok(reply) if reply.success && launch.target.engine.typed_after() => {
                let outcome = parse_launch(&reply.stdout, launch);
                self.first_message(launch, outcome, until, seen)
            }
            Ok(reply) if reply.success => parse_launch(&reply.stdout, launch),
            Ok(reply) if self.config.folder_missing && folder_missing(&reply.stdout) => {
                not_sent("folder_missing")
            }
            Ok(_) => uncertain("new_failed"),
            Err(error) if error.spawned => uncertain(error.code),
            Err(error) => not_sent(error.code),
        }
    }

    /// Harness's router, asked first (`serve --route`): `harness route
    /// --machine <id> --json --send -- <task>`. It reads that computer's
    /// sessions and either chose one, and typed the words into it when it
    /// is idle, or says the task is new work. `None` is "start new work as
    /// before": the router said new, is off or unreachable
    /// (`ROUTER_UNAVAILABLE`), refused (`REPLY_OFF`, `NOT_YOURS`), or its
    /// pick is no longer running or its machine refused the words
    /// (`FAILED`), and nothing was typed (openharness#2246). A busy pick is
    /// `agent_busy`, not sent and never new, so the person isn't handed a
    /// second session for one task. A route that may have typed the words
    /// and gave no answer is uncertain, never sent again.
    fn route_first(&self, launch: &Launch, seen: &mut Seen) -> Option<LaunchOutcome> {
        let uncertain = |code: &str| Some(LaunchOutcome::Uncertain { code: code.into() });
        let args: Vec<OsString> = vec![
            "route".into(),
            "--machine".into(),
            launch.target.machine_id.clone().into(),
            "--json".into(),
            "--send".into(),
            "--".into(),
            launch.prompt.clone().into(),
        ];
        let began = Instant::now();
        let reply = self.run(&args, ROUTE_TIMEOUT.min(self.config.launch_timeout));
        seen.took(&reply);
        let routed = match reply {
            Ok(reply) => route_answer(&reply.stdout),
            Err(error) if error.code == "cancelled" => return uncertain("cancelled"),
            // Started, and stopped before it answered: it may have typed.
            Err(error) if error.spawned => return uncertain("route_uncertain"),
            Err(error) => Routed::New(error.code),
        };
        let ms = began.elapsed().as_millis();
        match routed {
            Routed::Sent(agent_id) => {
                eprintln!(
                    "tasks: {}: routed to a session already there, in {ms} ms",
                    launch.name
                );
                Some(LaunchOutcome::Joined {
                    agent_id,
                    session: None,
                })
            }
            Routed::Busy => {
                eprintln!(
                    "tasks: {}: the router chose a busy session, in {ms} ms: not sent",
                    launch.name
                );
                Some(LaunchOutcome::NotSent {
                    code: "agent_busy".into(),
                })
            }
            Routed::New(why) => {
                eprintln!(
                    "tasks: {}: the router says new work ({why}), in {ms} ms",
                    launch.name
                );
                None
            }
            Routed::Unsure => uncertain("route_uncertain"),
        }
    }

    /// #5535, `serve --reply-named`: the session the person named, found
    /// among the target computer's sessions (`harness ps --all`, the ones
    /// stopped this week too) by its name or its folder's, and the words
    /// typed into it with `harness reply`, as the person would type them
    /// (behind their own `programs-reply` switch). Taken, it is the task's
    /// agent (`joined`), which the lamp never closes. Nothing else is ever
    /// started in its place, so each other end is not sent, with why:
    /// `session_not_found`, `session_ambiguous` (more than one live one
    /// fits, or, by its work, a live one and a stopped one), `session_stopped` (only a stopped one fits), `session_busy`
    /// (working, or asking the person something), the reply's refusal
    /// (`reply_off`, `reply_not_idle`, `reply_not_running`, `reply_failed`),
    /// or `sessions_…` when the list can't be read. A reply that started and
    /// gave no answer, or one Harness took whose answer can't be read or
    /// names another session, may have typed, so it is uncertain
    /// (`reply_uncertain`), never sent again.
    fn reply_named(&self, launch: &Launch, session: &str, seen: &mut Seen) -> LaunchOutcome {
        let not_sent = |code: &str| LaunchOutcome::NotSent { code: code.into() };
        let uncertain = |code: &str| LaunchOutcome::Uncertain { code: code.into() };
        let began = Instant::now();
        // The machines looked among: the launch's own, and with the lamp's
        // own target (#5978) each computer's too.
        let mut machines: Vec<(String, Option<JoinedOn>)> =
            vec![(launch.target.machine_id.clone(), None)];
        if let Some(shared) = &self.config.session_machines
            && let Ok(known) = shared.lock()
            && launch.target.engine == EngineKind::Hermes
            && known.lamp.as_ref() == Some(&launch.target.machine_id)
        {
            for (alias, machine_id, engine) in &known.computers {
                machines.push((
                    machine_id.clone(),
                    Some(JoinedOn {
                        alias: alias.clone(),
                        machine_id: machine_id.clone(),
                        engine: *engine,
                    }),
                ));
            }
        }
        let mut found: Vec<(Named, Option<JoinedOn>)> = Vec::new();
        let mut unread: Option<String> = None;
        for (machine_id, on) in machines {
            let left = self.config.launch_timeout.saturating_sub(began.elapsed());
            // Another machine's list may not take the reply's own floor.
            if on.is_some() && left < REPLY_FLOOR * 2 {
                unread.get_or_insert("sessions_timeout".to_owned());
                continue;
            }
            let listed = self.run(
                &[
                    "ps".into(),
                    "--machine".into(),
                    machine_id.into(),
                    "--all".into(),
                    "--json".into(),
                ],
                // The launch's own bound, not the 10 s read bound: on
                // lamp-c0a4 on 9 Nov, `ps --all` of the person's Mac took 4.5 to
                // 10 s, and two hand-overs a person had said yes to twice ran
                // out at 10 s (`sessions_timeout`). The reply below gets what is
                // left of the same bound, and each other machine what is left.
                left,
            );
            seen.took(&listed);
            let code = match listed {
                Ok(reply) if reply.success => {
                    found.push((named_session(&reply.stdout, session), on));
                    continue;
                }
                Ok(_) => "sessions_failed".to_owned(),
                Err(error) if error.code == "cancelled" => return not_sent("cancelled"),
                Err(error) => format!("sessions_{}", error.code),
            };
            // The launch's own machine unread: nothing typed, as before.
            if on.is_none() {
                return not_sent(&code);
            }
            unread.get_or_insert(code);
        }
        let (agent_id, joined, on) = match chosen_session(found) {
            Ok((Named::Idle(agent_id, joined), on)) => (agent_id, joined, on),
            Ok((Named::Refused(code), _)) | Err(code) => {
                // Nowhere fits while a computer couldn't be read: said as
                // that, not as no such session.
                let code = match unread.as_deref() {
                    Some(unread) if code == "session_not_found" => unread.to_owned(),
                    _ => code.to_owned(),
                };
                eprintln!(
                    "tasks: {}: the named session: {code}, not sent",
                    launch.name
                );
                return not_sent(&code);
            }
        };
        let machine_id = on
            .as_ref()
            .map_or(launch.target.machine_id.clone(), |on| on.machine_id.clone());
        let left = self.config.launch_timeout.saturating_sub(began.elapsed());
        // A list that took nearly the whole bound leaves the reply no time
        // to answer, and a reply cut short may have typed: under
        // `REPLY_FLOOR` left, nothing is typed (the Reviewer-lead's P3 on
        // #5784 at 71bf964baa).
        if left < REPLY_FLOOR {
            return not_sent("sessions_timeout");
        }
        let reply = self.run(
            &[
                "reply".into(),
                "--machine".into(),
                machine_id.into(),
                agent_id.clone().into(),
                "--json".into(),
                "--".into(),
                launch.prompt.clone().into(),
            ],
            left,
        );
        seen.took(&reply);
        let ms = began.elapsed().as_millis();
        match reply {
            Ok(reply) if reply.success && replied(&reply.stdout, &agent_id) => {
                eprintln!(
                    "tasks: {}: typed into the named session{}, in {ms} ms",
                    launch.name,
                    if on.is_some() { " on a computer" } else { "" }
                );
                match on {
                    Some(on) => LaunchOutcome::JoinedOn {
                        agent_id,
                        session: joined,
                        on,
                    },
                    None => LaunchOutcome::Joined {
                        agent_id,
                        session: joined,
                    },
                }
            }
            // Taken by Harness, and an answer that can't say for this
            // session: the words may be there, so never asked again (the
            // Reviewer-lead's P3 on #5779).
            Ok(reply) if reply.success => uncertain("reply_uncertain"),
            // Refused, or its machine did not take the words: nothing typed.
            Ok(reply) => not_sent(reply_refusal(&reply.stdout)),
            Err(error) if error.code == "cancelled" => uncertain("cancelled"),
            // Started, and stopped before it answered: it may have typed.
            Err(error) if error.spawned => uncertain("reply_uncertain"),
            Err(error) => not_sent(&format!("reply_{}", error.code)),
        }
    }

    /// Devin's task, typed in as its first message: once Harness reads the
    /// harness just started idle, `harness reply` gives it the task as the
    /// person would type it (openharness#2157, behind the person's own
    /// `programs-reply` switch), within what is left of the launch's bound.
    /// The harness is started either way, so a task not typed in is
    /// uncertain, never sent again: `never_idle`, `stopped_before_message`,
    /// or the reply's refusal (`reply_off` while the person hasn't turned
    /// the switch on, `reply_not_idle`, `reply_not_running`, `reply_failed`).
    fn first_message(
        &self,
        launch: &Launch,
        outcome: LaunchOutcome,
        until: Instant,
        seen: &mut Seen,
    ) -> LaunchOutcome {
        let uncertain = |code: &str| LaunchOutcome::Uncertain { code: code.into() };
        let agent_id = match outcome {
            LaunchOutcome::Created { agent_id } => agent_id,
            // Still being made: there is no harness to type into yet.
            LaunchOutcome::Pending { .. } => return uncertain("first_message_unsent"),
            other => return other,
        };
        let on = |verb: &str| -> Vec<OsString> {
            vec![
                verb.into(),
                "--machine".into(),
                launch.target.machine_id.clone().into(),
                agent_id.clone().into(),
                "--json".into(),
            ]
        };
        loop {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return uncertain("never_idle");
            }
            let shown = self.run(&on("show"), left.min(self.config.read_timeout));
            seen.took(&shown);
            let observation = match &shown {
                Ok(reply) if reply.success => parse_observation(&reply.stdout, &agent_id).ok(),
                // Under its own code, not retried: `cancelled`,
                // `cleanup_incomplete`, `executable_changed`, a timeout.
                Err(error) => return uncertain(error.code),
                _ => None,
            };
            match observation.as_ref().map(|seen| seen.state.as_str()) {
                Some("idle") => break,
                // A Harness before the fresh-Devin fix (0.5.6) reads a Devin
                // that has done nothing yet as `needs-input`: nothing is
                // asked of it, so the reply is tried at once, and Harness
                // takes or refuses it (`reply_not_idle`), never 60 s of looks
                // (the Reviewer-lead's P2 on #4374).
                Some("needs-input") if observation.as_ref().is_some_and(fresh) => break,
                Some("stopped" | "failed" | "offline") => {
                    return uncertain("stopped_before_message");
                }
                _ => {}
            }
            let left = until.saturating_duration_since(Instant::now());
            std::thread::sleep(FIRST_MESSAGE_LOOK.min(left));
        }
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return uncertain("never_idle");
        }
        let mut args = on("reply");
        args.extend(["--".into(), launch.prompt.clone().into()]);
        let reply = self.run(&args, left);
        seen.took(&reply);
        match reply {
            Ok(reply) if reply.success && replied(&reply.stdout, &agent_id) => {
                LaunchOutcome::Created { agent_id }
            }
            Ok(reply) => uncertain(reply_refusal(&reply.stdout)),
            Err(error) => uncertain(error.code),
        }
    }

    /// The folder `new` is sent for `target` (`serve --absolute-folder`,
    /// P4 step 1). Harness's CLI resolves a `~/…` folder for another
    /// machine by asking that machine for its home (`fs_list_dir`) before
    /// `agent_create`, one more round trip through the relay; an absolute
    /// folder is sent as it is (Harness 0.5.28, `harness new`). So once a
    /// computer's home is known ([`Cli::learn_home`]), its `~/…` target is
    /// sent as that home's folder. Otherwise, and with the switch off, the
    /// target's own words, as before.
    fn folder_of(&self, target: &Target) -> SentFolder {
        let project = &target.project;
        let Some(rest) = project.strip_prefix("~/") else {
            return SentFolder {
                sent: project.clone(),
                kind: if project.starts_with('/') {
                    "absolute, as the target names it"
                } else {
                    "as a project name"
                },
                learned: false,
            };
        };
        let home_relative = SentFolder {
            sent: project.clone(),
            kind: "home-relative (~/), so Harness asks the computer for its home",
            learned: false,
        };
        if !self.config.absolute_folder {
            return home_relative;
        }
        let homes = self.homes.lock().unwrap_or_else(|e| e.into_inner());
        match homes.get(&target.machine_id) {
            Some(home) if home.len() + 1 + rest.len() <= MOST_FOLDER_BYTES => SentFolder {
                sent: format!("{home}/{rest}"),
                kind: "absolute, from the computer's home learned earlier",
                learned: true,
            },
            _ => home_relative,
        }
    }

    /// Learns `target`'s computer's home from a `new` that started in its
    /// `~/…` folder: the folder the reply says the agent works in
    /// (`agent.project.cwd`), less the target's words after `~/`. Only
    /// with `serve --absolute-folder`, only from a reply that is `ok` for
    /// that machine, and only a plain absolute home. A failed `new` sent
    /// in a learned home (`learned`) forgets it, so the next one goes as
    /// `~/…` again: a home that moved costs one hand-over, never every one.
    fn learn_home(&self, target: &Target, learned: bool, reply: &Result<Reply, RunError>) {
        if !self.config.absolute_folder {
            return;
        }
        let Some(rest) = target.project.strip_prefix("~/") else {
            return;
        };
        let mut homes = self.homes.lock().unwrap_or_else(|e| e.into_inner());
        let succeeded = matches!(reply, Ok(reply) if reply.success);
        if !succeeded {
            if learned {
                homes.remove(&target.machine_id);
            }
            return;
        }
        let Ok(reply) = reply else { return };
        if let Some(home) = home_of(&reply.stdout, &target.machine_id, rest) {
            homes.insert(target.machine_id.clone(), home);
        }
    }

    /// Whether `target`'s computer says its engine can't take this work
    /// (`serve --engines-ready`), within [`ENGINES_PROBE_TIMEOUT`]. No
    /// answer, an unknown one or an older Harness without the command is no
    /// refusal, and is kept [`ENGINES_KEPT`] for that computer and engine,
    /// so a computer that can't say costs one probe. A refusal is asked
    /// again at the next hand-over: the person may have signed in meanwhile
    /// (the Reviewer-lead's P2 on #4033). Called inside a launch, under its
    /// operation lock: a read for another task waits for it, 8 s at most.
    fn engine_refusal(&self, target: &Target, seen: &mut Seen) -> Option<&'static str> {
        // Devin Cloud isn't one of Harness's engines (`ENGINES`): `--engine
        // devin-cloud` is a usage error there, so it is never asked (Ten's
        // P3 on #4033).
        if target.engine == EngineKind::DevinCloud {
            return None;
        }
        let engine = target.engine.argument();
        let key = (target.machine_id.to_lowercase(), engine);
        let mut kept = self.ready.lock().unwrap_or_else(|e| e.into_inner());
        kept.retain(|_, at| at.elapsed() < ENGINES_KEPT);
        if kept.contains_key(&key) {
            return None;
        }
        let began = Instant::now();
        let reply = self.run(
            &[
                "machines".into(),
                "engines".into(),
                target.machine_id.clone().into(),
                "--engine".into(),
                engine.into(),
                "--json".into(),
            ],
            ENGINES_PROBE_TIMEOUT.min(self.config.read_timeout),
        );
        seen.took(&reply);
        let refused = match &reply {
            Ok(reply) if reply.success => engine_refused(&reply.stdout, &target.machine_id, engine),
            _ => None,
        };
        // One line a probe, so its cost on a lamp can be read from the
        // journal; never the computer's name.
        eprintln!(
            "tasks: asked the computer about {engine}: {}, {} ms",
            match (&reply, refused) {
                (_, Some(code)) => code,
                (Ok(reply), None) if reply.success => "no refusal",
                (Ok(_), None) => "it could not say",
                (Err(error), None) => error.code,
            },
            began.elapsed().as_millis()
        );
        if refused.is_none() {
            kept.insert(key, Instant::now());
        }
        refused
    }

    /// Every engine on `machine`, as that computer says now (`harness
    /// machines engines <machine> --json`, openharness#1888; `serve
    /// --engines-from-machines`, #5199), within [`ENGINES_LOOK_TIMEOUT`].
    /// Never takes the operation lock: it is asked on a lane of its own
    /// ([`Cli::lane`]), so a hand-over never waits behind it. Exit 1 is
    /// `unreachable` (the computer, or the probe), 2 `usage` (an older
    /// Harness); a reply that isn't one is [`parse_engines`]'s code. A read
    /// only: it installs and signs in nothing.
    pub fn engines(&self, machine: &str) -> Result<Vec<EngineRow>, String> {
        if !machine_id(machine) {
            return Err("invalid_machine".into());
        }
        let reply = self
            .run(
                &[
                    "machines".into(),
                    "engines".into(),
                    machine.into(),
                    "--json".into(),
                ],
                ENGINES_LOOK_TIMEOUT.min(self.config.launch_timeout),
            )
            .map_err(|error| error.code.to_string())?;
        if !reply.success {
            return Err(match reply.code {
                Some(1) => "unreachable",
                Some(2) => "usage",
                _ => "engines_failed",
            }
            .into());
        }
        parse_engines(&reply.stdout, machine).map_err(str::to_string)
    }

    /// The agent's last turn, read on the machine it was started on.
    pub fn observe(&self, machine: &str, agent_id: &str) -> Result<Observation, String> {
        self.observe_within(machine, agent_id, self.config.read_timeout)
    }

    /// The agent's last turn, read just before it is closed (#1390): the
    /// same `show` as [`Cli::observe`], given [`CLOSE_READ_TIMEOUT`] (no
    /// more than one `launch_timeout`) in place of `read_timeout`. Nothing
    /// waits on this read but the close itself, and a slow computer must
    /// not leave every finished harness open. A read that runs out is
    /// `timeout`, as any other, and closes nothing.
    pub fn observe_to_close(&self, machine: &str, agent_id: &str) -> Result<Observation, String> {
        self.observe_within(
            machine,
            agent_id,
            CLOSE_READ_TIMEOUT.min(self.config.launch_timeout),
        )
    }

    fn observe_within(
        &self,
        machine: &str,
        agent_id: &str,
        timeout: Duration,
    ) -> Result<Observation, String> {
        if !machine_id(machine) {
            return Err("invalid_machine".into());
        }
        if !self::agent_id(agent_id) {
            return Err("invalid_agent_id".into());
        }
        let _guard = self.operation.try_lock().map_err(|_| "busy".to_string())?;
        let reply = self
            .run(
                &[
                    "show".into(),
                    "--machine".into(),
                    machine.into(),
                    agent_id.into(),
                    "--chars".into(),
                    "4000".into(),
                    "--json".into(),
                ],
                timeout,
            )
            .map_err(|error| error.code.to_string())?;
        if !reply.success {
            return Err("observe_failed".into());
        }
        parse_observation(&reply.stdout, agent_id).map_err(str::to_string)
    }

    /// Close a harness this lamp started, once its result has been told
    /// (#1390): `harness close`, which saves its screen and stops it,
    /// keeping its conversation. Never `--now`: Harness refuses one that is
    /// working or waits for the person (`SESSION_NOT_IDLE`). As an agent,
    /// Harness also refuses one this caller did not start (exit 5); called
    /// as the person (`--harness-as-person`) it does not, and the agent id
    /// the lamp kept from its own `new` is the only guard. The caller reads
    /// the agent just before ([`Cli::observe_to_close`]), so this only closes one
    /// still idle on the turn the lamp told.
    pub fn close(&self, machine: &str, agent_id: &str) -> Result<Closed, String> {
        self.close_within(machine, agent_id, self.config.launch_timeout)
    }

    /// [`Cli::close`], given what is left of the close's budget
    /// ([`CLOSE_BUDGET`]) once its read is done, and never more than one
    /// `launch_timeout`. Nothing left (`close_no_time`) starts no process:
    /// the close failed, and is tried again, read first, like any other.
    pub fn close_within(
        &self,
        machine: &str,
        agent_id: &str,
        within: Duration,
    ) -> Result<Closed, String> {
        if !machine_id(machine) {
            return Err("invalid_machine".into());
        }
        if !self::agent_id(agent_id) {
            return Err("invalid_agent_id".into());
        }
        // Harness closes no cloud harness (openharness#1444: "stop it in
        // Harness"), so none is asked: refused, not to be asked again.
        if agent_id.starts_with("cloud_") {
            return Ok(Closed::Refused);
        }
        let within = within.min(self.config.launch_timeout);
        if within.is_zero() {
            return Err("close_no_time".into());
        }
        let _guard = self.operation.try_lock().map_err(|_| "busy".to_string())?;
        let reply = self
            .run(
                &[
                    "close".into(),
                    "--machine".into(),
                    machine.into(),
                    agent_id.into(),
                    "--json".into(),
                ],
                within,
            )
            .map_err(|error| error.code.to_string())?;
        closed(reply.code)
    }

    /// Every harness on one machine, with its state and progress line:
    /// one `ps` for all of a machine's tasks (`serve --progress`). A read
    /// only: it starts, stops and answers nothing.
    pub fn status(&self, machine: &str) -> Result<Vec<Listed>, String> {
        if !machine_id(machine) {
            return Err("invalid_machine".into());
        }
        let _guard = self.operation.try_lock().map_err(|_| "busy".to_string())?;
        let reply = self
            .run_bounded(
                &[
                    "ps".into(),
                    "--machine".into(),
                    machine.into(),
                    "--json".into(),
                ],
                self.config.read_timeout,
                PS_STDOUT_LIMIT,
            )
            .map_err(|error| error.code.to_string())?;
        if !reply.success {
            return Err("status_failed".into());
        }
        parse_ps(&reply.stdout).map_err(str::to_string)
    }

    /// Whether Harness is signed in to the person's account (`harness auth
    /// status --json`, `serve --set-up-later`). A read only: it signs
    /// nothing in or out.
    pub fn signed_in(&self) -> Result<SignIn, String> {
        let _guard = self.operation.try_lock().map_err(|_| "busy".to_string())?;
        let reply = self
            .run(
                &["auth".into(), "status".into(), "--json".into()],
                self.config.read_timeout,
            )
            .map_err(|error| error.code.to_string())?;
        if !reply.success {
            return Err("auth_status_failed".into());
        }
        parse_auth_status(&reply.stdout).map_err(str::to_string)
    }

    /// The machines on the person's account (`harness machines --json`,
    /// `serve --targets-from-machines`). A read only.
    pub fn machines(&self) -> Result<Vec<AccountMachine>, String> {
        let _guard = self.operation.try_lock().map_err(|_| "busy".to_string())?;
        let reply = self
            .run(
                &["machines".into(), "--json".into()],
                self.config.read_timeout,
            )
            .map_err(|error| error.code.to_string())?;
        if !reply.success {
            return Err("machines_failed".into());
        }
        parse_machines(&reply.stdout).map_err(str::to_string)
    }

    /// Start Harness's daemon (`harness start`; with `harness service
    /// install`, through systemd, as on a lamp): after a sign-in by code,
    /// which stops it while its user service doesn't start it again
    /// (openharness#1463). It starts nothing else and answers nothing; one
    /// already running is left as it is. It has [`DAEMON_START_TIMEOUT`],
    /// and holds the job lane that long at most: no hand-over can work
    /// until the daemon is up.
    pub fn start_daemon(&self) -> Result<(), String> {
        let _guard = self.operation.try_lock().map_err(|_| "busy".to_string())?;
        let reply = self
            .run(&["start".into()], DAEMON_START_TIMEOUT)
            .map_err(|error| error.code.to_string())?;
        if !reply.success {
            return Err("start_failed".into());
        }
        if self.config.health_after_start {
            crate::hold::watch_health(crate::hold::port(&self.config.environment));
        }
        Ok(())
    }

    /// Harness's own sign-in by a spoken code (`harness login --code
    /// --json`, Harness 0.4.23; `serve --sign-in-by-code`, #1676): the same
    /// program, checked as every run is, with the same environment and
    /// `HOME`, but its own life ([`crate::sign_in::MOST_LIFE`]), its stdin
    /// kept for the person's answer and its lines read as they come. Never
    /// takes the operation lock: a sign-in waits minutes on a person.
    pub fn sign_in_by_code(&self) -> Result<Box<dyn crate::sign_in::Link>, String> {
        self.sign_in(crate::sign_in::Method::Code)
    }

    /// Harness's own sign-in by `method` (#5198): `harness login --code
    /// --json`, the only method for now.
    pub fn sign_in(
        &self,
        method: crate::sign_in::Method,
    ) -> Result<Box<dyn crate::sign_in::Link>, String> {
        if self.poisoned.load(Ordering::Acquire) {
            return Err("cleanup_incomplete".into());
        }
        if !executable_identity(&self.config.executable)
            .is_ok_and(|identity| identity == self.executable)
        {
            return Err("executable_changed".into());
        }
        let mut command = Command::new(&self.config.executable);
        command
            .args([
                "login",
                match method {
                    crate::sign_in::Method::Code => "--code",
                },
                "--json",
            ])
            .env_clear()
            .envs(self.child_environment());
        if !self.config.as_person {
            command
                .env("AI_AGENT", "lampos")
                .env("HARNESS_AGENT_SESSION", &self.caller_session);
        }
        command
            .current_dir(&self.config.environment["HOME"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let mut child = command.spawn().map_err(|_| "spawn_failed".to_string())?;
        let pid = Pid::from_child(&child);
        let (Some(stdin), Some(stdout), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            let _ = kill_process_group(pid, Signal::KILL);
            let _ = child.kill();
            let _ = child.wait();
            return Err("pipe_failed".into());
        };
        Ok(Box::new(CodeSignIn::start(
            child, pid, stdin, stdout, stderr,
        )))
    }

    /// A second adapter on the same CLI and settings, for `wait`
    /// (`serve --push-wait`): held for minutes, it must never take the one
    /// operation lane launches and reads share.
    pub fn lane(&self) -> io::Result<Self> {
        Self::new(self.config.clone(), self.caller_session.clone())
    }

    /// Harness's own `wait --any` on `agents`, all on `machine`: held until
    /// one of them ends (two looks at one turn, every 3 s, in Harness
    /// 0.4.17) or [`WAIT_HELD`] passes, which gives none. Never takes the
    /// operation lock: launches and reads go on beside it. `cancel` ends it
    /// early, as `cancelled`. A read only: it starts, stops and answers
    /// nothing.
    pub fn wait(
        &self,
        machine: &str,
        agents: &[String],
        cancel: &AtomicBool,
    ) -> Result<Vec<Ended>, String> {
        if !machine_id(machine) {
            return Err("invalid_machine".into());
        }
        if agents.is_empty()
            || agents.len() > MOST_WAITED
            || agents.iter().any(|agent| !self::agent_id(agent))
        {
            return Err("invalid_agent_id".into());
        }
        let reply = self
            .run_cancellable(
                &[
                    "wait".into(),
                    "--machine".into(),
                    machine.into(),
                    agents.join(",").into(),
                    "--any".into(),
                    "--timeout".into(),
                    format!("{}s", WAIT_HELD.as_secs()).into(),
                    "--json".into(),
                ],
                WAIT_HELD + WAIT_SLACK,
                STDOUT_LIMIT,
                Some(cancel),
            )
            .map_err(|error| error.code.to_string())?;
        parse_wait(reply.code, &reply.stdout, agents).map_err(str::to_string)
    }

    /// Harness's own `watch` on one agent (`serve --push-watch`, #5230),
    /// held until it ends or [`WATCH_HELD`] passes. Each event is handed to
    /// `on_event` as it comes; on `done` or `failed` the watch is killed
    /// here, since `watch` only exits by itself on `stopped`. Never takes
    /// the operation lock: launches and reads go on beside it. `cancel`
    /// ends it early. A read only: it starts, stops and answers nothing.
    /// Every child is killed and reaped before this returns.
    pub fn watch(
        &self,
        machine: &str,
        agent: &str,
        cancel: &AtomicBool,
        on_event: &mut dyn FnMut(WatchEvent),
    ) -> WatchEnd {
        if !machine_id(machine) {
            return WatchEnd::Failed("invalid_machine".into());
        }
        if !self::agent_id(agent) {
            return WatchEnd::Failed("invalid_agent_id".into());
        }
        let mut error: Option<String> = None;
        let mut sink = |line: &[u8]| match parse_watch_line(line, agent) {
            Ok(WatchLine::Event(event)) => {
                on_event(event);
                matches!(event, WatchEvent::Done | WatchEvent::Failed)
            }
            Ok(WatchLine::Error(code)) => {
                error = Some(code);
                false
            }
            // A line this does not read is passed over; the next is read.
            Err(_) => false,
        };
        let result = self.run_once_said(
            &[
                "watch".into(),
                "--machine".into(),
                machine.into(),
                agent.into(),
                // The summary is never read (the task's own `show` takes the
                // answer): kept short, so a line stays small.
                "--chars".into(),
                WATCH_CHARS.to_string().into(),
                "--timeout".into(),
                format!("{}m", WATCH_HELD.as_secs() / 60).into(),
                "--json".into(),
            ],
            WATCH_HELD + WATCH_SLACK,
            WATCH_LINE_LIMIT,
            Some(cancel),
            &mut sink,
        );
        match result {
            Err(failure) if failure.code == "ended" => WatchEnd::Settled,
            Err(failure) if failure.code == "cancelled" => WatchEnd::Cancelled,
            Err(failure) => WatchEnd::Failed(failure.code.into()),
            Ok(reply) => match reply.code {
                Some(4) => WatchEnd::Stopped,
                Some(124) => WatchEnd::Timeout,
                Some(2) => WatchEnd::Missing,
                _ => WatchEnd::Failed(error.unwrap_or_else(|| "watch_failed".into())),
            },
        }
    }

    /// [`Cli::run_cancellable`] with a line sink: one call line, whatever
    /// the outcome.
    fn run_once_said(
        &self,
        args: &[OsString],
        timeout: Duration,
        line_limit: usize,
        also: Option<&AtomicBool>,
        sink: &mut LineSink<'_>,
    ) -> Result<Reply, RunError> {
        let began = Instant::now();
        let result = self.run_once(args, timeout, line_limit, also, Some(sink));
        let line = call_line(args, &result, began.elapsed());
        eprintln!("{line}");
        if let Some(calls) = &self.config.calls {
            calls.lock().unwrap_or_else(|e| e.into_inner()).push(line);
        }
        result
    }

    fn run(&self, args: &[OsString], timeout: Duration) -> Result<Reply, RunError> {
        self.run_bounded(args, timeout, STDOUT_LIMIT)
    }

    fn run_bounded(
        &self,
        args: &[OsString],
        timeout: Duration,
        stdout_limit: usize,
    ) -> Result<Reply, RunError> {
        self.run_cancellable(args, timeout, stdout_limit, None)
    }

    /// Every Harness call goes through here, and says how it went in one
    /// journal line ([`call_line`]), whatever the outcome.
    fn run_cancellable(
        &self,
        args: &[OsString],
        timeout: Duration,
        stdout_limit: usize,
        also: Option<&AtomicBool>,
    ) -> Result<Reply, RunError> {
        let began = Instant::now();
        let result = self.run_once(args, timeout, stdout_limit, also, None);
        let line = call_line(args, &result, began.elapsed());
        eprintln!("{line}");
        if let Some(calls) = &self.config.calls {
            calls.lock().unwrap_or_else(|e| e.into_inner()).push(line);
        }
        result
    }

    fn run_once(
        &self,
        args: &[OsString],
        timeout: Duration,
        stdout_limit: usize,
        also: Option<&AtomicBool>,
        sink: Option<&mut LineSink<'_>>,
    ) -> Result<Reply, RunError> {
        let error = |code, spawned| RunError { code, spawned };
        if self.poisoned.load(Ordering::Acquire) {
            return Err(error("cleanup_incomplete", false));
        }
        if self.cancelled(also) {
            return Err(error("cancelled", false));
        }
        if !executable_identity(&self.config.executable)
            .is_ok_and(|identity| identity == self.executable)
        {
            return Err(error("executable_changed", false));
        }
        let deadline = Instant::now() + timeout;
        let mut command = Command::new(&self.config.executable);
        command
            .args(args)
            .env_clear()
            .envs(self.child_environment());
        if !self.config.as_person {
            command
                .env("AI_AGENT", "lampos")
                .env("HARNESS_AGENT_SESSION", &self.caller_session);
        }
        command
            .current_dir(&self.config.environment["HOME"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        if self.cancelled(also) {
            return Err(error("cancelled", false));
        }
        let child = command.spawn().map_err(|_| error("spawn_failed", false))?;
        let mut owned = OwnedChild::new(child);
        let result = self.capture(&mut owned, deadline, stdout_limit, also, sink);
        if !owned.finish() {
            self.poisoned.store(true, Ordering::Release);
            return Err(error("cleanup_incomplete", true));
        }
        result.map_err(|code| error(code, true))
    }

    fn capture(
        &self,
        owned: &mut OwnedChild,
        deadline: Instant,
        stdout_limit: usize,
        also: Option<&AtomicBool>,
        mut sink: Option<&mut LineSink<'_>>,
    ) -> Result<Reply, &'static str> {
        let mut stdout = owned.child.stdout.take().ok_or("pipe_failed")?;
        let mut stderr = owned.child.stderr.take().ok_or("pipe_failed")?;
        nonblocking(&stdout).map_err(|_| "pipe_failed")?;
        nonblocking(&stderr).map_err(|_| "pipe_failed")?;
        let mut output = Vec::new();
        let mut errors = Vec::new();
        let mut stdout_done = false;
        let mut stderr_done = false;
        let mut exit = None;
        // With a sink (`watch`), each whole line is handed on as it comes
        // and dropped, so `stdout_limit` bounds one line, not the run; the
        // last line is kept for the call line. The sink saying `true` ends
        // the run (`ended`), and the child is killed and reaped as any.
        let mut last_line: Vec<u8> = Vec::new();
        let result = (|| loop {
            let now = Instant::now();
            if self.cancelled(also) {
                return Err("cancelled");
            }
            if now >= deadline {
                return Err("timeout");
            }
            if !stdout_done {
                stdout_done = drain(&mut stdout, &mut output, stdout_limit, "stdout_limit")?;
                if let Some(sink) = sink.as_mut() {
                    while let Some(end) = output.iter().position(|byte| *byte == b'\n') {
                        last_line = output.drain(..=end).collect();
                        last_line.pop();
                        if sink(&last_line) {
                            return Err("ended");
                        }
                    }
                    if stdout_done && !output.is_empty() {
                        last_line = std::mem::take(&mut output);
                        if sink(&last_line) {
                            return Err("ended");
                        }
                    }
                }
            }
            if !stderr_done {
                stderr_done = drain(&mut stderr, &mut errors, STDERR_LIMIT, "stderr_limit")?;
            }
            if exit.is_none() {
                match waitid(
                    WaitId::Pid(owned.pid),
                    WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
                ) {
                    Ok(Some(status)) => exit = Some((now, status.exit_status())),
                    Ok(None) => {}
                    Err(error) if error == rustix::io::Errno::INTR => {}
                    Err(_) => return Err("wait_failed"),
                }
            }
            if let Some((at, code)) = exit {
                if stdout_done && stderr_done {
                    owned.exited_with_closed_pipes = true;
                    let stdout = if sink.is_some() {
                        std::mem::take(&mut last_line)
                    } else {
                        std::mem::take(&mut output)
                    };
                    return Ok(Reply {
                        success: code == Some(0),
                        code,
                        stdout,
                        stderr: errors,
                    });
                }
                if now.saturating_duration_since(at) >= DRAIN_LIMIT {
                    return Err("pipes_open");
                }
            }
            let mut descriptors = Vec::with_capacity(2);
            if !stdout_done {
                descriptors.push(PollFd::new(&stdout, PollFlags::IN));
            }
            if !stderr_done {
                descriptors.push(PollFd::new(&stderr, PollFlags::IN));
            }
            let left = deadline
                .saturating_duration_since(Instant::now())
                .min(POLL_INTERVAL);
            let wait = Timespec::try_from(left).map_err(|_| "poll_failed")?;
            match poll(&mut descriptors, Some(&wait)) {
                Ok(_) => {}
                Err(error) if error == rustix::io::Errno::INTR => {}
                Err(_) => return Err("poll_failed"),
            }
        })();
        if result.is_err() {
            owned.tail = Some(PipeTail {
                stdout,
                stderr,
                stdout_done,
                stderr_done,
            });
        }
        result
    }
}

fn nonblocking(fd: &impl AsFd) -> io::Result<()> {
    fcntl_setfl(fd, fcntl_getfl(fd)? | OFlags::NONBLOCK).map_err(Into::into)
}

fn drain(
    reader: &mut impl Read,
    bytes: &mut Vec<u8>,
    limit: usize,
    overflow: &'static str,
) -> Result<bool, &'static str> {
    // Bound work per pipe per iteration as well as total bytes. A chatty child
    // cannot starve cancellation or the other pipe.
    let mut chunk = [0; 4096];
    match reader.read(&mut chunk) {
        Ok(0) => Ok(true),
        Ok(count) => {
            if bytes.len() + count > limit {
                return Err(overflow);
            }
            bytes.extend_from_slice(&chunk[..count]);
            Ok(false)
        }
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
            ) =>
        {
            Ok(false)
        }
        Err(_) => Err("pipe_read_failed"),
    }
}

struct Reply {
    success: bool,
    code: Option<i32>,
    stdout: Vec<u8>,
    /// At most [`STDERR_LIMIT`]; only its last line is ever said
    /// ([`dispatch_line`]).
    stderr: Vec<u8>,
}
struct RunError {
    code: &'static str,
    spawned: bool,
}

struct PipeTail {
    stdout: ChildStdout,
    stderr: ChildStderr,
    stdout_done: bool,
    stderr_done: bool,
}
impl PipeTail {
    fn drain_once(&mut self) -> bool {
        // Cleanup discards at most one chunk per pipe per iteration. The
        // absolute cleanup deadline also bounds a descendant that keeps writing.
        let mut chunk = [0; 4096];
        if !self.stdout_done {
            self.stdout_done = matches!(self.stdout.read(&mut chunk), Ok(0));
        }
        if !self.stderr_done {
            self.stderr_done = matches!(self.stderr.read(&mut chunk), Ok(0));
        }
        self.stdout_done && self.stderr_done
    }
}

struct OwnedChild {
    child: Child,
    pid: Pid,
    cleaned: bool,
    exited_with_closed_pipes: bool,
    tail: Option<PipeTail>,
}
impl OwnedChild {
    fn new(child: Child) -> Self {
        let pid = Pid::from_child(&child);
        Self {
            child,
            pid,
            cleaned: false,
            exited_with_closed_pipes: false,
            tail: None,
        }
    }
    fn finish(&mut self) -> bool {
        if self.cleaned {
            return true;
        }
        // NOWAIT above reserves the child PID until this group signal: a
        // reaped PID must not be reused as an unrelated process-group target.
        let group = kill_process_group(self.pid, Signal::KILL);
        let _ = self.child.kill();
        let until = Instant::now() + REAP_LIMIT;
        let mut reaped = false;
        let mut pipes_closed = self.exited_with_closed_pipes;
        loop {
            if !reaped {
                match self.child.try_wait() {
                    Ok(Some(_)) => reaped = true,
                    Ok(None) => {}
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
            if let Some(tail) = &mut self.tail {
                pipes_closed = tail.drain_once();
            }
            if reaped && pipes_closed {
                break;
            }
            if Instant::now() >= until {
                break;
            }
            thread::sleep(Duration::from_millis(1));
        }
        self.exited_with_closed_pipes = reaped && pipes_closed;
        self.cleaned = true;
        // Darwin's killpg1 skips zombies and returns EPERM when a group has
        // no live members. Accept that case only after observing exit and
        // both pipe EOFs; a live or pipe-holding child still fails closed.
        let group_stopped = matches!(group, Ok(()) | Err(rustix::io::Errno::SRCH))
            || (cfg!(target_os = "macos")
                && self.exited_with_closed_pipes
                && group == Err(rustix::io::Errno::PERM));
        reaped && pipes_closed && group_stopped
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if !self.cleaned {
            let _ = self.finish();
        }
    }
}

/// A running `harness login --code --json` ([`Cli::sign_in_by_code`]): its
/// lines read on a thread of their own, its answer written to its stdin,
/// and its process group stopped when dropped.
struct CodeSignIn {
    child: Child,
    pid: Pid,
    stdin: std::process::ChildStdin,
    read: std::sync::mpsc::Receiver<Result<crate::sign_in::Event, &'static str>>,
    ended: bool,
}

/// What Harness writes when it doesn't know `--code`: a build before 0.4.23
/// ("error: unknown option '--code'").
fn too_old(errors: &str) -> bool {
    let errors = errors.to_lowercase();
    errors.contains("unknown option") || errors.contains("unknown argument")
}

impl CodeSignIn {
    fn start(
        child: Child,
        pid: Pid,
        stdin: std::process::ChildStdin,
        stdout: ChildStdout,
        stderr: ChildStderr,
    ) -> Self {
        let (send, read) = std::sync::mpsc::sync_channel(8);
        // What it says on stderr, bounded, for why it ended with no result.
        let errors = thread::spawn(move || {
            let mut kept = Vec::new();
            let _ = stderr.take(STDERR_LIMIT as u64).read_to_end(&mut kept);
            String::from_utf8_lossy(&kept).into_owned()
        });
        thread::spawn(move || {
            use std::io::BufRead;
            let mut lines = io::BufReader::new(stdout);
            let mut line = Vec::new();
            let ended = loop {
                line.clear();
                match (&mut lines)
                    .take(crate::sign_in::MOST_LINE_BYTES as u64 + 1)
                    .read_until(b'\n', &mut line)
                {
                    Ok(0) | Err(_) => break None,
                    Ok(_) => {}
                }
                let text = std::str::from_utf8(&line).map(str::trim);
                let read = match text {
                    Ok("") => continue,
                    Ok(text) if line.len() <= crate::sign_in::MOST_LINE_BYTES => {
                        crate::sign_in::parse_line(text)
                    }
                    _ => Err("unreadable"),
                };
                match read {
                    Ok(None) => {}
                    Ok(Some(event)) => {
                        if send.send(Ok(event)).is_err() {
                            return;
                        }
                    }
                    Err(code) => break Some(code),
                }
            };
            let code = ended.unwrap_or_else(|| {
                if errors.join().is_ok_and(|errors| too_old(&errors)) {
                    "harness_too_old"
                } else {
                    "ended"
                }
            });
            let _ = send.send(Err(code));
        });
        Self {
            child,
            pid,
            stdin,
            read,
            ended: false,
        }
    }
}

impl crate::sign_in::Link for CodeSignIn {
    fn poll(&mut self) -> Option<Result<crate::sign_in::Event, &'static str>> {
        if self.ended {
            return None;
        }
        match self.read.try_recv() {
            Ok(read) => {
                self.ended = read.is_err();
                Some(read)
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.ended = true;
                Some(Err("ended"))
            }
        }
    }

    fn answer(&mut self, yes: bool) -> io::Result<()> {
        use std::io::Write;
        self.stdin.write_all(if yes { b"yes\n" } else { b"no\n" })?;
        self.stdin.flush()
    }
}

impl Drop for CodeSignIn {
    fn drop(&mut self) {
        // Its whole group, as every run here is stopped; then reaped.
        let _ = kill_process_group(self.pid, Signal::KILL);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Deserialize)]
struct Machine {
    #[serde(rename = "machineId")]
    machine_id: String,
    current: bool,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    hostname: Option<String>,
    /// From relay presence (openharness#1888 era): `running`, `offline`,
    /// `unknown` or `payment_pending`. Anything else, or none, is unknown.
    #[serde(default)]
    status: Option<Value>,
}

/// The presence words `harness machines --json` gives a machine, as kept
/// ([`AccountMachine::status`]): any other is unknown.
pub const MACHINE_STATUSES: [&str; 4] = ["running", "offline", "unknown", "payment_pending"];

/// One machine on the person's account, as `harness machines --json` lists
/// it (openharness `cli.ts` `machinesListCommand`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountMachine {
    pub machine_id: String,
    /// Its own name, else the hostname of the computer that last connected
    /// it, as Harness names it; empty with neither.
    pub label: String,
    /// The machine this CLI runs as: the lamp itself.
    pub current: bool,
    /// Its presence, one of [`MACHINE_STATUSES`], as Harness's relay saw
    /// it; `None` when Harness gave none or another word. Read only with
    /// `serve --engines-from-machines` (#5199).
    pub status: Option<String>,
}

/// What `harness machines --json` says: one object a line, newest first,
/// each with a full machine id, unique, and at most one `current`. Other
/// fields are passed over.
pub fn parse_machines(bytes: &[u8]) -> Result<Vec<AccountMachine>, &'static str> {
    let invalid = "machines_invalid_reply";
    let text = std::str::from_utf8(bytes).map_err(|_| invalid)?;
    let mut ids = BTreeSet::new();
    let mut listed = Vec::new();
    let mut current = 0;
    for line in text.lines() {
        if line.trim().is_empty() || ids.len() >= 256 {
            return Err(invalid);
        }
        let machine: Machine = serde_json::from_str(line).map_err(|_| invalid)?;
        if !machine_id(&machine.machine_id) || !ids.insert(machine.machine_id.clone()) {
            return Err(invalid);
        }
        current += usize::from(machine.current);
        if current > 1 {
            return Err(invalid);
        }
        let label = [machine.name, machine.hostname]
            .into_iter()
            .flatten()
            .map(|value| value.trim().to_owned())
            .find(|value| !value.is_empty())
            .unwrap_or_default();
        let status = machine
            .status
            .as_ref()
            .and_then(Value::as_str)
            .filter(|word| MACHINE_STATUSES.contains(word))
            .map(str::to_owned);
        listed.push(AccountMachine {
            machine_id: machine.machine_id,
            label,
            current: machine.current,
            status,
        });
    }
    Ok(listed)
}

fn machines(bytes: &[u8], target: &str) -> Result<(), &'static str> {
    let listed = parse_machines(bytes).map_err(|_| "preflight_invalid_reply")?;
    if listed.iter().any(|machine| machine.machine_id == target) {
        Ok(())
    } else {
        Err("machine_unavailable")
    }
}

/// The folder `new` is sent ([`Cli::folder_of`]): the words, how they
/// were chosen for the dispatch's timing line, and whether they come from
/// a learned home.
struct SentFolder {
    sent: String,
    kind: &'static str,
    learned: bool,
}

/// The longest folder a target names ([`Target::validate`]).
const MOST_FOLDER_BYTES: usize = 512;

/// The computer's home from a successful `new --json` reply for `machine`
/// whose agent works in the `~/REST` folder: `agent.project.cwd` with
/// `/REST` taken off its end. Only a plain absolute home: `/` first, no
/// empty, `.` or `..` part, no control character, no `~`, and the whole
/// within [`MOST_FOLDER_BYTES`]. Anything else, or no such field, is none.
fn home_of(bytes: &[u8], machine: &str, rest: &str) -> Option<String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Started {
        ok: bool,
        machine_id: Option<String>,
        agent: Option<Agent>,
    }
    #[derive(Deserialize)]
    struct Agent {
        project: Option<Project>,
    }
    #[derive(Deserialize)]
    struct Project {
        cwd: Option<String>,
    }
    let reply: Started = serde_json::from_slice(bytes).ok()?;
    if !reply.ok || reply.machine_id.as_deref() != Some(machine) || rest.is_empty() {
        return None;
    }
    let cwd = reply.agent?.project?.cwd?;
    let home = cwd.strip_suffix(rest)?.strip_suffix('/')?;
    let plain = home.len() > 1
        && cwd.len() <= MOST_FOLDER_BYTES
        && home.strip_prefix('/').is_some_and(|parts| {
            parts
                .split('/')
                .all(|part| !matches!(part, "" | "." | ".."))
        })
        && !home.chars().any(|c| c.is_control() || c == '~');
    plain.then(|| home.to_owned())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NewReply {
    ok: bool,
    machine_id: Option<String>,
    pending: Option<bool>,
    existing: Option<bool>,
    agent: Option<CreatedAgent>,
    creation_id: Option<String>,
    name: Option<String>,
    error: Option<Value>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreatedAgent {
    id: String,
    name: String,
    engine: EngineKind,
    /// Absent, or null, for an agent with no modes (Devin, #1854).
    #[serde(default)]
    permission_mode: Option<String>,
    /// Required for an agent with modes, as before #1854: `None` only when
    /// the field is missing, an explicit `null` being `Some(Null)`.
    #[serde(default, deserialize_with = "present")]
    bypass_permission: Option<Value>,
}

/// A field that is there, `null` included, as `Some`: only a missing one is
/// `None` (by `default`).
fn present<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}

/// What `harness machines engines --json` says of `engine` on `machine`
/// (openharness#1888): `engine_no_prompt` for a known `firstPrompt: false`,
/// `engine_signed_out` for a known `signedIn: false` (read for Claude Code
/// and Codex only), and nothing for anything else: `null`, no row of that
/// computer and engine, or a line that isn't one. An older Harness's
/// `machines` list has no `machineId` row of an engine, so it refuses none.
fn engine_refused(bytes: &[u8], machine: &str, engine: &str) -> Option<&'static str> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Row {
        machine_id: String,
        engine: String,
        signed_in: Option<bool>,
        first_prompt: Option<bool>,
    }
    let row = String::from_utf8_lossy(bytes)
        .lines()
        .filter_map(|line| serde_json::from_str::<Row>(line).ok())
        .find(|row| row.machine_id.eq_ignore_ascii_case(machine) && row.engine == engine)?;
    match (row.first_prompt, row.signed_in) {
        (Some(false), _) => Some("engine_no_prompt"),
        (_, Some(false)) => Some("engine_signed_out"),
        _ => None,
    }
}

/// What one computer says of one engine (`harness machines engines --json`,
/// openharness#1888; `serve --engines-from-machines`, #5199). `None` is
/// unknown, never missing: a login shell that timed out reads `null` too.
/// Harness reads `signedIn` for Claude Code and Codex only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineRow {
    pub engine: String,
    /// Harness's CLI gives `true` or `null`: the computer's `false` comes
    /// out as `null` (openharness `machineEngines.ts`).
    pub installed: Option<bool>,
    /// The computer's own `!installed && command && recipe`, passed
    /// through: with `installed` null, `true` is an engine not found there.
    pub installable: Option<bool>,
    pub signed_in: Option<bool>,
}

/// The most rows one look at a computer's engines may give.
pub const MOST_ENGINE_ROWS: usize = 64;

/// What `harness machines engines <machine> --json` says of `machine`: one
/// object a line, `{machineId, engine, installed, signedIn, …}`, each
/// readiness field `true`, `false` or `null`. Rows of another machine are
/// passed over (the id compared without case, as Harness may print it);
/// other fields are too. A line that isn't such a row, an engine twice, more
/// than [`MOST_ENGINE_ROWS`] rows, or no row of this machine at all is no
/// answer: an older Harness's machine list included.
pub fn parse_engines(bytes: &[u8], machine: &str) -> Result<Vec<EngineRow>, &'static str> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Row {
        machine_id: String,
        engine: String,
        #[serde(default)]
        installed: Option<bool>,
        #[serde(default)]
        installable: Option<bool>,
        #[serde(default)]
        signed_in: Option<bool>,
    }
    let invalid = "engines_invalid_reply";
    let text = std::str::from_utf8(bytes).map_err(|_| invalid)?;
    let mut rows: Vec<EngineRow> = Vec::new();
    let mut count = 0usize;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        count += 1;
        if count > MOST_ENGINE_ROWS {
            return Err(invalid);
        }
        let row: Row = serde_json::from_str(line).map_err(|_| invalid)?;
        if !row.machine_id.eq_ignore_ascii_case(machine) {
            continue;
        }
        let plain = !row.engine.is_empty()
            && row.engine.len() <= 32
            && row
                .engine
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !plain || rows.iter().any(|kept| kept.engine == row.engine) {
            return Err(invalid);
        }
        rows.push(EngineRow {
            engine: row.engine,
            installed: row.installed,
            installable: row.installable,
            signed_in: row.signed_in,
        });
    }
    if rows.is_empty() {
        return Err("engines_none");
    }
    Ok(rows)
}

/// Whether a failed `new` says the target's folder isn't on the computer
/// (#2840). Harness refuses that before it makes anything (`CWD_NOT_FOUND`,
/// openharness `core/agents/create.ts`; `INVALID_CWD` in its own words), so
/// the task surely did not start. It never makes the folder (lamp-75c9, 22
/// Oct). Any other failure may have started an agent.
fn folder_missing(bytes: &[u8]) -> bool {
    #[derive(Deserialize)]
    struct Failed {
        ok: bool,
        error: String,
    }
    serde_json::from_slice::<Failed>(bytes).is_ok_and(|reply| {
        !reply.ok
            && (reply.error == "CWD_NOT_FOUND"
                || reply.error.starts_with("CWD_NOT_FOUND (")
                || reply.error == "That folder does not exist on the machine.")
    })
}

/// A dispatch's last CLI call, for its journal line: its exit status and
/// stderr, or neither when it never exited (a timeout, a pipe cap).
#[derive(Default)]
struct Seen {
    exit: Option<i32>,
    stderr: Vec<u8>,
    /// Every word of the machines the preflight listed (names, hostnames,
    /// ids), lowercased: never said ([`said_of_stderr`]).
    machines: BTreeSet<String>,
}
impl Seen {
    fn took(&mut self, result: &Result<Reply, RunError>) {
        (self.exit, self.stderr) = match result {
            Ok(reply) => (reply.code, reply.stderr.clone()),
            Err(_) => (None, Vec::new()),
        };
    }
}

/// Every word naming a machine in a `machines --json` reply, lowercased:
/// each machine's name and hostname whole, word by word and without a
/// domain (`lans-mbp.local` as `lans-mbp` too), and its ids; each part of
/// two letters or more split at spaces, `-`, `_`, `.` and apostrophes, and
/// the same without a trailing `s` (`lan` of "Lan’s MacBook Air" and of
/// `lans-macbook-air`); and every run of two or more dashed parts
/// (`lan-macbook`, `macbook-air`). [`NOT_A_NAME`] stays, so diagnostics keep
/// it. A line that isn't JSON gives nothing; the preflight's own check
/// refuses it.
fn machine_words(bytes: &[u8]) -> BTreeSet<String> {
    let mut words = BTreeSet::new();
    for line in String::from_utf8_lossy(bytes).lines() {
        let Ok(machine) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        for key in ["name", "hostname", "machineId", "computerId"] {
            let Some(value) = machine[key].as_str() else {
                continue;
            };
            let value = value.trim().to_lowercase();
            if let Some((host, _)) = value.split_once('.') {
                words.insert(host.to_owned());
            }
            words.extend(value.split_whitespace().map(str::to_owned));
            // Each part: a machine's owner ("Lan’s MacBook Air",
            // `lans-macbook-air`) is said alone as often as whole (the
            // Reviewer-lead's P2 on #3727).
            for part in value
                .split(|c: char| c.is_whitespace() || "-_.'’".contains(c))
                .filter(|part| part.chars().count() >= 2)
            {
                words.insert(part.to_owned());
                if let Some(owner) = part.strip_suffix('s')
                    && owner.chars().count() >= 2
                {
                    words.insert(owner.to_owned());
                }
            }
            // Every run of two or more dashed parts: `lan-macbook`,
            // `macbook-air` and `lan-mbp` of `dev-lan-mbp`.
            let dashed: Vec<&str> = value
                .split(['-', '.'])
                .filter(|part| !part.is_empty())
                .collect();
            for start in 0..dashed.len() {
                for end in start + 2..=dashed.len() {
                    words.insert(dashed[start..end].join("-"));
                }
            }
            words.insert(value);
        }
    }
    words.remove("");
    for kept in NOT_A_NAME {
        words.remove(kept);
    }
    words
}

/// Words a machine's name or hostname may hold that name nobody: `local`
/// of `.local`, which diagnostics need ("could not reach local daemon").
const NOT_A_NAME: [&str; 1] = ["local"];

/// The most of stderr's last line a journal line keeps.
const STDERR_SAID_CHARS: usize = 160;

/// One dispatch's journal line, whatever its outcome (the first builder on
/// #3126, 23 Oct 11:42: an `uncertain` dispatch left lamp-0c89's journal
/// empty). The task's label, its state and code, the CLI's last exit
/// status, how long it took, and stderr's last line with only what can't be
/// the person's kept ([`said_of_stderr`]). Never the ask.
fn dispatch_line(launch: &Launch, outcome: &LaunchOutcome, seen: &Seen, took: Duration) -> String {
    let state = match outcome {
        LaunchOutcome::Created { .. } => "accepted".to_owned(),
        LaunchOutcome::Joined { .. } => "joined".to_owned(),
        LaunchOutcome::JoinedOn { .. } => "joined on another machine".to_owned(),
        LaunchOutcome::Pending { .. } => "pending".to_owned(),
        LaunchOutcome::NotSent { code } => format!("not_sent {code}"),
        LaunchOutcome::Uncertain { code } => format!("uncertain {code}"),
    };
    let exit = seen
        .exit
        .map_or_else(|| "no exit".to_owned(), |code| format!("exit {code}"));
    let mut line = format!(
        "tasks: {}: dispatch {state} ({exit}, {:.1} s",
        launch.name,
        took.as_secs_f64()
    );
    if let Some(said) = said_of_stderr(&seen.stderr, &launch.prompt, &seen.machines) {
        line.push_str(&format!(", stderr: {said}"));
    }
    line.push(')');
    line
}

/// Stderr's last line that has words, with only what can't be the person's
/// kept (Fourteen's privacy P2 on #3234): quoted spans go whole, and a word
/// stays only if it is plain lowercase ASCII (`error:`, `could`, `start`)
/// or an upper-case code (`SPAWN_FAILED`), is none of the ask's words at any
/// length, has no `/` or `@`, and doesn't look secret. Anything else, a
/// name, a computer's label, a folder, a word with an accent, is "…", one
/// for each run. At most [`STDERR_SAID_CHARS`]; nothing when no word is left.
fn said_of_stderr(stderr: &[u8], prompt: &str, machines: &BTreeSet<String>) -> Option<String> {
    let text = String::from_utf8_lossy(stderr);
    let last = text.lines().rev().find(|line| !line.trim().is_empty())?;
    let bare = |word: &str| {
        word.trim_matches(|c: char| !c.is_alphanumeric())
            .to_lowercase()
    };
    let asked: BTreeSet<String> = prompt
        .split_whitespace()
        .map(bare)
        .filter(|word| !word.is_empty())
        .collect();
    // Each quoted span as one word, so it goes whole.
    let mut unquoted = String::new();
    let mut closing: Option<char> = None;
    for c in last.chars() {
        match closing {
            Some(end) if c == end => {
                closing = None;
                unquoted.push_str(" \u{1} ");
            }
            Some(_) => {}
            None => match c {
                '\'' | '"' | '`' => closing = Some(c),
                '“' => closing = Some('”'),
                '‘' => closing = Some('’'),
                _ => unquoted.push(if c.is_control() { ' ' } else { c }),
            },
        }
    }
    if closing.is_some() {
        unquoted.push_str(" \u{1} ");
    }
    let secret = |word: &str| {
        [
            "token",
            "secret",
            "password",
            "passwd",
            "apikey",
            "api_key",
            "api-key",
            "bearer",
            "authorization",
            "cookie",
            "credential",
            "session",
        ]
        .iter()
        .any(|name| word.to_lowercase().contains(name))
    };
    let kept = |word: &str| {
        let core = word.trim_end_matches([':', ',', '.', ';', '!', '?', ')']);
        let plain = !core.is_empty()
            && core
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "-_()".contains(c));
        let code = core.len() >= 3
            && core
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
        // A long run with digits in it: a machine's or a computer's id.
        let id = core.len() >= 16 && core.chars().any(|c| c.is_ascii_digit());
        let lower = core.to_lowercase();
        (plain || code)
            && !id
            && !asked.contains(&bare(word))
            && !machines.contains(&lower)
            && !machines.contains(&bare(word))
            && !secret(word)
    };
    let mut words: Vec<&str> = Vec::new();
    for word in unquoted.split_whitespace() {
        let word = if kept(word) { word } else { "…" };
        if !(word == "…" && words.last() == Some(&"…")) {
            words.push(word);
        }
    }
    if words.iter().all(|word| *word == "…") {
        return None;
    }
    Some(words.join(" ").chars().take(STDERR_SAID_CHARS).collect())
}

/// The project name whose folder Harness makes as `~/harnesses/<name>`,
/// when `project` is that folder (`serve --new-folder`). Only a name Harness
/// keeps as it is (openharness `projectName`: letters, digits, `.`, `_` and
/// `-`, no `.` or `-` at either end, at most 64), so `--new <name>` makes
/// exactly this folder; any other folder is never asked for so.
fn made_name(project: &str) -> Option<&str> {
    let name = project.strip_prefix("~/harnesses/")?;
    let kept = !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        && !name.starts_with(['.', '-'])
        && !name.ends_with(['.', '-']);
    kept.then_some(name)
}

/// Whether the reply is Harness's refusal of `--new` because that project's
/// folder is there already (`PROJECT_EXISTS`): exit 1 and
/// `{"ok":false,"error":"“NAME” already exists on COMPUTER: FOLDER. Choose
/// another name …"}`, a sentence, not a code (openharness `newCommand.ts`;
/// Harness 0.5.28). Read tolerantly, by " already exists on " alone; any
/// other refusal is a failure like any other.
fn project_exists(reply: &Reply) -> bool {
    #[derive(Deserialize)]
    struct Failed {
        ok: bool,
        error: String,
    }
    reply.code == Some(1)
        && serde_json::from_slice::<Failed>(&reply.stdout)
            .is_ok_and(|failed| !failed.ok && failed.error.contains(" already exists on "))
}

/// The most folders kept as seen: far more than a targets file names.
const MOST_FOLDERS_SEEN: usize = 256;
/// The kept file is at most this long; a longer one is read as empty.
const FOLDERS_FILE_BYTES: u64 = 128 * 1024;

/// The `~/harnesses/<name>` folders this lamp has seen on each computer
/// (`serve --new-folder`): made by Harness on a `--new`, or found there
/// already. Kept in a file of the tasks folder, so a restart doesn't ask
/// `--new` again. Only a shortcut: a folder missing from it costs one
/// `--new` that Harness refuses as there already, and the hand-over then
/// goes to the folder, as before.
struct NewFolder {
    file: Option<PathBuf>,
    seen: BTreeSet<(String, String)>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FoldersFile {
    version: u32,
    seen: Vec<SeenFolder>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SeenFolder {
    machine_id: String,
    project: String,
}

impl NewFolder {
    /// What `file` keeps. One missing is none seen; one that can't be read,
    /// or isn't this shape, is none seen too, with a line.
    fn load(file: Option<&Path>) -> Self {
        let mut folders = Self {
            file: file.map(Path::to_path_buf),
            seen: BTreeSet::new(),
        };
        let Some(file) = file else {
            return folders;
        };
        match Self::read(file) {
            Ok(seen) => folders.seen = seen,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                eprintln!(
                    "tasks: the folders seen could not be read ({error}); none are taken as seen"
                );
            }
        }
        folders
    }

    fn read(file: &Path) -> io::Result<BTreeSet<(String, String)>> {
        let metadata = fs::symlink_metadata(file)?;
        if !metadata.is_file() || metadata.len() > FOLDERS_FILE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "not a small file",
            ));
        }
        let kept: FoldersFile = serde_json::from_slice(&fs::read(file)?)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "not the folders file"))?;
        if kept.version != 1 || kept.seen.len() > MOST_FOLDERS_SEEN {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "not version 1"));
        }
        Ok(kept
            .seen
            .into_iter()
            .map(|seen| (seen.machine_id, seen.project))
            .collect())
    }

    fn contains(&self, target: &Target) -> bool {
        self.seen
            .contains(&(target.machine_id.clone(), target.project.clone()))
    }

    /// Mark `target`'s folder seen, and keep the file: written whole beside
    /// it, then renamed over it. A failed write is said and leaves the mark
    /// in memory.
    fn insert(&mut self, target: &Target) {
        if self.contains(target) {
            return;
        }
        if self.seen.len() >= MOST_FOLDERS_SEEN {
            let first = self.seen.iter().next().cloned();
            if let Some(first) = first {
                self.seen.remove(&first);
            }
        }
        self.seen
            .insert((target.machine_id.clone(), target.project.clone()));
        if let Some(file) = &self.file
            && let Err(error) = self.write(file)
        {
            eprintln!("tasks: the folders seen could not be kept ({error})");
        }
    }

    fn write(&self, file: &Path) -> io::Result<()> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let kept = FoldersFile {
            version: 1,
            seen: self
                .seen
                .iter()
                .map(|(machine_id, project)| SeenFolder {
                    machine_id: machine_id.clone(),
                    project: project.clone(),
                })
                .collect(),
        };
        let bytes = serde_json::to_vec(&kept).map_err(io::Error::other)?;
        let name = file
            .file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no file name"))?;
        let mut partial = name.to_os_string();
        partial.push(".partial");
        let partial = file.with_file_name(partial);
        let mut out = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .custom_flags(libc_nofollow())
            .open(&partial)?;
        out.write_all(&bytes)?;
        out.sync_all()?;
        drop(out);
        fs::rename(&partial, file)?;
        if let Some(parent) = file.parent() {
            fs::File::open(parent)?.sync_all()?;
        }
        Ok(())
    }
}

/// `O_NOFOLLOW`, so the partial file is never written through a link.
fn libc_nofollow() -> i32 {
    OFlags::NOFOLLOW.bits() as i32
}

/// How much of an id a call line keeps: enough to find the same call in
/// Harness's own log on the computer, never the whole id.
const ID_CHARS: usize = 8;

/// The Harness verbs a call line names by name; any other is `other`.
const VERBS: [&str; 10] = [
    "new", "show", "ps", "wait", "reply", "close", "route", "machines", "start", "watch",
];

/// The first [`ID_CHARS`] of an id; a cloud agent's keeps its `cloud_`
/// first, so the part that tells two apart is there.
fn short_id(id: &str) -> String {
    let (kind, rest) = id
        .strip_prefix("cloud_")
        .map_or(("", id), |rest| ("cloud_", rest));
    format!("{kind}{}", rest.chars().take(ID_CHARS).collect::<String>())
}

/// A word Harness answers with that a call line may say: a state
/// (`needs-input`) or a decision (`new`), plain lowercase ASCII, 32 bytes at
/// most. Anything else is `?`.
fn state_word(value: &str) -> &str {
    if !value.is_empty()
        && value.len() <= 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"-_".contains(&byte))
    {
        value
    } else {
        "?"
    }
}

/// Harness's refusal code (`SPAWN_FAILED`, `NOT_IDLE`): upper-case ASCII,
/// digits and `_`, 3 to 48 bytes. Anything else, a message in words, is `?`.
fn error_code(value: &str) -> &str {
    if (3..=48).contains(&value.len())
        && value.starts_with(|c: char| c.is_ascii_uppercase())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    {
        value
    } else {
        "?"
    }
}

/// One Harness call's journal line, whatever its outcome (proposal L1 and
/// L2), so a slow or failed call can be found on a lamp and matched to
/// Harness's own log: `tasks: harness VERB: [machine=M] [agent=A |
/// agents=N] exit=CODE ms=T [out=B] [what stdout said]`. Ids are their
/// first [`ID_CHARS`] only ([`short_id`]), and only from where the adapter
/// put them, never from the ask (`--task` of a `new`, after `--` of a
/// `reply`). From
/// stdout only typed things ([`said_of_stdout`]). Never the ask, a
/// question, an answer, a name or Harness's words.
fn call_line(args: &[OsString], result: &Result<Reply, RunError>, took: Duration) -> String {
    // Only what comes before `--`: after it are the person's words.
    let named = &args[..args
        .iter()
        .position(|arg| arg == "--")
        .unwrap_or(args.len())];
    let arg = |at: usize| named.get(at).and_then(|arg| arg.to_str());
    let verb = match (arg(0), arg(1)) {
        (Some("machines"), Some("engines")) => "engines",
        (Some("auth"), Some("status")) => "auth-status",
        (Some(verb), _) if VERBS.contains(&verb) => verb,
        _ => "other",
    };
    let mut line = format!("tasks: harness {verb}:");
    // `machines engines ID`; every other verb names it with `--machine`.
    let machine_at = if verb == "engines" {
        Some(2)
    } else {
        named
            .iter()
            .position(|arg| arg == "--machine")
            .map(|at| at + 1)
    };
    if let Some(at) = machine_at {
        if let Some(machine) = arg(at).filter(|machine| machine_id(machine)) {
            line.push_str(&format!(" machine={}", short_id(machine)));
        }
        match verb {
            "show" | "reply" | "close" | "watch" => {
                if let Some(agent) = arg(at + 1).filter(|agent| agent_id(agent)) {
                    line.push_str(&format!(" agent={}", short_id(agent)));
                }
            }
            "wait" => {
                let count = arg(at + 1).map_or(0, |agents| {
                    agents.split(',').filter(|agent| agent_id(agent)).count()
                });
                line.push_str(&format!(" agents={count}"));
            }
            _ => {}
        }
    }
    let ms = took.as_millis();
    match result {
        Ok(reply) => {
            let exit = reply
                .code
                .map_or_else(|| "killed".to_owned(), |code| code.to_string());
            line.push_str(&format!(" exit={exit} ms={ms} out={}", reply.stdout.len()));
            line.push_str(&said_of_stdout(verb, &reply.stdout, reply.success));
        }
        // `timeout`, `cancelled`, `stdout_limit`, `spawn_failed` …: the
        // adapter's own code, never Harness's words.
        Err(error) => line.push_str(&format!(" exit={} ms={ms}", error.code)),
    }
    line
}

/// What a call line says of a Harness reply on stdout: typed fields only.
/// `state=` the agent's or the send's state, a route's decision, a sign-in's
/// or `ok`'s; `error=` Harness's code; `tools=` the agent's tool count;
/// `listed=` how many harnesses or machines, with `states=` how many in each
/// state; and the ids it gave, `id=` the agent's (`agent.id` of `new`, `id`
/// of `show`, `reply`, `route`), `ids=` those a `wait` came back for, and
/// only when a `new` or `reply` exited 0 with `ok: true` (`exited_ok`), no
/// error and no failed state, `creation=` a pending `new`'s and `message=`
/// a `reply`'s sent message's. Never an `ask`, `answer`, `about`, `name`,
/// `reason`, or a `message` that is Harness's words.
fn said_of_stdout(verb: &str, bytes: &[u8], exited_ok: bool) -> String {
    let mut said = String::new();
    // `machines --json` is one JSON line a machine.
    if verb == "machines" {
        let listed = String::from_utf8_lossy(bytes)
            .lines()
            .filter(|line| serde_json::from_str::<Value>(line).is_ok_and(|line| line.is_object()))
            .count();
        said.push_str(&format!(" listed={listed}"));
        return said;
    }
    let Ok(reply) = serde_json::from_slice::<Value>(bytes) else {
        return said;
    };
    let text = |value: &Value| value.as_str().map(str::to_owned);
    let state = if reply["pending"] == true {
        Some("pending".to_owned())
    } else if let Some(state) = text(&reply["state"])
        .or_else(|| text(&reply["sent"]["state"]))
        .or_else(|| text(&reply["decided"]))
    {
        Some(state_word(&state).to_owned())
    } else if let Some(signed_in) = reply["loggedIn"].as_bool() {
        Some(
            match (signed_in, reply["offline"] == true) {
                (false, _) => "signed-out",
                (true, true) => "offline",
                (true, false) => "signed-in",
            }
            .to_owned(),
        )
    } else {
        reply["ok"]
            .as_bool()
            .map(|ok| if ok { "ok" } else { "failed" }.to_owned())
    };
    if let Some(state) = state {
        said.push_str(&format!(" state={state}"));
    }
    if let Some(code) = text(&reply["error"])
        .or_else(|| text(&reply["sent"]["error"]))
        .or_else(|| text(&reply["sent"]["code"]))
    {
        said.push_str(&format!(" error={}", error_code(&code)));
    } else if reply["error"].is_object() {
        said.push_str(" error=?");
    }
    if let Some(tools) = reply["tools"].as_u64() {
        said.push_str(&format!(" tools={tools}"));
    }
    if let Some(rows) = reply["harnesses"].as_array() {
        let mut states: BTreeMap<&str, usize> = BTreeMap::new();
        for row in rows {
            *states
                .entry(row["state"].as_str().map_or("?", state_word))
                .or_default() += 1;
        }
        said.push_str(&format!(" listed={}", rows.len()));
        if !states.is_empty() {
            let states: Vec<String> = states
                .iter()
                .map(|(state, count)| format!("{state}:{count}"))
                .collect();
            said.push_str(&format!(" states={}", states.join(",")));
        }
        // The agents a `wait` came back for, a few at most.
        if verb == "wait" {
            let ids: Vec<String> = rows
                .iter()
                .filter_map(|row| row["id"].as_str().filter(|id| agent_id(id)))
                .take(4)
                .map(short_id)
                .collect();
            if !ids.is_empty() {
                said.push_str(&format!(" ids={}", ids.join(",")));
            }
        }
    }
    if let Some(id) = reply["agent"]["id"]
        .as_str()
        .or_else(|| reply["id"].as_str())
        .filter(|id| agent_id(id))
    {
        said.push_str(&format!(" id={}", short_id(id)));
    }
    // `creationId` and `message` are ids only in a successful `new` or
    // `reply`: exit 0, `ok: true`, no error and a state other than failed.
    // Elsewhere, and in any failure, `message` is Harness's own words (a
    // failed `close` or an `Unauthorized` puts only them there), never
    // journaled.
    let answered_ids = exited_ok
        && matches!(verb, "new" | "reply")
        && reply["ok"] == true
        && reply["error"].is_null()
        && reply["sent"]["error"].is_null()
        && reply["state"] != "failed"
        && reply["sent"]["state"] != "failed";
    if answered_ids {
        if let Some(id) = reply["creationId"].as_str().filter(|id| token(id, 96)) {
            said.push_str(&format!(" creation={}", short_id(id)));
        }
        if let Some(id) = reply["message"]
            .as_str()
            .or_else(|| reply["sent"]["message"].as_str())
            .filter(|id| token(id, 96))
        {
            said.push_str(&format!(" message={}", short_id(id)));
        }
    }
    said
}

/// The folder Harness made for the bare project name `project`, when the
/// reply is its refusal of a `new` for it: "“NAME” already exists on
/// COMPUTER: FOLDER. Choose another name …", FOLDER as that computer
/// reports it. A folder target is never one, and FOLDER must end in this
/// name.
fn name_taken(bytes: &[u8], project: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct Failed {
        ok: bool,
        error: String,
    }
    if project.starts_with('/') || project.starts_with("~/") {
        return None;
    }
    let reply = serde_json::from_slice::<Failed>(bytes).ok()?;
    if reply.ok {
        return None;
    }
    // "… exists on COMPUTER: FOLDER. Choose …": the computer's label comes
    // first, so the folder is what follows the last ": " before the end.
    let (said, _) = reply
        .error
        .strip_prefix(&format!("“{project}” already exists on "))?
        .split_once(". Choose another name")?;
    let (_, folder) = said.rsplit_once(": ")?;
    // The folder as the computer reports it: `~/…` from Harness 0.4.55, or
    // its absolute path (openharness `newCommand.ts`, for another
    // machine). Only one that ends in this name, with nothing odd in it.
    let well_formed = (folder.starts_with("~/") || folder.starts_with('/'))
        && folder.len() <= 512
        && !folder.chars().any(char::is_control)
        && folder.rsplit('/').next() == Some(project);
    well_formed.then(|| folder.to_owned())
}

/// Whether a harness has done nothing yet: nothing asked of it, no answer and
/// no tool used ([`Cli::first_message`]).
fn fresh(observation: &Observation) -> bool {
    observation.ask.is_none() && observation.answer.is_empty() && observation.tools == 0
}

/// What `harness route --json --send` answered ([`Cli::route_first`]).
enum Routed {
    /// Chose this session, and its words are queued in its mailbox.
    Sent(String),
    /// Chose a session that is working, or may be waiting on the person.
    Busy,
    /// Nothing typed: start new work, for this reason.
    New(&'static str),
    /// Chose a session and may have typed the words: an answer it can't read.
    Unsure,
}

fn route_answer(bytes: &[u8]) -> Routed {
    let Ok(answer) = serde_json::from_slice::<Value>(bytes) else {
        // No answer at all: nothing it decided can be read, so nothing is
        // taken as typed only when it said nothing.
        return if bytes.iter().all(u8::is_ascii_whitespace) {
            Routed::New("no_answer")
        } else {
            Routed::Unsure
        };
    };
    // A send that ended unconfirmed may have typed the words: uncertain,
    // never new (openharness#2252 says so under `sent`, with `uncertain`;
    // a `harness route` before it said the same codes as a bare error).
    let unconfirmed = |code: Option<&str>| {
        matches!(
            code,
            Some("UNCONFIRMED" | "DISCONNECTED" | "INVALID_RESPONSE" | "CANCELLED")
        )
    };
    if answer["sent"]["uncertain"] == true
        || unconfirmed(answer["sent"]["error"].as_str())
        || unconfirmed(answer["error"].as_str())
    {
        return Routed::Unsure;
    }
    // A refusal before or after deciding: Harness's code, at the top or
    // under `sent`. Anything else under `sent` typed nothing.
    let refused = answer["error"]
        .as_str()
        .or_else(|| answer["sent"]["error"].as_str())
        .or_else(|| answer["sent"]["code"].as_str());
    match refused {
        Some("NOT_IDLE") => return Routed::Busy,
        Some("NOT_RUNNING") => return Routed::New("not_running"),
        // Its machine's mailbox refused the words: nothing was typed.
        Some("FAILED") => return Routed::New("send_failed"),
        Some("ROUTER_UNAVAILABLE" | "JEV_UNAVAILABLE") => return Routed::New("router_unavailable"),
        Some("REPLY_OFF") => return Routed::New("reply_off"),
        Some(_) => return Routed::New("refused"),
        None => {}
    }
    match answer["decided"].as_str() {
        Some("new") => Routed::New("router"),
        Some("session") => match (answer["id"].as_str(), answer["sent"]["state"].as_str()) {
            (Some(id), Some("queued")) if agent_id(id) => Routed::Sent(id.to_owned()),
            // Its machine refused the words: nothing was typed.
            (_, Some("failed")) => Routed::New("send_failed"),
            _ => Routed::Unsure,
        },
        _ => Routed::Unsure,
    }
}

/// What `harness ps --all --json` says of the session the person named.
#[derive(Debug, PartialEq, Eq)]
enum Named {
    /// One live session fits and it is idle: its id, and how the person
    /// would know it.
    Idle(String, Option<JoinedSession>),
    /// Why nothing is typed.
    Refused(&'static str),
}

/// A name or folder as the person might say it: lowercase letters and
/// digits only, in any script, so "Docs-Update", "docs update" and
/// "docs_update" are one.
fn session_words(text: &str) -> String {
    words_of(text).concat()
}

/// The words of a name, folder or what was said: runs of letters and
/// digits, lowercased.
fn words_of(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// The session the person named among a computer's (#5535): a row fits
/// when its name or its folder's last part is the words said, or, when no
/// row is that, when each word said begins one of its words, in order
/// ("docs" fits "Docs-Update"; "api" never fits "rapid-prototype": the
/// Reviewer-lead's P2 on #5779), or, when none is, by what it worked on
/// ([`by_its_work`]). Of those that fit, one live session is
/// the one; a stopped one is said to be stopped; more than one live is
/// asked about, never guessed.
fn named_session(bytes: &[u8], session: &str) -> Named {
    let Ok(answer) = serde_json::from_slice::<Value>(bytes) else {
        return Named::Refused("sessions_failed");
    };
    let Some(rows) = answer["harnesses"].as_array() else {
        return Named::Refused("sessions_failed");
    };
    let said = session_words(session);
    if said.is_empty() {
        return Named::Refused("session_not_found");
    }
    let said_words = words_of(session);
    let parts = |row: &Value| -> [String; 2] {
        let folder = row["folder"]
            .as_str()
            .and_then(|folder| folder.trim_end_matches('/').rsplit('/').next())
            .unwrap_or_default();
        [
            row["name"].as_str().unwrap_or_default().to_owned(),
            folder.to_owned(),
        ]
    };
    let names = |row: &Value| parts(row).map(|part| session_words(&part));
    // Each word said begins a word of the name, in order.
    let begins = |part: &str| {
        let mut words = words_of(part).into_iter();
        said_words
            .iter()
            .all(|said| words.any(|word| word.starts_with(said.as_str())))
    };
    let exact: Vec<&Value> = rows
        .iter()
        .filter(|row| names(row).contains(&said))
        .collect();
    let fits: Vec<&Value> = if exact.is_empty() && said.chars().count() >= 3 {
        rows.iter()
            .filter(|row| parts(row).iter().any(|part| begins(part)))
            .collect()
    } else {
        exact
    };
    // No name or folder is it: what the person said is what the session
    // worked on ("the book program one"), found in what it was asked and
    // what it says it did (the owner, 9 Nov: a person remembers the work,
    // never a session's name or id).
    let by_work = fits.is_empty();
    let fits = if by_work {
        by_its_work(rows, &said_words)
    } else {
        fits
    };
    let gone = |row: &&Value| {
        matches!(
            row["state"].as_str(),
            Some("stopped" | "failed" | "offline")
        )
    };
    let live: Vec<&Value> = fits.iter().copied().filter(|row| !gone(row)).collect();
    // Found by its work, a live session and a stopped one that both hold
    // the words are not told apart: the one meant may be the stopped one,
    // so neither is typed into (the Reviewer-lead's P2 on #5784 at
    // 71bf964baa).
    if by_work && !live.is_empty() && live.len() < fits.len() {
        return Named::Refused("session_ambiguous");
    }
    match live.as_slice() {
        [] if fits.is_empty() => Named::Refused("session_not_found"),
        [] => Named::Refused("session_stopped"),
        [row] => match (row["id"].as_str(), row["state"].as_str()) {
            (Some(id), Some("idle")) if agent_id(id) => {
                Named::Idle(id.to_owned(), joined_session(row))
            }
            (Some(id), _) if agent_id(id) => Named::Refused("session_busy"),
            _ => Named::Refused("sessions_failed"),
        },
        _ => Named::Refused("session_ambiguous"),
    }
}

/// Which machine's session a named hand-over goes into (#5978), from what
/// each machine's list said, the launch's own first (`None`): a fit there
/// (anything but `session_not_found`) is the answer, so a session on the
/// lamp wins over one on a computer, and the lamp's own rules (stopped,
/// busy, a live and a stopped one by their work) hold there. Else one
/// computer with a fit is its answer, and more than one is
/// `session_ambiguous`, never guessed. None anywhere: `session_not_found`.
fn chosen_session(
    found: Vec<(Named, Option<JoinedOn>)>,
) -> Result<(Named, Option<JoinedOn>), &'static str> {
    let fits = |named: &Named| *named != Named::Refused("session_not_found");
    let mut found = found.into_iter();
    match found.next() {
        Some(own) if fits(&own.0) => return Ok(own),
        Some(_) => {}
        None => return Err("session_not_found"),
    }
    let mut elsewhere: Vec<(Named, Option<JoinedOn>)> =
        found.filter(|(named, _)| fits(named)).collect();
    match elsewhere.len() {
        0 => Err("session_not_found"),
        1 => Ok(elsewhere.remove(0)),
        _ => Err("session_ambiguous"),
    }
}

/// The most characters of a joined session's first ask kept on its task
/// ([`JoinedSession`]): enough to know the work by, never the whole of it.
const MOST_JOINED_ASK_CHARS: usize = 80;

/// Whether a session's name is one the lamp made (`lamp-` and hex, as
/// `engine::task_label` makes them): it names no work the person knows.
fn lamp_made(name: &str) -> bool {
    name.strip_prefix("lamp-")
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_hexdigit()))
}

/// How the person would know a `harness ps` row ([`JoinedSession`]): its
/// name, or, with none, its folder's last part, never a lamp-made one; a
/// lamp-made session's folder is the lamp's target folder, the same for
/// every task it hands over, so it tells nothing apart. And the first thing
/// it was asked, on one line, cut at a word to [`MOST_JOINED_ASK_CHARS`]
/// with "…". None when it has neither.
fn joined_session(row: &Value) -> Option<JoinedSession> {
    let flat = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
    let name = flat(row["name"].as_str().unwrap_or_default());
    let name = if name.is_empty() {
        row["folder"]
            .as_str()
            .and_then(|folder| folder.trim_end_matches('/').rsplit('/').next())
            .map(flat)
            .unwrap_or_default()
    } else {
        name
    };
    let name = (!name.is_empty() && !lamp_made(&name)).then_some(name);
    let ask = row["asks"]
        .as_array()
        .and_then(|asks| asks.first())
        .and_then(Value::as_str)
        .map(flat)
        .filter(|ask| !ask.is_empty())
        .map(|ask| cut_at_word(&ask, MOST_JOINED_ASK_CHARS));
    (name.is_some() || ask.is_some()).then_some(JoinedSession { name, ask })
}

/// `text` whole when it is at most `most` characters, else cut at the last
/// space that leaves room for "…" (at a character when it has none), and
/// "…": at most `most` characters either way.
fn cut_at_word(text: &str, most: usize) -> String {
    if text.chars().count() <= most {
        return text.to_owned();
    }
    // `most` characters: a space at the last of them still leaves room.
    let kept: String = text.chars().take(most).collect();
    let cut = match kept.rfind(' ').filter(|at| *at > 0) {
        Some(at) => kept[..at].to_owned(),
        None => kept.chars().take(most.saturating_sub(1)).collect(),
    };
    format!("{}…", cut.trim_end())
}

/// Words that say which session, not what it did: "the one from before",
/// "my earlier session", "that thing you did".
const NOT_THE_WORK: [&str; 40] = [
    "a", "an", "the", "one", "ones", "session", "sessions", "my", "your", "our", "that", "this",
    "which", "where", "who", "in", "on", "of", "for", "to", "with", "about", "from", "earlier",
    "before", "last", "previous", "other", "did", "do", "made", "was", "is", "it", "you", "we",
    "i", "thing", "work", "and",
];

/// Endings a word of a session's work may add to a word said, or the
/// other way round: "book" is "books", "fix" is "fixed", "test" is
/// "tests" but never "testament", "note" never "notebook" (the
/// Reviewer-lead's P2 on #5784 at de9bebdb41). A word said under
/// [`SHORTEST_STEM`] letters takes no ending.
const ENDINGS: [&str; 6] = ["s", "es", "ed", "ing", "er", "ers"];

/// The least time a named session's `harness reply` is given after its
/// list ([`Cli::reply_named`]): less than this left of the launch bound is
/// `sessions_timeout`, nothing typed.
const REPLY_FLOOR: Duration = Duration::from_secs(1);

/// The fewest letters a word needs before an ending may follow it: "us"
/// is never "user", nor "it" "items" (Reviewer-three's P2 on #5784 at
/// 71bf964baa); two letters are an acronym or a stop word, held only as
/// they are.
const SHORTEST_STEM: usize = 3;

/// Whether a word said stands for a word of a session's work: the same
/// word, or the same word with one of [`ENDINGS`].
fn same_word(said: &str, word: &str) -> bool {
    let (short, long) = if said.len() <= word.len() {
        (said, word)
    } else {
        (word, said)
    };
    short == long
        || short.len() >= SHORTEST_STEM
            && long
                .strip_prefix(short)
                .is_some_and(|ending| ENDINGS.contains(&ending))
}

/// The session whose work is what the person said (#5535): each row's
/// `asks` and `about` against the words said that name work
/// ([`NOT_THE_WORK`] left out). Its name and folder are not read here: a
/// name is matched only as [`named_session`] matches it, word starts in
/// order, so "client api" never finds "Fix the API client". A row fits
/// only when it holds every one of those words ([`same_word`]): "the
/// payroll dashboard one" never finds a weather dashboard on "dashboard"
/// alone (Reviewer-three's P2 on #5784 at 3d90e56736). Every fitting row
/// is given back, live or stopped: [`named_session`] says a stopped one is
/// stopped, and more than one fitting, a live one beside a stopped one
/// too, is asked about, never guessed.
fn by_its_work<'a>(rows: &'a [Value], said_words: &[String]) -> Vec<&'a Value> {
    let work: Vec<&str> = said_words
        .iter()
        .map(String::as_str)
        .filter(|word| !NOT_THE_WORK.contains(word))
        .collect();
    if work.is_empty() {
        return Vec::new();
    }
    let holds_all = |row: &Value| -> bool {
        let mut text: Vec<String> = words_of(row["about"].as_str().unwrap_or_default());
        for ask in row["asks"].as_array().into_iter().flatten() {
            text.extend(words_of(ask.as_str().unwrap_or_default()));
        }
        work.iter()
            .all(|said| text.iter().any(|word| same_word(said, word)))
    };
    rows.iter().filter(|row| holds_all(row)).collect()
}

/// Whether `harness reply --json` took the words for `agent_id`: queued in
/// its machine's mailbox, not refused there.
fn replied(bytes: &[u8], agent_id: &str) -> bool {
    serde_json::from_slice::<Value>(bytes).is_ok_and(|reply| {
        reply["ok"] == true && reply["id"] == agent_id && reply["state"] != "failed"
    })
}

/// A refused or failed `harness reply`, by Harness's code.
fn reply_refusal(bytes: &[u8]) -> &'static str {
    let reply = serde_json::from_slice::<Value>(bytes).unwrap_or_default();
    match reply["error"].as_str() {
        Some("REPLY_OFF") => "reply_off",
        Some("NOT_IDLE") => "reply_not_idle",
        Some("NOT_RUNNING") => "reply_not_running",
        _ => "reply_failed",
    }
}

fn parse_launch(bytes: &[u8], launch: &Launch) -> LaunchOutcome {
    let uncertain = |code: &str| LaunchOutcome::Uncertain { code: code.into() };
    let Ok(reply) = serde_json::from_slice::<NewReply>(bytes) else {
        return uncertain("invalid_reply");
    };
    if !reply.ok {
        return uncertain("new_failed");
    }
    if reply.error.is_some() {
        return uncertain("invalid_reply");
    }
    if reply.machine_id.as_deref() != Some(&launch.target.machine_id) {
        return uncertain("machine_mismatch");
    }
    if reply.existing == Some(true) {
        return uncertain("existing_unbound");
    }
    if reply.pending == Some(true) {
        if reply.agent.is_some() || reply.name.as_deref() != Some(&launch.name) {
            return uncertain("invalid_reply");
        }
        let Some(id) = reply
            .creation_id
            .filter(|id| id.len() >= 16 && token(id, 96))
        else {
            return uncertain("invalid_reply");
        };
        return LaunchOutcome::Pending { creation_id: id };
    }
    if reply.creation_id.is_some() {
        return uncertain("invalid_reply");
    }
    let Some(agent) = reply.agent else {
        return uncertain("invalid_reply");
    };
    if !agent_id(&agent.id)
        || agent.name != launch.name
        || agent.engine != launch.target.engine
        || !asked_permission(
            launch.target.engine,
            agent.permission_mode.as_deref(),
            agent.bypass_permission.as_ref(),
        )
    {
        return uncertain("invalid_reply");
    }
    LaunchOutcome::Created { agent_id: agent.id }
}

/// The launch asks for `--mode auto`. Harness 0.4.6 reports that mode as
/// `permissionMode: "auto"` with `bypassPermission: true` (the agent does not
/// ask before ordinary edits; it still stops for risky actions). Any other
/// mode, `full` among them, is not what was asked for.
fn auto_permission(mode: &str, bypass: &Value) -> bool {
    mode == "auto" && (bypass.is_null() || bypass.is_boolean())
}

/// What was asked for: `auto` for an agent with modes; for one with none
/// (Devin), no mode was sent, so the receipt must claim none that approves
/// on its own (`auto` or `full`) and no bypass. Its exact receipt is not yet
/// seen from Harness: absent, null, or another mode, are all taken.
fn asked_permission(engine: EngineKind, mode: Option<&str>, bypass: Option<&Value>) -> bool {
    if engine.has_modes() {
        return mode
            .zip(bypass)
            .is_some_and(|(mode, bypass)| auto_permission(mode, bypass));
    }
    !matches!(mode, Some("auto" | "full")) && bypass != Some(&Value::Bool(true))
}

#[derive(Deserialize)]
struct ShowReply {
    id: String,
    state: String,
    ask: Value,
    answer: String,
    truncated: bool,
    tools: u64,
    ok: Option<bool>,
    error: Option<Value>,
}

fn parse_observation(bytes: &[u8], expected: &str) -> Result<Observation, &'static str> {
    let reply: ShowReply = serde_json::from_slice(bytes).map_err(|_| "invalid_observation")?;
    if reply.ok == Some(false) || reply.error.is_some() {
        return Err("invalid_observation");
    }
    if reply.id != expected {
        return Err("agent_mismatch");
    }
    if ![
        "working",
        "idle",
        "needs-input",
        "stopped",
        "offline",
        "unknown",
        "installing",
        "starting",
        "failed",
    ]
    .contains(&reply.state.as_str())
        || reply.answer.len() > MAX_ANSWER_BYTES
        || reply.answer.contains('\0')
        || reply.tools > 1_000_000
    {
        return Err("invalid_observation");
    }
    let ask = match reply.ask {
        Value::Null => None,
        Value::String(ask) if ask.len() <= MAX_ASK_BYTES && !ask.contains('\0') => Some(ask),
        _ => return Err("invalid_observation"),
    };
    Ok(Observation {
        state: state_of(&reply.id, reply.state, Some(&reply.answer)),
        agent_id: reply.id,
        ask,
        answer: reply.answer,
        truncated: reply.truncated,
        tools: reply.tools,
    })
}

#[derive(Deserialize)]
struct PsReply {
    harnesses: Vec<PsRow>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PsRow {
    id: String,
    state: String,
    #[serde(default)]
    last_active: Option<String>,
    #[serde(default)]
    about: Option<String>,
}

/// `harness close`'s exit code (Harness 0.4.18's CLI): 0 closed, 5 refused
/// (`NOT_YOURS`, `SPAWN_LIMIT`, `TURNED_OFF`; do not retry). Everything else
/// is exit 1, with only a message in the `--json` reply: a harness working or
/// waiting for the person (`WORKING`), one that changed while closing
/// (`AGENT_CHANGED`) or one already gone alike. So none is told apart here:
/// it failed this time, and the next try reads the agent again first.
pub fn closed(code: Option<i32>) -> Result<Closed, String> {
    match code {
        Some(0) => Ok(Closed::Closed),
        Some(5) => Ok(Closed::Refused),
        Some(_) => Err("close_failed".into()),
        None => Err("close_killed".into()),
    }
}

/// Whether Harness is signed in to the person's account.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignIn {
    In,
    /// Signed in, and its sign-in could not be renewed just now: no
    /// network, or the sign-in service is down.
    Offline,
    Out,
}

impl SignIn {
    pub fn name(self) -> &'static str {
        match self {
            Self::In => "yes",
            Self::Offline => "offline",
            Self::Out => "no",
        }
    }
}

/// What `harness auth status --json` says (openharness `cli.ts`
/// `authStatusCommand`): one line, exit 0 signed in or not, with
/// `loggedIn`, and `offline` when signed in but unrenewable just now.
/// Other fields are passed over.
pub fn parse_auth_status(bytes: &[u8]) -> Result<SignIn, &'static str> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Status {
        logged_in: bool,
        #[serde(default)]
        offline: bool,
    }
    let status: Status = serde_json::from_slice(bytes).map_err(|_| "auth_status_invalid_reply")?;
    Ok(match (status.logged_in, status.offline) {
        (false, _) => SignIn::Out,
        (true, true) => SignIn::Offline,
        (true, false) => SignIn::In,
    })
}

/// What `harness ps --json` says (Harness 0.4.2): an object whose
/// `harnesses` each have `id`, `state`, `lastActive` (ISO 8601 in UTC, or
/// null) and, while the agent is not stopped, an `about` line made of its
/// last few events. A harness still being created is listed by its
/// creation id, which is not an agent id: it is passed over. Unknown fields
/// are allowed, since the listing is for people and may grow.
pub fn parse_ps(bytes: &[u8]) -> Result<Vec<Listed>, &'static str> {
    if bytes.len() > PS_STDOUT_LIMIT {
        return Err("status_invalid_reply");
    }
    let reply: PsReply = serde_json::from_slice(bytes).map_err(|_| "status_invalid_reply")?;
    if reply.harnesses.len() > MOST_LISTED {
        return Err("status_invalid_reply");
    }
    let mut listed = Vec::new();
    for row in reply.harnesses {
        if !agent_id(&row.id) {
            continue;
        }
        if !token(&row.state, 32) {
            return Err("status_invalid_reply");
        }
        // A time in a form this does not read is not known, rather than a
        // reason to lose the whole listing.
        let last_active_unix_ms = row.last_active.as_deref().and_then(iso_unix_ms);
        let about = row
            .about
            .map(|about| clip(about.trim(), MOST_ABOUT_BYTES))
            .filter(|about| !about.is_empty());
        listed.push(Listed {
            state: state_of(&row.id, row.state, None),
            agent_id: row.id,
            last_active_unix_ms,
            about,
        });
    }
    Ok(listed)
}

#[derive(Deserialize)]
struct WaitReply {
    harnesses: Vec<WaitRow>,
}
#[derive(Deserialize)]
struct WaitRow {
    id: String,
    state: String,
}

/// What `harness wait --any --json` says (Harness 0.4.17), by its exit:
/// 0 an agent is idle, 3 one waits for the person, 4 one stopped or is
/// gone, each with `harnesses` listing those that ended (`id`, `state`);
/// 124 the time passed with none, which is no agent. Rows for agents not
/// waited on, and `timeout` rows, are passed over. Anything else fails.
pub fn parse_wait(
    code: Option<i32>,
    bytes: &[u8],
    agents: &[String],
) -> Result<Vec<Ended>, &'static str> {
    match code {
        Some(124) => return Ok(Vec::new()),
        Some(0 | 3 | 4) => {}
        _ => return Err("wait_failed"),
    }
    let reply: WaitReply = serde_json::from_slice(bytes).map_err(|_| "wait_invalid_reply")?;
    if reply.harnesses.len() > MOST_LISTED {
        return Err("wait_invalid_reply");
    }
    let mut ended = Vec::new();
    for row in reply.harnesses {
        if !token(&row.state, 32) {
            return Err("wait_invalid_reply");
        }
        if row.state == "timeout" || !agents.contains(&row.id) {
            continue;
        }
        ended.push(Ended {
            state: state_of(&row.id, row.state, None),
            agent_id: row.id,
        });
    }
    // An exit that says one ended, with none of ours listed, is not read.
    if ended.is_empty() {
        return Err("wait_invalid_reply");
    }
    Ok(ended)
}

/// One line of `harness watch --json`: an event, or the error it ended on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WatchLine {
    Event(WatchEvent),
    /// `{"error":"CODE","detail":"..."}`: only the code is kept.
    Error(String),
}

#[derive(Deserialize)]
struct WatchRow {
    #[serde(default)]
    event: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

/// One line of `harness watch --json` for `agent` (openharness `watch`):
/// every event line names the harness's `id`, and one for another id is
/// refused. Only the event is read: the summary, ask and question stay
/// with Harness, and the task's own read (`show`) takes the answer.
pub fn parse_watch_line(line: &[u8], agent: &str) -> Result<WatchLine, &'static str> {
    if line.len() > WATCH_LINE_LIMIT {
        return Err("watch_invalid_line");
    }
    let row: WatchRow = serde_json::from_slice(line).map_err(|_| "watch_invalid_line")?;
    if let Some(code) = row.error {
        return Ok(WatchLine::Error(error_code(&code).to_owned()));
    }
    if row.id.as_deref() != Some(agent) {
        return Err("watch_agent_mismatch");
    }
    row.event
        .as_deref()
        .and_then(WatchEvent::of)
        .map(WatchLine::Event)
        .ok_or("watch_invalid_line")
}

/// At most `most` bytes of `text`, ended at a whole character, without
/// control characters other than line ends and tabs, which become spaces.
fn clip(text: &str, most: usize) -> String {
    let mut end = text.len().min(most);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end]
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// `2026-10-15T00:05:12.345Z` as milliseconds since 1970: the form
/// JavaScript's `toISOString` writes, which is what Harness sends. Anything
/// else is not read.
pub fn iso_unix_ms(text: &str) -> Option<u64> {
    let bytes = text.as_bytes();
    if !(20..=24).contains(&bytes.len()) || bytes[bytes.len() - 1] != b'Z' {
        return None;
    }
    let digits = |from: usize, to: usize| -> Option<u64> {
        let part = text.get(from..to)?;
        (!part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
            .then(|| part.parse().ok())
            .flatten()
    };
    if [4, 7].iter().any(|at| bytes[*at] != b'-')
        || bytes[10] != b'T'
        || [13, 16].iter().any(|at| bytes[*at] != b':')
    {
        return None;
    }
    let (year, month, day) = (digits(0, 4)?, digits(5, 7)?, digits(8, 10)?);
    let (hour, minute, second) = (digits(11, 13)?, digits(14, 16)?, digits(17, 19)?);
    let millis = match bytes[19] {
        b'Z' if bytes.len() == 20 => 0,
        b'.' if bytes.len() == 24 => digits(20, 23)?,
        _ => return None,
    };
    if !(1970..=9999).contains(&year)
        || !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    // Days since 1970 from a civil date: the usual arithmetic on eras of
    // 400 years that begin on 1 March.
    let (y, m) = if month <= 2 {
        (year - 1, month + 9)
    } else {
        (year, month - 3)
    };
    let era = y / 400;
    let of_era = y - era * 400;
    let of_year = (153 * m + 2) / 5 + day - 1;
    let of_era_days = of_era * 365 + of_era / 4 - of_era / 100 + of_year;
    let days = (era * 146_097 + of_era_days).checked_sub(719_468)?;
    Some(((days * 24 + hour) * 60 + minute) * 60_000 + second * 1_000 + millis)
}
