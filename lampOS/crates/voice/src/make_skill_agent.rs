//! A voice make the lamp's own model couldn't make, handed to the lamp's
//! own agent (a proposal on #6059, the owner's OK and the first builder's
//! yes, 11 Nov): with `LAMP_MAKE_SKILL_AGENT=on` ([`ENVIRONMENT`], off
//! unless set; on in the product's lamp-voice again since 15 Nov, after
//! #7014 had it off, a code make by the agent passing 3 of 3 on lamp-9adf).
//!
//! The order: DeepSeek writes first ([`crate::make_skill_local`]). With
//! Hermes as the lamp's agent (the tasks part's `lamp` target; only Hermes
//! has a namespace runner for a make today, so another engine, OpenClaw,
//! is never handed one and DeepSeek makes it alone) it gets one try and
//! no repairs: an answer not kept (the tidies, the check, the say-back, its
//! own `cannot:`, or no answer) goes at once, with the same system prompt
//! and request, to the agent, which writes the blocks again and gets the
//! repairs. Its answers go through the very same checks. With no agent on
//! the lamp DeepSeek keeps its repairs, as today; with no answer from the
//! agent within [`WRITE_WITHIN`], or its run busy or failed, DeepSeek's
//! failure stands, said as the agent's ([`not_made_try`]).
//!
//! The agent writes in a skill run's namespace (#6643, the first builder's
//! guard, 12 Nov 18:15): the make goes to lamp-tasks' make socket
//! ([`MAKE_SOCKET_NAME`], `lamp_tasks::skill_make`), which runs the lamp's
//! Hermes exactly as a skill's `do.task` run is started (row-6443's path),
//! through its namespace wrapper: `control.sock` alone and never
//! `admin.sock`. Its network is kept by default, as V1's agent had it (the
//! owner's ask, 12 Nov); lamp-tasks' `LAMP_MAKE_AGENT_NET=held` gives the
//! strict run, `LAMP_NS_NET=run`: only `lo` and the kernel's `@NONE`
//! tunnels, the model only through `model.sock`. It writes its answer in a
//! make's folder of its own, which lamp-tasks reads back. Never through a
//! Harness task with the agent's own tools: when lamp-tasks can't give that
//! run (an older build with no make socket, the switch off, a wrapper that
//! can't hold it off when held, or a wrapper or namespace script with no
//! make-home line, `no_namespace`), the make is not handed over and DeepSeek's
//! failure stands ([`NotWritten::NoMakeRun`]).
//!
//! With [`GUIDE_ENVIRONMENT`] `on` too (`LAMP_MAKE_AGENT_GUIDE`, off unless
//! set; the owner's ask, 13 Nov: the agent makes a skill as V1's
//! `skill-creator` did), each make's line asks lamp-tasks for the lamp's
//! skill-creator guide (`"guide":true`, `lamp_tasks::skill_make::GUIDE`):
//! Hermes takes the intent from the person's words, drafts the skill,
//! checks it with the lamp's own check (`lamp-skills pack`, the voice's
//! own) and fixes it before it answers, each answer given
//! [`GUIDED_WRITE_WITHIN`] for that. The voice still puts every answer
//! through its whole check, and one it refuses goes back with the check's
//! words at most [`GUIDED_MOST_REPAIRS`] more times, within the agent's
//! [`MAKE_WITHIN`]. Off, the line is today's, byte for byte.
//!
//! With the guide, the line also carries this process's values of the
//! switches `lamp-skills` reads as it packs a skill ([`CHECK_SWITCHES`],
//! `"switches"`, Heron's P2 on #6768): lamp-tasks starts the make with a
//! cleared environment, so without them the guide's check would refuse a
//! `write` step or a `do.task`'s service grant the voice's own check
//! accepts, and Hermes would take the asked-for step out.
//!
//! With [`ONE_PATH_ENVIRONMENT`] `on` too (`LAMP_MAKE_AGENT_ONE_PATH`, off
//! unless set; #7245), a make goes to the lamp's agent when it is one the
//! one run path takes ([`ONE_PATH_ENGINES`]: Hermes or OpenClaw), and its
//! line carries `"one_path":true` so lamp-tasks' make socket picks its
//! runner by `default_agent_takes_runs` (#7133's `HarnessRunner` for a
//! gateway agent). Off, the line and the hand-over are today's, byte for
//! byte. A `cannot:` answer is passed on verbatim, as before (#7234). It is
//! the voice's make's alone ([`Agent::for_voice`]): the app's MAKE IT below
//! hands over to Hermes only, as before.
//!
//! The app's MAKE IT (`crate::app_make`, the owner's ask, 13 Nov) makes the
//! same way: its typed words go through the same order and this same
//! hand-over ([`Agent::for_app`]), read from the coordinator's own
//! environment, where the app's makes run (its `LAMP_TASKS_DIR`, the
//! directory the second route's worker is given). No waiting line is said
//! for it: the app shows the ask as making until it is on or not kept.
use crate::agent_tasks;
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    sync::OnceLock,
    time::Duration,
};

/// The switch: `on`, and nothing else.
pub const ENVIRONMENT: &str = "LAMP_MAKE_SKILL_AGENT";
/// The longest one of the agent's answers is waited for, from the hand-over
/// to its answer kept: the make run's own deadline, within lamp-tasks'
/// most (`lamp_tasks::skill_make::MOST_WITHIN`, 120 s). 45 s before #6749:
/// on lamp-9adf (an Orange Pi, build 12958) a make with code ran out at
/// 45 s 2 of 2, where `hermes chat -q "Say OK."` alone takes about 24.5 s,
/// about 13 s of it before Hermes's own session; a gmail make with no
/// script took 30 s on another lamp. 90 s leaves about a minute for the
/// work once Hermes is up.
pub const WRITE_WITHIN: Duration = Duration::from_secs(90);
/// The longest the agent's part of a make goes on, its repairs included: a
/// repair is begun only while one more answer ([`WRITE_WITHIN`]) still
/// ends within it.
pub const MAKE_WITHIN: Duration = Duration::from_secs(120);
/// The guide's switch: `on`, and nothing else; off unless set.
pub const GUIDE_ENVIRONMENT: &str = "LAMP_MAKE_AGENT_GUIDE";
/// With the guide, the longest one of the agent's answers is waited for: a
/// draft, its check and up to two fixes in one run, within lamp-tasks'
/// most (`lamp_tasks::skill_make::MOST_WITHIN`, 120 s) and the agent's
/// [`MAKE_WITHIN`].
pub const GUIDED_WRITE_WITHIN: Duration = Duration::from_secs(90);
/// With the guide, the most times an answer the voice's check refused goes
/// back to the agent with the check's words.
pub const GUIDED_MOST_REPAIRS: usize = 2;
/// lamp-tasks' make socket in its folder (`lamp_tasks::skill_make::MAKE_SOCKET`).
pub const MAKE_SOCKET_NAME: &str = "skill-make.sock";
/// What the lamp says as the agent begins, so the person isn't left in
/// silence: in the make's own voice turn when one waits on it
/// ([`waiting_due`]), else through the say service.
pub const WAITING_LINE: &str = "Let me think about that a little longer.";
/// What the lamp says when the agent's own answer is `cannot:`: never the
/// agent's words.
pub const CANNOT_LINE: &str = "I couldn't make that one.";
/// How long the waiting line may wait to be said: past it, it is old.
const WAITING_WITHIN_MS: u64 = 15_000;
/// How long the line may take to send.
const SEND_WITHIN: Duration = Duration::from_secs(5);
/// The most of an answer taken, as the lamp's own model's.
const MOST_ANSWER_BYTES: usize = 20 * 1024;
/// lamp-tasks' refusals that mean no make run can be had here.
/// `no_namespace`: the wrapper or the namespace script has no make-home
/// line yet (an older lamp-update round).
const NOT_HELD_OFF_CODES: [&str; 3] = ["off", "not_held_off", "no_namespace"];
/// lamp-tasks' refusal of a make that can't start because Harness or the
/// gateway isn't ready yet (#7245, `lamp_tasks::skill_make::NOT_READY`).
const NOT_READY_CODE: &str = "not_ready";
/// The one engine a make is handed to: only Hermes has a namespace runner
/// for a make today (lamp-tasks' `MakeHermes`, #6643's 19:14 ruling).
pub const MAKE_ENGINE: &str = "Hermes";
/// The one run path's switch for a make (#7245): `on`, and nothing else;
/// off unless set.
pub const ONE_PATH_ENVIRONMENT: &str = "LAMP_MAKE_AGENT_ONE_PATH";
/// With [`ONE_PATH_ENVIRONMENT`] on, the engines a make is handed to:
/// lamp-tasks takes each through the one run path (#7133).
pub const ONE_PATH_ENGINES: [&str; 2] = [MAKE_ENGINE, "OpenClaw"];

/// Whether the switch is on.
pub fn on() -> bool {
    on_with(std::env::var(ENVIRONMENT).ok().as_deref())
}

/// Whether `value` turns the switch on: `on`, and nothing else.
fn on_with(value: Option<&str>) -> bool {
    value == Some("on")
}

/// The voice turns waiting on a make, each with its request and the
/// sender its waiting line is told on ([`waiting_in_turn`]).
static IN_TURN: std::sync::Mutex<Vec<(u64, u64, tokio::sync::oneshot::Sender<()>)>> =
    std::sync::Mutex::new(Vec::new());
/// The last [`InTurn`]'s number.
static IN_TURN_MADE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

std::thread_local! {
    /// The voice request whose turn waits on the make on this thread
    /// ([`set_made_for`]); 0 for none.
    static MADE_FOR: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Kept on the thread a voice make runs on (the second route's
/// `made_running`): the request whose turn waits on it, 0 for none.
pub fn set_made_for(request: u64) {
    MADE_FOR.with(|kept| kept.set(request));
}

/// A voice turn waiting on its make (#6749): while it is held, the make's
/// waiting line is told to it ([`waiting_due`]), to be said in the turn
/// itself, after its words so far. A say through the say service waits for
/// the turn to end, as every say does: on lamp-9adf, 14 Nov, the line
/// asked there was `Some(Expired)` 2 of 2 and the person waited about 49 s
/// in silence. Dropped, the turn no longer waits.
pub struct InTurn {
    made: u64,
    /// Sent once, when the make's waiting line is due.
    pub due: tokio::sync::oneshot::Receiver<()>,
}

impl Drop for InTurn {
    fn drop(&mut self) {
        if let Ok(mut waiting) = IN_TURN.lock() {
            waiting.retain(|(made, ..)| *made != self.made);
        }
    }
}

/// `request`'s turn waits on a make: its waiting line comes to it.
pub fn waiting_in_turn(request: u64) -> InTurn {
    let made = IN_TURN_MADE.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    let (told, due) = tokio::sync::oneshot::channel();
    if let Ok(mut waiting) = IN_TURN.lock() {
        waiting.push((made, request, told));
    }
    InTurn { made, due }
}

/// Tells `request`'s waiting turn ([`waiting_in_turn`]) that its make's
/// waiting line is due: true when a turn waited on it, which says it (or
/// was cut, and nobody is left to hear it); false with none, and the say
/// service is asked instead.
pub fn waiting_due(request: u64) -> bool {
    if request == 0 {
        return false;
    }
    let told = IN_TURN.lock().ok().and_then(|mut waiting| {
        let at = waiting.iter().position(|(_, of, _)| *of == request)?;
        Some(waiting.remove(at).2)
    });
    match told {
        Some(told) => {
            let _ = told.send(());
            true
        }
        None => false,
    }
}

/// Whether the guide's switch is on ([`GUIDE_ENVIRONMENT`]).
pub fn guide_on() -> bool {
    on_with(std::env::var(GUIDE_ENVIRONMENT).ok().as_deref())
}

/// Whether the one run path's switch is on ([`ONE_PATH_ENVIRONMENT`]).
pub fn one_path_on() -> bool {
    on_with(std::env::var(ONE_PATH_ENVIRONMENT).ok().as_deref())
}

/// The switches `lamp-skills pack` reads as it loads a skill, so that change
/// what it refuses: a `do.task`'s linked service grants and a `write` step
/// or permission (`lamp_tasks::skill_make::CHECK_SWITCHES`, the only names
/// lamp-tasks takes). The voice's check runs with the environment of the
/// process that makes: the second route's worker, which has each as `on`
/// exactly or not at all (`process::agent_services_worker_environment`,
/// `skill_writes_worker_environment`), or the coordinator for the app's
/// MAKE IT, with lamp-voice's own unit's.
pub const CHECK_SWITCHES: [&str; 2] = [
    crate::made_skill::AGENT_SERVICES_ENVIRONMENT,
    crate::made_skill::WRITES_ENVIRONMENT,
];

/// The line's `"switches"`: each of [`CHECK_SWITCHES`] `"on"` when `var`
/// gives it as `on` exactly, else `"off"`.
pub fn check_switches_with(var: impl Fn(&str) -> Option<String>) -> Value {
    CHECK_SWITCHES
        .iter()
        .map(|name| {
            let on = on_with(var(name).as_deref());
            ((*name).to_owned(), json!(if on { "on" } else { "off" }))
        })
        .collect::<serde_json::Map<_, _>>()
        .into()
}

/// [`check_switches_with`] this process's environment.
pub fn check_switches() -> Value {
    check_switches_with(|name| std::env::var(name).ok())
}

/// The tasks part's folder, as the second route's worker was given it
/// (`--tasks-dir`).
static TASKS: OnceLock<PathBuf> = OnceLock::new();

/// Kept by the second route's worker as it starts, with a tasks part.
pub fn set_tasks_dir(directory: PathBuf) {
    let _ = TASKS.set(directory);
}

/// The lamp's agent, reached through the tasks part.
#[derive(Clone, Debug)]
pub struct Agent {
    /// The tasks part's folder: `request.sock` and [`MAKE_SOCKET_NAME`].
    pub tasks: PathBuf,
    /// The say socket the waiting line goes to; none says nothing.
    pub say: Option<PathBuf>,
    /// [`WRITE_WITHIN`], or a test's own.
    pub within: Duration,
    /// [`MAKE_WITHIN`], or a test's own.
    pub make_within: Duration,
    /// Whether each make asks for the lamp's skill-creator guide
    /// ([`GUIDE_ENVIRONMENT`]).
    pub guide: bool,
    /// Whether a make goes to the lamp's agent on the one run path, its
    /// line carrying `"one_path":true` ([`ONE_PATH_ENVIRONMENT`]).
    pub one_path: bool,
    /// Whether DeepSeek gets its repairs when the agent doesn't take the
    /// make (no make run, `busy`, or no first answer): the app's MAKE IT,
    /// with no turn waiting ([`Agent::for_app`], Heron's P2 on #6788); a
    /// voice make's one answer stands, as before.
    pub repairs_if_untaken: bool,
}

/// Why the agent gave no answer to take.
#[derive(Debug, PartialEq, Eq)]
pub enum NotWritten {
    /// The tasks part has no `lamp` target, or couldn't be asked.
    NoAgent,
    /// lamp-tasks can't run the agent in a skill run's namespace here: an
    /// older build with no make socket, its switch off, or (held) a wrapper
    /// that can't hold it off. Never handed over any other way.
    NoMakeRun,
    /// No answer within [`Agent::within`].
    Timeout,
    /// Anything else, in the lamp's own words (never the agent's).
    Failed(String),
}

impl std::fmt::Display for NotWritten {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoAgent => f.write_str("no agent on the lamp"),
            Self::NoMakeRun => f.write_str("no make run in the agent's namespace can be had here"),
            Self::Timeout => write!(f, "no answer within {} s", WRITE_WITHIN.as_secs()),
            Self::Failed(why) => f.write_str(why),
        }
    }
}

/// What the person hears when the agent made nothing for a reason of its
/// own side (`why`, a [`NotWritten`] as [`Agent::write`] says it): lamp-tasks
/// makes one at a time (`busy`), [`crate::made_skill::AGENT_BUSY_TRY`]; no
/// answer within [`WRITE_WITHIN`], [`crate::made_skill::AGENT_LATE_TRY`];
/// no agent after all ([`NotWritten::NoAgent`]),
/// [`crate::made_skill::AGENT_MISSING`], what is missing and never "try
/// again"; any other (`run_failed`, `no_answer`, an answer cut or unread),
/// [`crate::made_skill::AGENT_FAILED_TRY`]. Never the tip to ask for one
/// thing: the ask was not at fault.
pub fn not_made_try(why: &str) -> &'static str {
    if why == NotWritten::NoAgent.to_string() {
        crate::made_skill::AGENT_MISSING
    } else if why == NotWritten::Timeout.to_string() {
        crate::made_skill::AGENT_LATE_TRY
    } else if why
        == NotWritten::Failed(format!("lamp-tasks didn't make it ({BUSY_CODE})")).to_string()
    {
        crate::made_skill::AGENT_BUSY_TRY
    } else {
        crate::made_skill::AGENT_FAILED_TRY
    }
}

/// lamp-tasks' code for a make while another is made
/// (`lamp_tasks::skill_make`, one at a time).
const BUSY_CODE: &str = "busy";

impl Agent {
    /// The agent for a voice make, with the switch on and a tasks part;
    /// none otherwise.
    pub fn for_voice() -> Option<Self> {
        if !on() {
            return None;
        }
        let tasks = TASKS.get()?.clone();
        Some(Self {
            one_path: one_path_on(),
            ..Self::made_with(
                tasks,
                Some(PathBuf::from(crate::say::DEFAULT_SOCKET)),
                guide_on(),
            )
        })
    }

    /// The agent for a make typed in the app's MAKE IT
    /// (`crate::app_make`), with the switch on and a tasks part: the
    /// coordinator's own environment, where the app's makes run, its
    /// `LAMP_TASKS_DIR` the directory the second route's worker is given
    /// as `--tasks-dir`; none otherwise. The guide as [`Agent::for_voice`]
    /// has it. No waiting line: the app shows the ask as making.
    pub fn for_app() -> Option<Self> {
        let var = |name: &str| std::env::var(name).ok();
        Self::for_app_with(
            var(ENVIRONMENT).as_deref(),
            var(GUIDE_ENVIRONMENT).as_deref(),
            var(agent_tasks::ENVIRONMENT).as_deref(),
        )
    }

    /// [`Agent::for_app`] from the switch's value `switch`, the guide's
    /// `guide` and the tasks part's directory `tasks`, absolute only.
    pub fn for_app_with(
        switch: Option<&str>,
        guide: Option<&str>,
        tasks: Option<&str>,
    ) -> Option<Self> {
        if !on_with(switch) {
            return None;
        }
        let tasks = PathBuf::from(tasks?);
        if !tasks.is_absolute() {
            return None;
        }
        // No turn waits on the app's make: DeepSeek keeps its repairs when
        // the agent doesn't take it.
        Some(Self {
            repairs_if_untaken: true,
            ..Self::made_with(tasks, None, on_with(guide))
        })
    }

    /// One make's agent through the tasks part at `tasks`, its waiting line
    /// said at `say`, each answer given [`GUIDED_WRITE_WITHIN`] with the
    /// guide and [`WRITE_WITHIN`] without, within [`MAKE_WITHIN`].
    fn made_with(tasks: PathBuf, say: Option<PathBuf>, guide: bool) -> Self {
        Self {
            tasks,
            say,
            within: if guide {
                GUIDED_WRITE_WITHIN
            } else {
                WRITE_WITHIN
            },
            make_within: MAKE_WITHIN,
            guide,
            // The one run path is the voice's (#7245): `for_voice` sets it
            // from its switch.
            one_path: false,
            repairs_if_untaken: false,
        }
    }

    /// The most times an answer the voice's check refused goes back to the
    /// agent: [`GUIDED_MOST_REPAIRS`] with the guide, else
    /// `make_skill_local::MOST_REPAIRS`, as before.
    pub fn most_repairs(&self) -> usize {
        if self.guide {
            GUIDED_MOST_REPAIRS
        } else {
            crate::make_skill_local::MOST_REPAIRS
        }
    }

    /// Who the lamp's agent is, as the person calls it ("Hermes",
    /// "OpenClaw"), from the tasks part's targets; none without one.
    pub fn engine(&self) -> Option<&'static str> {
        let answer = agent_tasks::ask(
            &self.tasks.join("request.sock"),
            &json!({"op":"targets"}),
            Duration::from_millis(1_500),
        )?;
        agent_tasks::lamp_target_in(&answer)
    }

    /// The lamp's agent when it is Hermes, the one engine a make is handed
    /// to ([`MAKE_ENGINE`]); any other (OpenClaw has no namespace runner for
    /// a make) is journaled with its name and not handed over, so
    /// DeepSeek's answer stands. With [`Agent::one_path`], the lamp's agent
    /// when the one run path takes it ([`ONE_PATH_ENGINES`]).
    /// How long the make's answer is waited for: [`Agent::within`] and a
    /// grace; with the one run path, the hand-over's own [`Agent::within`]
    /// too, as lamp-tasks counts a gateway make's deadline from it (#7245).
    pub fn read_within(&self, grace: Duration) -> Duration {
        match self.one_path {
            true => self.within * 2 + grace,
            false => self.within + grace,
        }
    }

    pub fn hermes(&self) -> Option<&'static str> {
        let engine = self.engine()?;
        if engine == MAKE_ENGINE {
            return Some(engine);
        }
        if self.one_path && ONE_PATH_ENGINES.contains(&engine) {
            eprintln!(
                "lamp-voice: a skill made here: the lamp's agent is {engine}: handed over on the one run path"
            );
            return Some(engine);
        }
        eprintln!(
            "lamp-voice: a skill made here: the lamp's agent is {engine}: only Hermes has a namespace runner for a make today: not handed over, DeepSeek makes it alone"
        );
        None
    }

    /// Say [`WAITING_LINE`]: in the voice turn waiting on this thread's
    /// make, when one does ([`waiting_due`]); else through the say service
    /// on a thread of its own, nothing waiting on it.
    pub fn say_waiting(&self) {
        if waiting_due(MADE_FOR.with(std::cell::Cell::get)) {
            return;
        }
        let Some(socket) = self.say.clone() else {
            return;
        };
        let _ = std::thread::Builder::new()
            .name("lamp-voice-make-waiting".into())
            .spawn(move || {
                crate::app_make::say_waiting_at(&socket, WAITING_LINE, WAITING_WITHIN_MS);
            });
    }

    /// The agent's answer to `system` and `brief`, as it wrote it in its
    /// make run, within [`Agent::within`]. With `waiting`,
    /// [`WAITING_LINE`] is said once lamp-tasks says the run starts, never
    /// when it was refused at once.
    pub fn write(&self, system: &str, brief: &str, waiting: bool) -> Result<String, NotWritten> {
        let mut stream = UnixStream::connect(self.tasks.join(MAKE_SOCKET_NAME))
            .map_err(|_| NotWritten::NoMakeRun)?;
        // lamp-tasks ends the run at `within`; its answer comes just after.
        let grace = (self.within / 2).min(Duration::from_secs(5));
        let mut line = json!({
            "op": "skill_make",
            "rules": format!("{system}\n\n{brief}\n"),
            "within_ms": self.within.as_millis() as u64,
        });
        // The guide asked for only with its switch: off, the line is
        // today's, its three keys alone. With it, the check's switches as
        // this worker has them, so the guide's check is the voice's.
        if self.guide {
            line["guide"] = json!(true);
            line["switches"] = check_switches();
        }
        // The one run path asked for only with its switch: off, the line
        // is as above, byte for byte.
        if self.one_path {
            line["one_path"] = json!(true);
            // The lamp's agent as seen here, so lamp-tasks never makes an
            // OpenClaw lamp's skill as a Hermes make (#7245).
            if let Some(engine) = self.engine() {
                line["engine"] = json!(engine);
            }
        }
        stream
            .set_write_timeout(Some(SEND_WITHIN))
            .and_then(|()| stream.set_read_timeout(Some(self.read_within(grace))))
            .and_then(|()| writeln!(stream, "{line}"))
            .map_err(|error| NotWritten::Failed(format!("the make was not sent: {error}")))?;
        let mut lines = BufReader::new(std::io::Read::take(stream, 2 * MOST_ANSWER_BYTES as u64));
        let mut read = || -> Result<Value, NotWritten> {
            let mut text = String::new();
            match lines.read_line(&mut text) {
                Ok(0) => Err(NotWritten::Failed("lamp-tasks gave no answer".into())),
                Ok(_) => serde_json::from_str(&text)
                    .map_err(|_| NotWritten::Failed("lamp-tasks' answer can't be read".into())),
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    Err(NotWritten::Timeout)
                }
                Err(error) => Err(NotWritten::Failed(format!("lamp-tasks' answer: {error}"))),
            }
        };
        let mut answer = read()?;
        if answer == json!({"ok": true, "started": true}) {
            if waiting {
                self.say_waiting();
            }
            answer = read()?;
        }
        if answer.get("ok") == Some(&json!(true))
            && let Some(text) = answer.get("answer").and_then(Value::as_str)
        {
            return match text.len() > MOST_ANSWER_BYTES {
                true => Err(NotWritten::Failed("its answer was cut".into())),
                false => Ok(text.to_owned()),
            };
        }
        let code = code(&answer);
        if NOT_HELD_OFF_CODES.contains(&code.as_str()) {
            return Err(NotWritten::NoMakeRun);
        }
        // Harness or the gateway not ready yet (#7245): the lamp's own
        // short line, said to the person at once.
        if code == NOT_READY_CODE {
            return Err(NotWritten::Failed(crate::made_skill::NOT_READY_LINE.into()));
        }
        Err(NotWritten::Failed(format!(
            "lamp-tasks didn't make it ({code})"
        )))
    }
}

/// Whether a make of a skill that uses a linked service is the lamp's
/// agent's (the owner's ask, 13 Nov): with `LAMP_AGENT_SERVICES` on and a
/// made skill's code on, as the making model is then told to make such a
/// skill as one `do.task` for the agent (`make_skill_local::agent_services`).
pub fn services_through_agent() -> bool {
    crate::made_skill::agent_services_here() && crate::made_skill::scripts_on()
}

/// Words that ask for a linked service whether or not it is linked, each
/// as whole words in order, with the service's code: none of them is an
/// everyday word for something the lamp sees or does itself.
const SERVICE_WORDS: &[(&str, &[&str])] = &[
    ("gmail", &["mail"]),
    ("gmail", &["mails"]),
    ("gmail", &["email"]),
    ("gmail", &["emails"]),
    ("gmail", &["emailed"]),
    ("gmail", &["emailing"]),
    ("gmail", &["inbox"]),
    ("gmail", &["gmail"]),
    ("google_calendar", &["calendar"]),
    ("google_calendar", &["calendars"]),
    ("google_calendar", &["meeting"]),
    ("google_calendar", &["meetings"]),
    ("google_calendar", &["appointment"]),
    ("google_calendar", &["appointments"]),
    ("google_drive", &["google", "drive"]),
    ("google_drive", &["drive", "file"]),
    ("google_drive", &["drive", "files"]),
    ("google_drive", &["drive", "folder"]),
    ("github", &["github"]),
    ("github", &["pull", "request"]),
    ("github", &["pull", "requests"]),
];

/// Words that ask for a linked service only when the lamp is told it is
/// linked: a service's own name, or a word that is an everyday one too
/// ("drive", "issue", "slack", "notion").
const LINKED_WORDS: &[(&str, &[&str])] = &[
    ("google_calendar", &["event"]),
    ("google_calendar", &["events"]),
    ("google_calendar", &["agenda"]),
    ("google_drive", &["drive"]),
    ("github", &["issue"]),
    ("github", &["issues"]),
    ("github", &["repo"]),
    ("github", &["repository"]),
    ("figma", &["figma"]),
    ("linear", &["linear"]),
    ("asana", &["asana"]),
    ("ahrefs", &["ahrefs"]),
    ("facebook", &["facebook"]),
    ("slack", &["slack"]),
    ("mondaydotcom", &["monday", "com"]),
    ("hubspot", &["hubspot"]),
    ("amplitude", &["amplitude"]),
    ("notion", &["notion"]),
];

/// The code of the linked service `words` ask for, when they do: one of
/// [`SERVICE_WORDS`], or one of [`LINKED_WORDS`] for a service `linked`
/// (the codes `lamp-services` tells linked now, asked only when no word of
/// the first list is there) names. Whole words only, in any case: "mail"
/// in "voicemail" or "Monday" alone is none. None asks for none.
pub fn service_asked(
    words: &str,
    linked: impl FnOnce() -> Option<Vec<String>>,
) -> Option<&'static str> {
    let words: Vec<String> = words
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect();
    let said = |phrase: &[&str]| {
        words
            .windows(phrase.len())
            .any(|run| run.iter().zip(phrase).all(|(word, wanted)| word == wanted))
    };
    if let Some((code, _)) = SERVICE_WORDS.iter().find(|(_, phrase)| said(phrase)) {
        return Some(code);
    }
    let linked = linked()?;
    LINKED_WORDS
        .iter()
        .find(|(code, phrase)| linked.iter().any(|kept| kept == code) && said(phrase))
        .map(|(code, _)| *code)
}

/// The service `words` may mean, linked or not, by [`SERVICE_WORDS`] or
/// [`LINKED_WORDS`]: only a hint for the lamp's own model, which decides
/// by naming the grant (`LAMP_MAKE_NO_CANNOT`; the lead's rule, the first
/// builder on #7261: the bare word decides nothing). None names none.
pub fn service_hint(words: &str) -> Option<&'static str> {
    let words: Vec<String> = words
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect();
    let said = |phrase: &[&str]| {
        words
            .windows(phrase.len())
            .any(|run| run.iter().zip(phrase).all(|(word, wanted)| word == wanted))
    };
    let all = || SERVICE_WORDS.iter().chain(LINKED_WORDS);
    // A service's own name first ("Linear" over "issues"), then any word.
    all()
        .find(|(code, phrase)| code.starts_with(phrase[0]) && said(phrase))
        .or_else(|| all().find(|(_, phrase)| said(phrase)))
        .map(|(code, _)| *code)
}

/// lamp-tasks' code only, never the run's words.
fn code(answer: &Value) -> String {
    answer
        .get("error")
        .and_then(Value::as_str)
        .filter(|code| code.len() <= 40 && code.bytes().all(|b| b.is_ascii_graphic()))
        .unwrap_or("no code")
        .to_owned()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    /// A stub tasks part in a folder of its own: its `lamp` target runs
    /// `engine` (none for no target; "old" for a build with no make
    /// socket; "not_held_off" for one whose run can't be held off;
    /// "no_namespace" for one whose wrapper has no make-home line; "busy"
    /// for one making another; "run_failed" for a run that failed), and
    /// its make run answers each make's rules with `answer(rules)`, or
    /// never with `None`. The lines it was sent are kept.
    pub(crate) struct StubTasks {
        pub dir: Folder,
        pub sent: Arc<Mutex<Vec<Value>>>,
    }

    /// A folder of a test's own, short enough for a socket's path, removed
    /// when dropped.
    pub(crate) struct Folder(PathBuf);

    impl Folder {
        pub(crate) fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    impl StubTasks {
        pub(crate) fn agent(&self) -> Agent {
            Agent {
                tasks: self.dir.path().to_path_buf(),
                say: None,
                within: Duration::from_millis(1_500),
                make_within: Duration::from_secs(10),
                guide: false,
                one_path: false,
                repairs_if_untaken: false,
            }
        }

        /// [`StubTasks::agent`], each make asking for the guide.
        pub(crate) fn guided(&self) -> Agent {
            Agent {
                guide: true,
                ..self.agent()
            }
        }

        /// The makes handed over to the make run.
        pub(crate) fn hand_overs(&self) -> usize {
            self.sent
                .lock()
                .unwrap()
                .iter()
                .filter(|line| line["op"] == "skill_make")
                .count()
        }
    }

    type Answers = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

    pub(crate) fn stub(engine: Option<&'static str>, answers: Answers) -> StubTasks {
        static MADE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = Folder(PathBuf::from(format!(
            "/tmp/msa-{}-{}",
            std::process::id(),
            MADE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        )));
        let _ = std::fs::remove_dir_all(dir.path());
        std::fs::create_dir_all(dir.path()).unwrap();
        let sent = Arc::new(Mutex::new(Vec::new()));
        let mut sockets = vec!["request.sock"];
        if engine != Some("old") {
            sockets.push(MAKE_SOCKET_NAME);
        }
        for name in sockets {
            let listener = UnixListener::bind(dir.path().join(name)).unwrap();
            let (sent, answers) = (sent.clone(), answers.clone());
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else { return };
                    let mut line = String::new();
                    if BufReader::new(&stream).read_line(&mut line).is_err() {
                        continue;
                    }
                    let asked: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
                    sent.lock().unwrap().push(asked.clone());
                    let answer = match asked["op"].as_str().unwrap_or_default() {
                        "targets" => match engine {
                            Some(engine) => {
                                let engine = match engine {
                                    "old" | "not_held_off" | "no_namespace" | "busy"
                                    | "run_failed" | "not_ready" => "hermes",
                                    engine => engine,
                                };
                                json!({"ok":true,"targets":[{"alias":"lamp","engine":engine}]})
                            }
                            None => json!({"ok":true,"targets":[]}),
                        },
                        "skill_make"
                            if matches!(
                                engine,
                                Some(
                                    "not_held_off"
                                        | "no_namespace"
                                        | "busy"
                                        | "run_failed"
                                        | "not_ready"
                                )
                            ) =>
                        {
                            json!({"ok":false,"error":engine.unwrap(),"message":"no"})
                        }
                        "skill_make" => {
                            let _ = writeln!(stream, "{}", json!({"ok":true,"started":true}));
                            match answers(asked["rules"].as_str().unwrap()) {
                                Some(text) => json!({"ok":true,"answer":text}),
                                None => {
                                    // A run that never ends in the voice's time.
                                    std::thread::sleep(Duration::from_secs(5));
                                    continue;
                                }
                            }
                        }
                        _ => json!({"ok":false,"error":"bad_request"}),
                    };
                    let _ = writeln!(stream, "{answer}");
                }
            });
        }
        StubTasks { dir, sent }
    }

    #[test]
    fn the_agent_is_handed_the_rules_as_a_make_run_and_its_answer_read_back() {
        let tasks = stub(
            Some("hermes"),
            Arc::new(|rules: &str| {
                assert_eq!(rules, "SYSTEM\n\nBRIEF\n");
                Some("```skill.toml\nx\n```".to_owned())
            }),
        );
        let agent = tasks.agent();
        assert_eq!(agent.engine(), Some("Hermes"));
        assert_eq!(
            agent.write("SYSTEM", "BRIEF", false).unwrap(),
            "```skill.toml\nx\n```"
        );
        let sent = tasks.sent.lock().unwrap().clone();
        let make = sent.iter().find(|line| line["op"] == "skill_make").unwrap();
        // The make's line, as lamp-tasks' make run takes it, and
        // never a Harness task with the agent's own tools.
        assert_eq!(make["within_ms"], 1_500);
        assert_eq!(
            make.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["op", "rules", "within_ms"]
        );
        assert!(!sent.iter().any(|line| line["op"] == "prepare"));
    }

    /// Fourteen on lamp-9adf (12958, #6749): a make with code ran out at
    /// 45 s, 2 of 2, where `hermes chat -q "Say OK."` alone takes about
    /// 24.5 s, 13 s of it before Hermes's own session. A voice make is
    /// sent 90 s, within lamp-tasks' most, and the voice waits that and
    /// its grace; the agent's part keeps room for that one answer, and the
    /// second route's wait for the whole make stays past both parts.
    #[test]
    fn a_voice_make_is_given_time_for_the_agent_to_start_and_write_code() {
        assert_eq!(WRITE_WITHIN, Duration::from_secs(90));
        assert!(WRITE_WITHIN <= lamp_tasks::skill_make::MOST_WITHIN);
        assert!(WRITE_WITHIN <= MAKE_WITHIN);
        assert!(crate::make_skill_local::MAKE_WITHIN + MAKE_WITHIN <= Duration::from_secs(240));
        let tasks = stub(Some("hermes"), Arc::new(|_: &str| Some("x".to_owned())));
        let agent = Agent {
            within: WRITE_WITHIN,
            ..tasks.agent()
        };
        assert_eq!(agent.write("S", "B", false).unwrap(), "x");
        let sent = tasks.sent.lock().unwrap().clone();
        let make = sent.iter().find(|line| line["op"] == "skill_make").unwrap();
        assert_eq!(make["within_ms"], 90_000);
    }

    /// #6749: with a voice turn waiting on this thread's make, the waiting
    /// line goes to that turn once lamp-tasks says the run starts, never to
    /// the say service; told once. With none waiting, the say service is
    /// asked, as before.
    #[test]
    fn the_waiting_line_goes_to_the_turn_waiting_on_the_make() {
        let tasks = stub(Some("hermes"), Arc::new(|_: &str| Some("x".to_owned())));
        let dir = tasks.dir.path().to_path_buf();
        let say = dir.join("say.sock");
        let listener = UnixListener::bind(&say).unwrap();
        listener.set_nonblocking(true).unwrap();
        let agent = Agent {
            say: Some(say),
            ..tasks.agent()
        };
        let request = 6_749_100;
        let mut in_turn = waiting_in_turn(request);
        set_made_for(request);
        assert_eq!(agent.write("S", "B", true).unwrap(), "x");
        set_made_for(0);
        assert_eq!(in_turn.due.try_recv(), Ok(()));
        assert!(!waiting_due(request), "told once");
        std::thread::sleep(Duration::from_millis(100));
        assert!(listener.accept().is_err(), "the say service is not asked");
        drop(in_turn);
        // No turn waits: the say service is asked.
        assert_eq!(agent.write("S", "B", true).unwrap(), "x");
        let began = Instant::now();
        let asked = loop {
            if let Ok((stream, _)) = listener.accept() {
                stream.set_nonblocking(false).unwrap();
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                break serde_json::from_str::<Value>(&line).unwrap();
            }
            assert!(began.elapsed() < Duration::from_secs(5), "never asked");
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(asked["text"], WAITING_LINE);
    }

    /// The first builder's guard on #6643: no make run, no hand-over.
    /// An older lamp-tasks with no make socket, or one whose run can't be
    /// held off, is [`NotWritten::NoMakeRun`]; nothing is prepared.
    #[test]
    fn no_make_run_is_never_handed_over() {
        for engine in ["old", "not_held_off", "no_namespace"] {
            let tasks = stub(Some(engine), Arc::new(|_: &str| Some("x".into())));
            assert_eq!(tasks.agent().engine(), Some("Hermes"));
            assert_eq!(
                tasks.agent().write("S", "B", false),
                Err(NotWritten::NoMakeRun),
                "{engine}"
            );
            assert!(
                !tasks
                    .sent
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|line| line["op"] == "prepare")
            );
        }
    }

    #[test]
    fn no_lamp_target_is_no_agent_and_an_agent_that_never_answers_times_out() {
        let none = stub(None, Arc::new(|_: &str| None));
        assert_eq!(none.agent().engine(), None);
        assert_eq!(none.agent().hermes(), None);
        // OpenClaw is the lamp's agent, but a make is Hermes's alone.
        let openclaw = stub(Some("openclaw"), Arc::new(|_: &str| Some("A".into())));
        assert_eq!(openclaw.agent().engine(), Some("OpenClaw"));
        assert_eq!(openclaw.agent().hermes(), None);
        let hermes = stub(Some("hermes"), Arc::new(|_: &str| Some("A".into())));
        assert_eq!(hermes.agent().hermes(), Some("Hermes"));
        let silent = stub(Some("openclaw"), Arc::new(|_: &str| None));
        let began = Instant::now();
        assert_eq!(
            silent.agent().write("S", "B", false),
            Err(NotWritten::Timeout)
        );
        assert!(began.elapsed() < Duration::from_secs(4));
    }

    /// The audit's A3 (16 Nov): the agent busy, its run failed or no answer
    /// in time is the agent's side, and each is said as such, never as the
    /// tip to ask for one thing.
    #[test]
    fn the_agents_own_failures_are_each_said_as_the_agents() {
        use crate::made_skill::{
            AGENT_BUSY_TRY, AGENT_FAILED_TRY, AGENT_LATE_TRY, AGENT_MISSING, NOT_MADE_TRY,
        };
        let busy = stub(Some("busy"), Arc::new(|_: &str| Some("x".into())));
        let why = busy.agent().write("S", "B", false).unwrap_err().to_string();
        assert_eq!(not_made_try(&why), AGENT_BUSY_TRY, "{why}");
        let failed = stub(Some("run_failed"), Arc::new(|_: &str| Some("x".into())));
        let why = failed
            .agent()
            .write("S", "B", false)
            .unwrap_err()
            .to_string();
        assert_eq!(not_made_try(&why), AGENT_FAILED_TRY, "{why}");
        assert_eq!(
            not_made_try(&NotWritten::Timeout.to_string()),
            AGENT_LATE_TRY
        );
        // No agent after all: what is missing, never "try again" (El's P3
        // on #7243).
        assert_eq!(
            not_made_try(&NotWritten::NoAgent.to_string()),
            AGENT_MISSING
        );
        assert!(!AGENT_MISSING.contains("try again"), "{AGENT_MISSING}");
        for line in [
            AGENT_BUSY_TRY,
            AGENT_LATE_TRY,
            AGENT_FAILED_TRY,
            AGENT_MISSING,
        ] {
            assert_eq!(crate::made_skill::plain_reason(line), line);
            assert_eq!(crate::made_skill::not_made_line(line), line);
            assert_ne!(line, NOT_MADE_TRY);
        }
    }

    /// The owner's ask, 13 Nov: a make naming mail, a calendar, Drive or
    /// GitHub asks for a linked service, as whole words; a word that is an
    /// everyday one too counts only for a service told linked; a make
    /// naming none asks for none.
    #[test]
    fn a_linked_service_is_asked_for_in_whole_words() {
        let none = || -> Option<Vec<String>> { None };
        for (words, code) in [
            ("when I leave my desk, email me that I'm out", "gmail"),
            ("every morning at eight, read me my new mail", "gmail"),
            ("tell me when my inbox has something new", "gmail"),
            ("send an E-mail to Minh when I arrive", "gmail"),
            (
                "every evening, tell me tomorrow's calendar",
                "google_calendar",
            ),
            (
                "glow blue five minutes before each meeting",
                "google_calendar",
            ),
            ("list what's new in my Google Drive at noon", "google_drive"),
            ("say how many pull requests wait for me", "github"),
            ("read my GitHub notifications at nine", "github"),
        ] {
            assert_eq!(service_asked(words, none), Some(code), "{words}");
        }
        for words in [
            "make me a skill that dims the light at 10 pm",
            "play my voicemail jingle when I arrive",
            "nod when someone says emailer",
            "every Monday at nine, glow green",
            "when there's an issue with the air, say so",
            "glow red when I drive home",
        ] {
            assert_eq!(service_asked(words, none), None, "{words}");
        }
        let linked = || Some(vec!["github".to_owned(), "slack".to_owned()]);
        assert_eq!(
            service_asked("say my new issues at nine", linked),
            Some("github")
        );
        assert_eq!(
            service_asked("read my Slack messages when I sit down", linked),
            Some("slack")
        );
        assert_eq!(service_asked("what's on my drive at noon", linked), None);
        assert_eq!(
            service_asked("what's on my drive at noon", || Some(vec![
                "google_drive".to_owned()
            ])),
            Some("google_drive")
        );
        // What is linked is asked only when no plain word is there.
        assert_eq!(
            service_asked("email me", || -> Option<Vec<String>> {
                panic!("not asked")
            }),
            Some("gmail")
        );
    }

    /// The first builder on #7261 (the lead's rule): the bare word never
    /// routes a make to the agent for a service not linked, with
    /// `LAMP_MAKE_NO_CANNOT` on or off; it is only a hint for the lamp's own
    /// model ([`service_hint`]), linked or not.
    #[test]
    fn a_service_word_unlinked_routes_nothing_and_is_only_a_hint() {
        let issues = "when I sit down at my desk, tell me how many open issues I have on Linear";
        let timer = "make a linear timer that dims the light over ten minutes";
        let none = || Some(Vec::new());
        for on in [false, true] {
            crate::make_no_cannot::set_on_here(Some(on));
            assert_eq!(service_asked(issues, none), None);
            assert_eq!(service_asked(timer, none), None);
        }
        crate::make_no_cannot::set_on_here(None);
        assert_eq!(service_hint(issues), Some("linear"));
        assert_eq!(service_hint("say my new issues at nine"), Some("github"));
        assert_eq!(service_hint(timer), Some("linear"));
        assert_eq!(service_hint("glow green at nine"), None);
    }

    /// `LAMP_MAKE_AGENT_GUIDE`: with it each make's line asks lamp-tasks
    /// for the guide, the rules the same; without it the line is today's,
    /// its three keys alone (above).
    #[test]
    fn the_guide_is_asked_for_only_with_its_switch() {
        let tasks = stub(Some("hermes"), Arc::new(|_: &str| Some("A".into())));
        assert_eq!(tasks.agent().write("S", "B", false).unwrap(), "A");
        assert_eq!(tasks.guided().write("S", "B", false).unwrap(), "A");
        let sent = tasks.sent.lock().unwrap().clone();
        let makes: Vec<&Value> = sent
            .iter()
            .filter(|line| line["op"] == "skill_make")
            .collect();
        assert_eq!(
            makes[0].as_object().unwrap().keys().collect::<Vec<_>>(),
            ["op", "rules", "within_ms"]
        );
        assert_eq!(
            makes[1].as_object().unwrap().keys().collect::<Vec<_>>(),
            ["guide", "op", "rules", "switches", "within_ms"]
        );
        assert_eq!(makes[1]["guide"], true);
        assert_eq!(makes[1]["switches"], check_switches());
        assert_eq!(makes[0]["rules"], makes[1]["rules"]);
        assert_eq!(
            tasks.agent().most_repairs(),
            crate::make_skill_local::MOST_REPAIRS
        );
        assert_eq!(tasks.guided().most_repairs(), GUIDED_MOST_REPAIRS);
        // Within lamp-tasks' most and the agent's part.
        const { assert!(GUIDED_WRITE_WITHIN.as_secs() <= 120) };
        const { assert!(GUIDED_WRITE_WITHIN.as_secs() <= MAKE_WITHIN.as_secs()) };
    }

    /// Heron's P2 on #6768: the guided line's switches are this worker's
    /// values of exactly lamp-tasks' list, `on` only for an exact `on`.
    #[test]
    fn the_checks_switches_are_lamp_tasks_list_with_this_workers_values() {
        assert_eq!(CHECK_SWITCHES, lamp_tasks::skill_make::CHECK_SWITCHES);
        let given = |on: &'static [&'static str], value: &'static str| {
            move |name: &str| on.contains(&name).then(|| value.to_owned())
        };
        assert_eq!(
            check_switches_with(given(&[], "on")),
            json!({"LAMP_AGENT_SERVICES": "off", "LAMP_SKILL_WRITES": "off"})
        );
        assert_eq!(
            check_switches_with(given(&["LAMP_SKILL_WRITES"], "on")),
            json!({"LAMP_AGENT_SERVICES": "off", "LAMP_SKILL_WRITES": "on"})
        );
        assert_eq!(
            check_switches_with(given(&CHECK_SWITCHES, "on")),
            json!({"LAMP_AGENT_SERVICES": "on", "LAMP_SKILL_WRITES": "on"})
        );
        for not_on in ["1", "true", "ON", "on ", ""] {
            assert_eq!(
                check_switches_with(given(&CHECK_SWITCHES, not_on)),
                json!({"LAMP_AGENT_SERVICES": "off", "LAMP_SKILL_WRITES": "off"}),
                "{not_on:?}"
            );
        }
        // What lamp-tasks takes, whole.
        let line = json!({
            "op": "skill_make", "rules": "R", "within_ms": 2_000, "guide": true,
            "switches": check_switches_with(given(&CHECK_SWITCHES, "on")),
        });
        let make = lamp_tasks::skill_make::MakeLine::parse(line.to_string().as_bytes()).unwrap();
        assert_eq!(
            make.switches.unwrap().env(),
            [
                ("LAMP_AGENT_SERVICES", "on".to_owned()),
                ("LAMP_SKILL_WRITES", "on".to_owned())
            ]
        );
    }

    /// #7245, lamp-c0a4 (13333, OpenClaw, 11:46): with
    /// `LAMP_MAKE_AGENT_ONE_PATH` on, a make on an OpenClaw lamp is handed
    /// to OpenClaw, never "DeepSeek makes it alone", and its line asks
    /// lamp-tasks for the one run path.
    #[test]
    fn with_the_one_path_on_a_make_on_openclaw_is_handed_over_on_it() {
        let tasks = stub(Some("openclaw"), Arc::new(|_: &str| Some("x".to_owned())));
        let agent = Agent {
            one_path: true,
            ..tasks.agent()
        };
        assert_eq!(agent.hermes(), Some("OpenClaw"));
        assert_eq!(agent.write("S", "B", false).unwrap(), "x");
        let sent = tasks.sent.lock().unwrap().clone();
        let make = sent.iter().find(|line| line["op"] == "skill_make").unwrap();
        assert_eq!(make["one_path"], json!(true));
        let mut keys = make.as_object().unwrap().keys().collect::<Vec<_>>();
        keys.sort();
        assert_eq!(keys, ["engine", "one_path", "op", "rules", "within_ms"]);
        assert_eq!(make["engine"], json!("OpenClaw"));
    }

    /// #7245: the switch off, an OpenClaw lamp is as today: not handed
    /// over, and a line sent anyway has no `one_path`.
    #[test]
    fn with_the_one_path_off_a_make_on_openclaw_is_as_today() {
        let tasks = stub(Some("openclaw"), Arc::new(|_: &str| Some("x".to_owned())));
        let agent = tasks.agent();
        assert!(!agent.one_path);
        assert_eq!(agent.engine(), Some("OpenClaw"));
        assert_eq!(agent.hermes(), None);
        assert_eq!(tasks.hand_overs(), 0);
        assert_eq!(agent.write("S", "B", false).unwrap(), "x");
        let sent = tasks.sent.lock().unwrap().clone();
        let make = sent.iter().find(|line| line["op"] == "skill_make").unwrap();
        assert_eq!(
            make.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["op", "rules", "within_ms"]
        );
    }

    /// #7245: with the switch on, a Hermes lamp's make is handed over as
    /// before, its line carrying `one_path` too.
    #[test]
    fn with_the_one_path_on_a_make_on_hermes_carries_it() {
        let tasks = stub(Some("hermes"), Arc::new(|_: &str| Some("x".to_owned())));
        let agent = Agent {
            one_path: true,
            ..tasks.agent()
        };
        assert_eq!(agent.hermes(), Some("Hermes"));
        assert_eq!(agent.write("S", "B", false).unwrap(), "x");
        let sent = tasks.sent.lock().unwrap().clone();
        let make = sent.iter().find(|line| line["op"] == "skill_make").unwrap();
        assert_eq!(make["one_path"], json!(true));
    }

    #[test]
    fn the_one_path_switch_is_on_only_when_set_on() {
        assert_eq!(ONE_PATH_ENVIRONMENT, "LAMP_MAKE_AGENT_ONE_PATH");
        assert_eq!(ONE_PATH_ENGINES, ["Hermes", "OpenClaw"]);
    }

    /// The app's MAKE IT (the owner's ask, 13 Nov): its makes go to the
    /// agent only with the switch on and an absolute tasks part, the guide
    /// only with its own switch, as the voice's; no waiting line is said.
    #[test]
    fn the_apps_agent_is_had_only_with_its_switch_and_a_tasks_part() {
        let tasks = Some("/run/lampos-tasks");
        let agent = Agent::for_app_with(Some("on"), None, tasks).unwrap();
        assert_eq!(agent.tasks, PathBuf::from("/run/lampos-tasks"));
        assert_eq!(agent.say, None);
        assert!(!agent.guide);
        assert!(agent.repairs_if_untaken);
        assert_eq!(agent.within, WRITE_WITHIN);
        assert_eq!(agent.make_within, MAKE_WITHIN);
        let guided = Agent::for_app_with(Some("on"), Some("on"), tasks).unwrap();
        assert!(guided.guide);
        assert_eq!(guided.within, GUIDED_WRITE_WITHIN);
        assert_eq!(guided.most_repairs(), GUIDED_MOST_REPAIRS);
        for (switch, folder) in [
            (None, tasks),
            (Some("off"), tasks),
            (Some("ON"), tasks),
            (Some("on"), None),
            (Some("on"), Some("run/lampos-tasks")),
        ] {
            assert!(
                Agent::for_app_with(switch, Some("on"), folder).is_none(),
                "{switch:?} {folder:?}"
            );
        }
    }

    #[test]
    fn off_unless_set() {
        assert!(on_with(Some("on")));
        for off in [None, Some(""), Some("off"), Some("ON"), Some("1")] {
            assert!(!on_with(off), "{off:?}");
        }
    }
}
