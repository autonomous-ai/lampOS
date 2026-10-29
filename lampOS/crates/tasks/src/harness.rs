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
    "ADAPTER_DATA_DIR",
    "HARNESS_AUTH_DIR",
    "PORT",
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
    fn argument(self) -> &'static str {
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
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LaunchOutcome {
    Created { agent_id: String },
    Pending { creation_id: String },
    NotSent { code: String },
    Uncertain { code: String },
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
/// No more agents than this are waited on in one `wait`.
pub const MOST_WAITED: usize = 64;
/// How long `harness machines engines` is given (`serve --engines-ready`):
/// the computer's Harness reads the one engine in its login shell, 5 s at
/// most there (openharness `engineProbe.ts`), over the machine link.
pub const ENGINES_PROBE_TIMEOUT: Duration = Duration::from_secs(8);
/// How long one computer's answer for one engine is kept when it refuses
/// nothing: a second hand-over within it asks nothing. A refusal is never
/// kept: the person may sign in at once and ask again.
pub const ENGINES_KEPT: Duration = Duration::from_secs(300);
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
        let maximum = if folder { 512 } else { 128 };
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
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct CliConfig {
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
    /// Each dispatch's journal line ([`dispatch_line`]) is also kept here,
    /// for a test to read; it always goes to stderr, the unit's journal.
    pub journal: Option<Arc<Mutex<Vec<String>>>>,
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
            cancel: None,
            as_person: false,
            folder_missing: false,
            engines_ready: false,
            journal: None,
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
        Ok(Self {
            config,
            caller_session,
            operation: Mutex::new(()),
            poisoned: AtomicBool::new(false),
            executable,
            ready: Mutex::new(BTreeMap::new()),
        })
    }

    fn cancelled(&self, also: Option<&AtomicBool>) -> bool {
        self.config
            .cancel
            .as_ref()
            .is_some_and(|cancel| cancel.load(Ordering::Acquire))
            || also.is_some_and(|cancel| cancel.load(Ordering::Acquire))
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
        let project_at = (!launch.target.engine.cloud()).then(|| {
            args.push(launch.target.project.clone().into());
            args.len() - 1
        });
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
        // One `launch_timeout` for the whole `new`, a retry included, and
        // Devin's first message after it.
        let until = Instant::now() + self.config.launch_timeout;
        let mut reply = self.run(&args, self.config.launch_timeout);
        seen.took(&reply);
        // A bare name gets its folder, `~/harnesses/<name>`, from Harness on
        // the first `new`, and every later `new` on that name is refused
        // for it, before anything starts (Harness 0.4.55; Mobile on
        // lamp-0c89, 23 Oct). On that refusal alone, asked once more in
        // the folder it names, in what is left of the bound; nothing else
        // is ever asked twice.
        let left = until.saturating_duration_since(Instant::now());
        if let (Some(at), Ok(refused)) = (project_at, &reply)
            && !refused.success
            && !left.is_zero()
            && let Some(folder) = name_taken(&refused.stdout, &launch.target.project)
        {
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

    /// The agent's last turn, read on the machine it was started on.
    pub fn observe(&self, machine: &str, agent_id: &str) -> Result<Observation, String> {
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
                self.config.read_timeout,
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
    /// the agent just before ([`Cli::observe`]), so this only closes one
    /// still idle on the turn the lamp told.
    pub fn close(&self, machine: &str, agent_id: &str) -> Result<Closed, String> {
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
                self.config.launch_timeout,
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
        Ok(())
    }

    /// Harness's own sign-in by a spoken code (`harness login --code
    /// --json`, Harness 0.4.23; `serve --sign-in-by-code`, #1676): the same
    /// program, checked as every run is, with the same environment and
    /// `HOME`, but its own life ([`crate::sign_in::MOST_LIFE`]), its stdin
    /// kept for the person's answer and its lines read as they come. Never
    /// takes the operation lock: a sign-in waits minutes on a person.
    pub fn sign_in_by_code(&self) -> Result<Box<dyn crate::sign_in::Link>, String> {
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
            .args(["login", "--code", "--json"])
            .env_clear()
            .envs(&self.config.environment);
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

    fn run_cancellable(
        &self,
        args: &[OsString],
        timeout: Duration,
        stdout_limit: usize,
        also: Option<&AtomicBool>,
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
            .envs(&self.config.environment);
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
        let result = self.capture(&mut owned, deadline, stdout_limit, also);
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
                    return Ok(Reply {
                        success: code == Some(0),
                        code,
                        stdout: output,
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
}

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
        listed.push(AccountMachine {
            machine_id: machine.machine_id,
            label,
            current: machine.current,
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
