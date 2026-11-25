//! The lamp's own agent spoken to directly: every turn of the lamp's
//! OpenClaw (`LAMP_TASKS_OPENCLAW_DIRECT`) or the lamp's Hermes
//! (`LAMP_TASKS_HERMES_DIRECT`) handed straight to that agent's gateway
//! through [`lamp_agent_gateway::AgentSession`], one connection and one kept
//! session, in place of the Harness CLI (`harness new/reply/ensure/wait/
//! show/close/ps`, `openclaw tui` or `hermes chat -q` in tmux: the stuck
//! sessions of #7906, #8001 and #6226) and, for OpenClaw, the `openclaw …
//! --gateway` CLI calls (`approvals get`, `allowlist add`, `gateway call
//! sessions.delete`). Each engine has its own switch, its own [`Direct`]
//! and its own pump; everything below is the same for both, except where
//! an engine is named. An engine whose switch is off keeps today's path.
//!
//! # The plan
//!
//! The seams, each one `if` on [`lamps`] (set only with a switch on and a
//! session built); with both switches off nothing is set and every line
//! below is skipped, so the Harness path runs as before:
//!
//! 1. **Spoken task.** `runtime::Production::Direct` (the production
//!    backend with these sessions beside Harness) asks
//!    [`Direct::spoken_launch`] before the one session's pane: a task for
//!    the session's engine on the lamp's own machine with no session named
//!    is queued here and answered `Joined` with an agent id of its own (a
//!    UUID per turn, so the store's checks hold and each task is followed
//!    by itself: no shared-pane caveat). The store then follows it as any
//!    joined task, through `Production::observe` and `Production::status`,
//!    which answer such ids from here ([`Direct::observe`],
//!    [`Direct::listed`]). An idle observation with the answer is
//!    what settles the task and fires the unasked news, as before.
//!    The tasks-harness worker's wait for OpenClaw (`gateway::launch_with`)
//!    is the same as Harness's, its probe this session's readiness in place
//!    of the port's (the `gateway::Probe` impl below); Hermes's is the same
//!    wait on its own session ([`not_ready_for`]), never OpenClaw's mark.
//! 2. **Skill `do.task` and make.** `skill_task::HarnessRunner::run` (both
//!    a skill's run on the one run path and a make, which reaches it through
//!    `skill_make::MakeTurn`, THE SEAM) calls [`skill_turn`] in place of its
//!    gateway wait and one-session pane: the run's folder, its skill-start
//!    grant (`gateway::open_start`, the fence) and its token stay exactly as
//!    they are; only the hand-over and the wait for the end are here. A
//!    skill's answer is kept only when its launcher claimed the start and
//!    the fence said it started the script (`skill_start::Live::ran`).
//!    Hermes is told to run the launcher with its `terminal` tool in the
//!    foreground ([`skill_words`]); no per-run token file, MCP token or
//!    `shell_init_files` reach it on this path, only the launcher's grant.
//! 3. **Readiness.** One pump thread per engine ([`Direct::start`])
//!    connects at lamp-tasks' start, opens or adopts the kept session and
//!    keeps it ready, with backoff (U1). A launch waits on that state
//!    (`gateway::Gateway`'s own wait for OpenClaw, `before_launch_until`;
//!    [`Direct::ready_by`] for Hermes), never on a port probe or a CLI
//!    read. Past [`GETTING_READY_AFTER`] not ready the getting-ready hook
//!    is told once (#8237's hook: not at this head, so the production hook
//!    only journals; TODO(#8237) below).
//! 4. **Allow-list.** Before each skill run is handed over, OpenClaw's
//!    agent `main`'s allow-list is read over the session and the
//!    skill-start entry put back when missing ([`DirectSession`]). Hermes
//!    has no exec allow-list: its approvals are answered by the same fence
//!    policy (allow once only the launcher, deny the rest), so its read
//!    always says allowed.
//! 5. **Progress.** Each run's [`lamp_agent_gateway::Progress`] is kept here
//!    and given as `harness ps`'s and `harness show`'s rows were
//!    ([`listed_of`], [`observation_of`]), so `progress::Tracker`,
//!    `works::Board` and `now` are fed with the same state enum and fields;
//!    the board's step comes from the run's tools ([`Direct::record_steps`],
//!    once per refresh of the work records).
//!    Before a "how far along" read is answered, the tracker is read from
//!    here at once ([`Direct::refresh`]): no `ps` and no wait.
//! 6. **Hygiene, V1's way.** One kept session, one turn at a time. A run
//!    with no event for [`STUCK_AFTER`] (V1's busyTTL) is stuck: aborted and
//!    ended failed, and the next turn goes on. A refused send, or [`BROKEN_SENDS`] failed in a row, is a
//!    broken session: [`AgentSession::reset`] and the turn tried once more.
//!    The context rotation is `reset()` too, at the engine's bounds
//!    ([`near_limit`]: 85% of the engine's context ([`context_tokens`]) or
//!    150 messages for a task, 60% or 100 for a make; for Hermes also V1's
//!    40 turns, `one_session::rotation_due`, since Hermes compacts on its
//!    own and counts after it) on the gateway's own size
//!    of the session ([`DirectSession::session_size`], read before the send
//!    when not read since the last run), else on the words handed over and
//!    read. A gateway down is reconnected to for ever;
//!    a queued turn waits [`QUEUED_MOST`] of it, then ends not ready.
//!    A gateway drop under a run ends it, and every turn queued behind it,
//!    with the shared [`lamp_agent_gateway::DROPPED`] words; nothing is
//!    run again.
//! 7. **The kept session across restarts (Hermes).** Hermes's kept
//!    session's id is lamp-tasks' own (`lamp-main-<ms>`): the session keeps
//!    it in [`KEPT_FILE`], in lamp-tasks' state folder (0600), with its turns
//!    and counted tokens, so a restart of lamp-tasks, of the gateway or of
//!    the lamp keeps the conversation, and the rotation's backstop counts
//!    ([`DirectSession::rotation_counts`]) go on where they were. A
//!    rotation names a new id. OpenClaw's kept session is its gateway's
//!    fixed key (`agent:main:lamp`).
//! 8. **The agent switch.** The lamp runs one agent's gateway at a time,
//!    its default's (lamp-update stops the other before it starts the
//!    default's), so each engine's session takes work only while its engine
//!    is the lamp's default ([`serves_now`], OpenClaw as Hermes). When the
//!    default changes (the app's `app_engine_default`, or any other change
//!    of the lamp's order, [`switch_check`]) the old agent's turns, queued
//!    or running, are ended with [`SWITCHED`], told as a stopped task, and
//!    the new default's session looks at its gateway at once and every
//!    [`RETRY_WAITING_MOST`] until it answers. Until then the engines'
//!    default read says `"gateway":"switching"`, then `"ready"`
//!    ([`gateway_said`]).
use crate::engine::{MAX_ANSWER_BYTES, MAX_ASK_BYTES, Watched};
use crate::harness::{EngineKind, Launch, LaunchOutcome, Listed, Observation};
use lamp_agent_gateway::{AgentSession, Error, Outcome, Phase, Progress, RunId, Turn, TurnKind};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant, SystemTime};

/// OpenClaw's switch, in lamp-tasks' own environment: `on` exactly ([`on`]).
pub const SWITCH: &str = "LAMP_TASKS_OPENCLAW_DIRECT";
/// Hermes's switch, in lamp-tasks' own environment: `on` exactly ([`on`]).
/// Set in no unit yet: off until a Hermes lamp's row passes.
pub const HERMES_SWITCH: &str = "LAMP_TASKS_HERMES_DIRECT";

/// A switch's value: `on` exactly; anything else (unset too) is off.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// The engines with a direct path, and each one's switch.
pub const ENGINES: [(EngineKind, &str); 2] = [
    (EngineKind::Openclaw, SWITCH),
    (EngineKind::Hermes, HERMES_SWITCH),
];

/// The lamp's direct OpenClaw session: set once at `serve`'s start, only
/// with [`SWITCH`] on and a session built ([`lamp_session`]). Never set in
/// the crate's tests, which make their own.
pub static LAMP: OnceLock<Arc<Direct>> = OnceLock::new();
/// The lamp's direct Hermes session, the same with [`HERMES_SWITCH`].
pub static HERMES_LAMP: OnceLock<Arc<Direct>> = OnceLock::new();

/// Where `engine`'s direct session is set; `None` for an engine with no
/// direct path.
pub fn slot(engine: EngineKind) -> Option<&'static OnceLock<Arc<Direct>>> {
    match engine {
        EngineKind::Openclaw => Some(&LAMP),
        EngineKind::Hermes => Some(&HERMES_LAMP),
        _ => None,
    }
}

/// lamp-tasks' engine for a gateway's.
pub fn engine_kind(engine: lamp_agent_gateway::Engine) -> EngineKind {
    match engine {
        lamp_agent_gateway::Engine::Openclaw => EngineKind::Openclaw,
        lamp_agent_gateway::Engine::Hermes => EngineKind::Hermes,
    }
}

/// An engine's name as the journal says it.
pub fn label(engine: EngineKind) -> &'static str {
    match engine {
        EngineKind::Openclaw => "OpenClaw",
        EngineKind::Hermes => "Hermes",
        _ => "the agent",
    }
}

/// A session lamp-tasks can hand turns to, with what the engine-neutral
/// [`AgentSession`] lacks: agent `main`'s exec allow-list over the
/// session's own socket (OpenClaw's `exec.approvals.get` / `set`, in place
/// of the CLI's `approvals get` and `approvals allowlist add`), the kept
/// conversation's size, and its turns and tokens when the session counts
/// them.
pub trait DirectSession: AgentSession {
    /// Whether agent `main` (or `*`) may exec the skill-start launcher.
    fn skill_start_allowed(&mut self) -> Result<bool, Error>;
    /// Puts the skill-start launcher's entry on agent `main`'s allow-list.
    fn allow_skill_start(&mut self) -> Result<(), Error>;
    /// The kept conversation's size as the gateway counts it: its tokens
    /// (as `bytes`, 4 a token, for `one_session::near_context_limit`) and
    /// messages, read now when not read since the last run; `None` when it
    /// can't be read, and lamp-tasks judges from the words handed over.
    fn session_size(&mut self) -> Option<crate::harness::SessionSize>;
    /// The kept session's turns handed over and tokens counted since it
    /// began, as the session keeps them across restarts ([`KEPT_FILE`],
    /// Hermes), for V1's rotation backstop ([`near_limit`]); `None` when it
    /// keeps none, and lamp-tasks counts since its own start.
    fn rotation_counts(&self) -> Option<(u64, u64)> {
        None
    }
}

/// One approvals call's bound over the session.
const APPROVALS_WITHIN: Duration = Duration::from_secs(10);

impl DirectSession for lamp_agent_gateway::openclaw::Session {
    fn skill_start_allowed(&mut self) -> Result<bool, Error> {
        let read = self.approvals_get(APPROVALS_WITHIN)?;
        // Not such a read: never blocks the run, as the CLI read's was.
        Ok(crate::gateway::allows_skill_start(&read.to_string()).unwrap_or(true))
    }
    fn allow_skill_start(&mut self) -> Result<(), Error> {
        self.allowlist_add(crate::skill_start::LAUNCHER, APPROVALS_WITHIN)
            .map(|_| ())
    }
    fn session_size(&mut self) -> Option<crate::harness::SessionSize> {
        // The larger of `totalTokens` and `estimatedPromptTokens`, and
        // `totalMessages`, from `chat.history`.
        let (tokens, messages) = self.size_now()?;
        Some(crate::harness::SessionSize {
            messages,
            bytes: tokens.saturating_mul(4),
        })
    }
}

/// Hermes has no exec allow-list: a command that asks is answered by the
/// session's approval policy (allow once only the launcher, deny the rest,
/// never left waiting), so the launcher is always "allowed" here and the
/// fence guards the run's grant.
impl DirectSession for lamp_agent_gateway::hermes::Session {
    fn skill_start_allowed(&mut self) -> Result<bool, Error> {
        Ok(true)
    }
    fn allow_skill_start(&mut self) -> Result<(), Error> {
        Ok(())
    }
    fn session_size(&mut self) -> Option<crate::harness::SessionSize> {
        // Tokens and messages of the kept session, as its gateway counts
        // them (after Hermes's own compaction).
        let (tokens, messages) = self.size_now()?;
        Some(crate::harness::SessionSize {
            messages,
            bytes: tokens.saturating_mul(4),
        })
    }
    fn rotation_counts(&self) -> Option<(u64, u64)> {
        // Its runs handed over, and the sum of its ended runs'
        // `usage.input_tokens`, both kept in its file with its id.
        Some((self.turns(), self.run_tokens()))
    }
}

/// The file in lamp-tasks' state folder where Hermes's session keeps its
/// kept session's id, turns and tokens across restarts, 0600
/// (`lamp_agent_gateway::hermes::Config::state_path`, its default).
pub const KEPT_FILE: &str = lamp_agent_gateway::hermes::STATE_PATH;

/// Hermes's gateway's API key, as lamp-update makes it (`hermes_gateway::KEY`),
/// read by lamp-tasks at each check.
pub const HERMES_KEY_PATH: &str = "/var/lib/lampos-hermes-gateway/api-key";

/// The lamp's real session for `engine`:
/// - OpenClaw: its gateway on loopback ([`crate::gateway::PORT`]), its
///   token from the product's file, the one kept session `agent:main:lamp`,
///   each task its own run (`followup`);
/// - Hermes: its gateway's API server on loopback, its key from
///   [`HERMES_KEY_PATH`], its kept session kept in [`KEPT_FILE`] (none kept:
///   the session names a fresh one);
///
/// approvals answered by the skill-start fence's policy
/// ([`crate::skill_start::LAUNCHER`]). `None` for any other engine.
pub fn lamp_session(engine: EngineKind) -> Option<Box<dyn DirectSession>> {
    match engine {
        EngineKind::Openclaw => {
            let cfg = lamp_agent_gateway::openclaw::Config {
                url: format!("ws://127.0.0.1:{}", crate::gateway::PORT),
                fence: crate::skill_start::LAUNCHER.into(),
                ..lamp_agent_gateway::openclaw::Config::default()
            };
            Some(Box::new(lamp_agent_gateway::openclaw::Session::new(cfg)))
        }
        EngineKind::Hermes => {
            let cfg = lamp_agent_gateway::hermes::Config {
                fence: crate::skill_start::LAUNCHER.into(),
                key_path: HERMES_KEY_PATH.into(),
                ..lamp_agent_gateway::hermes::Config::default()
            };
            Some(Box::new(lamp_agent_gateway::hermes::Session::new(cfg)))
        }
        _ => None,
    }
}

/// Not ready this long while something waits, the getting-ready hook is
/// told once (#8237's 5 s).
pub const GETTING_READY_AFTER: Duration = Duration::from_secs(5);
/// One readiness look's bound on the pump.
pub const READY_LOOK: Duration = Duration::from_secs(2);
/// One wait for a run's end on the pump, between progress reads.
pub const WAIT_SLICE: Duration = Duration::from_millis(250);
/// The first pause after a readiness look failed, doubled at each failure.
pub const RETRY_FIRST: Duration = Duration::from_millis(500);
/// The longest pause between readiness looks with nothing waiting...
pub const RETRY_MOST: Duration = Duration::from_secs(30);
/// ...and with a turn waiting, a run in flight, or the session's engine the
/// lamp's default ([`retry_most`]).
pub const RETRY_WAITING_MOST: Duration = Duration::from_secs(2);

/// The longest pause between two readiness looks of a session not ready:
/// [`RETRY_WAITING_MOST`] while something waits on it (`waited_on`: a turn
/// queued, a launch waiting for readiness, a run whose end is owed) or
/// while its engine is the lamp's default (`default`), so the lamp's own
/// agent is ready within about 2 s of its gateway coming up (after an agent
/// switch, a boot or a gateway restart), whatever asks first; else
/// [`RETRY_MOST`]: the other agent's gateway is not run by lamp-update, and
/// looking at it every 2 s would only spend the lamp's CPU.
pub fn retry_most(waited_on: bool, default: bool) -> Duration {
    if waited_on || default {
        RETRY_WAITING_MOST
    } else {
        RETRY_MOST
    }
}
/// While ready and idle, readiness is looked at again this often, so a
/// gateway gone between tasks is known before the next ask.
pub const IDLE_READY_EVERY: Duration = Duration::from_secs(30);
/// The most characters of a progress line (`about`), as `now`'s step.
pub const ABOUT_MOST: usize = crate::now::MOST_STEP_CHARS;
/// Ended runs kept for the store's reads, the oldest dropped past it.
const ENDED_KEPT: usize = 64;
/// A run with no event from the gateway this long is checked for stuck
/// (V1's busyTTL, 5 min).
pub const STUCK_AFTER: Duration = Duration::from_secs(5 * 60);
/// How many times [`STUCK_AFTER`] a run waits when its last event is a
/// tool's start with no result yet: a build or a `sleep` sends nothing
/// between the two, so a legit long tool is not ended at the base limit.
pub const TOOL_QUIET_TIMES: u32 = 6;
/// Sets the stuck limit in seconds (10 or more) at lamp-tasks' start,
/// for tests on a lamp; unset or otherwise, [`STUCK_AFTER`].
pub const STUCK_SWITCH: &str = "LAMP_TASKS_DIRECT_STUCK_S";
/// The least [`STUCK_SWITCH`] takes.
pub const STUCK_LEAST_S: u64 = 10;

/// The stuck limit [`STUCK_SWITCH`] sets: whole seconds, at least
/// [`STUCK_LEAST_S`]; anything else, none.
pub fn stuck_after_of(value: Option<&str>) -> Option<Duration> {
    let seconds: u64 = value?.trim().parse().ok()?;
    (seconds >= STUCK_LEAST_S).then(|| Duration::from_secs(seconds))
}

/// How long `progress`'s run may be quiet before it is checked for stuck:
/// [`TOOL_QUIET_TIMES`] × `stuck_after` while its last event is a tool's
/// start with no result, else `stuck_after`.
pub fn quiet_limit(stuck_after: Duration, progress: &Progress) -> Duration {
    if progress.last_tool.as_ref().is_some_and(|tool| tool.running) {
        stuck_after.saturating_mul(TOOL_QUIET_TIMES)
    } else {
        stuck_after
    }
}

/// Whether `progress`'s run, at `now`, has been quiet past its
/// [`quiet_limit`].
pub fn quiet_past(stuck_after: Duration, progress: &Progress, now: SystemTime) -> bool {
    now.duration_since(progress.updated_at)
        .is_ok_and(|quiet| quiet >= quiet_limit(stuck_after, progress))
}

/// What a stuck run ends with, said to the person as its failure.
pub const STUCK_WHY: &str = "the agent stopped responding, so the task was stopped";
/// The longest a queued turn waits for a gateway that is not ready.
pub const QUEUED_MOST: Duration = Duration::from_secs(2 * 60);
/// A queued turn that waited [`QUEUED_MOST`] ends with this.
pub const QUEUED_NOT_READY: &str =
    "the agent's gateway was not ready in time (openclaw_gateway_not_ready)";
/// What a turn of the old agent ends with when the lamp's default agent is
/// switched ([`switch_check`]): the owner's rule is that the switch stops
/// the old agent's work rather than letting it run on a gateway lamp-update
/// is stopping. Carried as the observation's answer, as the drop's words
/// are ([`lamp_agent_gateway::DROPPED`]), so the voice's news of a stopped
/// task says it; a skill run fails with it.
pub const SWITCHED: &str =
    "This task ended because the lamp switched to another agent. Please ask again.";
/// The code a task for an agent that is not the lamp's default is refused
/// with at once ([`not_default_for`]): its gateway is not run, so waiting
/// for it would only end in "not ready" minutes later.
pub const NOT_DEFAULT: &str = "agent_not_lamp_default";
/// Sends failed in a row that make the session broken: reset, then the
/// waiting turn tried once more.
pub const BROKEN_SENDS: u32 = 2;
/// Sends of one turn before it ends failed.
pub const SEND_TRIES_MOST: u32 = 4;

/// The context of the lamp's Hermes model (`hermes_config::MODEL`, the
/// voice's DeepSeek through lamp-cloud's forwarder), in tokens. Not read
/// from the model: taken as OpenClaw's 128k, the smaller of what that
/// model is said to take, so a rotation comes early rather than late.
pub const HERMES_CONTEXT_TOKENS: u64 = 128_000;

/// `engine`'s model context, in tokens, for [`near_limit`].
pub fn context_tokens(engine: EngineKind) -> u64 {
    match engine {
        EngineKind::Hermes => HERMES_CONTEXT_TOKENS,
        _ => crate::one_session::CONTEXT_TOKENS,
    }
}

/// Whether `engine`'s kept session is due a fresh one before the next turn
/// (a make's when `make`): its size (`size`, the gateway's own; `None`, the
/// estimate `tokens` of the words handed over and read alone) near the
/// engine's context (`one_session::near_context_limit_in` at
/// [`context_tokens`]), or, for an engine that compacts on its own and
/// reports after it (Hermes), V1's turn backstop (`one_session::rotation_due`,
/// 40 turns) on `counts`' turns; its tokens are not a bound here. OpenClaw's bounds are exactly those of the one session.
pub fn near_limit(
    engine: EngineKind,
    size: Option<crate::harness::SessionSize>,
    tokens: u64,
    counts: (u64, u64),
    make: bool,
) -> bool {
    // Only the turn count of V1's backstop: a turn's tokens count every
    // model call's prompt in it (a web search turn passed 250k alone on
    // ee17, 24 Nov), so the context itself is judged by `size`, as
    // OpenClaw's is (owner: Hermes behaves as OpenClaw).
    let (turns, _counted) = counts;
    crate::one_session::near_context_limit_in(context_tokens(engine), size, tokens, make)
        || crate::one_session::rotation_due(engine, turns, 0)
}

/// The most a Hermes foreground command may run (Hermes's own
/// `FOREGROUND_MAX_TIMEOUT`, 600 s): above it the terminal tool backgrounds
/// it, and the run would end before its script.
pub const HERMES_FOREGROUND_MOST_S: u64 = 600;

/// A skill run's words for `engine` (`prompt`, the run's
/// `gateway::open_start` prompt, which names the skill-start launcher by
/// its full path, [`crate::skill_start::LAUNCHER`], and the run's id):
/// Hermes is told to run it with its `terminal` tool in the foreground with
/// a timeout as long as the run's `deadline` (at most
/// [`HERMES_FOREGROUND_MOST_S`]), so the launcher's end is the run's.
/// The launcher keeps its full path, as for OpenClaw: Hermes's `terminal`
/// takes its `PATH` from a login shell's snapshot (`/etc/profile`), not
/// from the gateway's, so a bare `skill-start` is not found there (ee17,
/// 24 Nov 02:51: `skill-start: command not found`, exit 127).
/// OpenClaw's are `prompt` as it is.
pub fn skill_words(engine: EngineKind, prompt: &str, deadline: Duration) -> String {
    if engine != EngineKind::Hermes {
        return prompt.to_owned();
    }
    let timeout = deadline.as_secs().clamp(1, HERMES_FOREGROUND_MOST_S);
    format!(
        "{prompt}\nRun it with your `terminal` tool, in the foreground (`background` false), with `timeout` {timeout}, and wait for it to end."
    )
}

/// How a run ended, as lamp-tasks keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum End {
    Answered(String),
    Failed(String),
    Aborted,
}

#[derive(Clone, Debug)]
struct Run {
    /// The engine whose session it is handed to.
    engine: EngineKind,
    /// The store's launch name, or the skill run's id: the turn's id.
    name: String,
    /// Its own agent id (a UUID), as the store follows it.
    agent: String,
    kind: TurnKind,
    message: String,
    /// The task's own words, for the observation's `ask`.
    asked: String,
    queued_at: SystemTime,
    queued_since: Instant,
    /// Sends tried, and whether a reset was spent on it.
    tries: u32,
    reset_spent: bool,
    run: Option<RunId>,
    progress: Option<Progress>,
    end: Option<End>,
}

impl Run {
    fn in_flight(&self) -> bool {
        self.run.is_some() && self.end.is_none()
    }
    fn queued(&self) -> bool {
        self.run.is_none() && self.end.is_none()
    }
}

#[derive(Default)]
struct Inner {
    runs: Vec<Run>,
    ready: bool,
    not_ready_since: Option<Instant>,
    not_ready_why: String,
    told_getting_ready: bool,
    /// Waiters on readiness now (`Direct::wait_ready`).
    waiting_ready: u32,
    /// Estimated tokens in the kept conversation (4 characters a token).
    tokens: u64,
    /// Turns handed over since lamp-tasks' start, for the journal.
    turns: u64,
    /// Turns handed into the kept conversation since lamp-tasks' start or
    /// its last reset: [`near_limit`]'s count for a session that keeps none.
    session_turns: u64,
    resets: u64,
    /// Sends failed in a row ([`BROKEN_SENDS`]).
    failed_sends: u32,
    /// Set when the lamp's default agent was switched to this session's
    /// engine ([`Direct::look_again_for_switch`]), until its gateway next
    /// answers ready: the switch is under way ([`gateway_said`]).
    switch_since: Option<Instant>,
}

impl Inner {
    fn find(&self, agent: &str) -> Option<&Run> {
        self.runs.iter().find(|run| run.agent == agent)
    }
    fn find_mut(&mut self, agent: &str) -> Option<&mut Run> {
        self.runs.iter_mut().find(|run| run.agent == agent)
    }
    fn waiting(&self) -> bool {
        self.waiting_ready > 0 || self.runs.iter().any(Run::queued)
    }
    fn prune(&mut self) {
        while self.runs.iter().filter(|run| run.end.is_some()).count() > ENDED_KEPT {
            let Some(at) = self.runs.iter().position(|run| run.end.is_some()) else {
                break;
            };
            self.runs.remove(at);
        }
    }
}

/// Told once a not-ready stretch passes [`GETTING_READY_AFTER`] while
/// something waits, with why in plain words.
pub type GettingReady = Box<dyn Fn(&str) + Send + Sync>;

/// Reads the lamp's own machine id.
type MachineOf = Box<dyn Fn() -> Option<String> + Send + Sync>;

/// The lamp's one direct session for one engine, and its turns.
pub struct Direct {
    /// The session's engine ([`AgentSession::engine`]).
    engine: EngineKind,
    session: Mutex<Box<dyn DirectSession>>,
    inner: Mutex<Inner>,
    changed: Condvar,
    /// The watched rows last given the tracker ([`Self::refresh_if_changed`]).
    last_rows: Mutex<String>,
    getting_ready: GettingReady,
    getting_ready_after: Duration,
    ready_look: Duration,
    wait_slice: Duration,
    retry_first: Duration,
    stuck_after: Duration,
    queued_most: Duration,
    /// The lamp's own machine id, read at each spoken launch.
    lamp_machine: Mutex<Option<MachineOf>>,
    stop: Arc<AtomicBool>,
}

impl Direct {
    pub fn new(session: Box<dyn DirectSession>, stop: Arc<AtomicBool>) -> Self {
        Self {
            engine: engine_kind(session.engine()),
            session: Mutex::new(session),
            inner: Mutex::new(Inner::default()),
            changed: Condvar::new(),
            last_rows: Mutex::new(String::new()),
            getting_ready: Box::new(|why| {
                // TODO(#8237): its `tell_getting_ready` is not at this
                // head; once it is, call it here.
                eprintln!("tasks: direct: still getting ready: {why}");
            }),
            getting_ready_after: GETTING_READY_AFTER,
            ready_look: READY_LOOK,
            wait_slice: WAIT_SLICE,
            retry_first: RETRY_FIRST,
            stuck_after: STUCK_AFTER,
            queued_most: QUEUED_MOST,
            lamp_machine: Mutex::new(None),
            stop,
        }
    }

    /// The engine this session speaks to.
    pub fn engine(&self) -> EngineKind {
        self.engine
    }

    /// With `hook` told once a not-ready stretch passes `after`.
    pub fn with_getting_ready(mut self, after: Duration, hook: GettingReady) -> Self {
        self.getting_ready_after = after;
        self.getting_ready = hook;
        self
    }

    /// With the pump's readiness look, wait slice and first retry bounds.
    pub fn with_bounds(
        mut self,
        ready_look: Duration,
        wait_slice: Duration,
        retry_first: Duration,
    ) -> Self {
        self.ready_look = ready_look;
        self.wait_slice = wait_slice;
        self.retry_first = retry_first;
        self
    }

    /// With a run quiet `stuck_after` checked for stuck, and a queued turn
    /// failed after `queued_most` of a gateway not ready.
    pub fn with_limits(mut self, stuck_after: Duration, queued_most: Duration) -> Self {
        self.stuck_after = stuck_after;
        self.queued_most = queued_most;
        self
    }

    /// With a run checked for stuck after `stuck_after` quiet (a running
    /// tool, [`TOOL_QUIET_TIMES`] that).
    pub fn with_stuck_after(mut self, stuck_after: Duration) -> Self {
        self.stuck_after = stuck_after;
        self
    }

    /// The lamp's own machine, read by `machine` at each spoken launch.
    pub fn set_lamp_machine(&self, machine: Box<dyn Fn() -> Option<String> + Send + Sync>) {
        *self
            .lamp_machine
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(machine);
    }

    fn lamp_machine(&self) -> Option<String> {
        self.lamp_machine
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .and_then(|machine| machine())
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn session(&self) -> MutexGuard<'_, Box<dyn DirectSession>> {
        self.session.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The pump started on its own thread (U1): it connects at once.
    pub fn start(self: &Arc<Self>) -> std::io::Result<std::thread::JoinHandle<()>> {
        let me = self.clone();
        std::thread::Builder::new()
            .name(format!("{}-direct", self.engine.argument()))
            .spawn(move || me.pump())
    }

    /// Whether the session is ready now, as the pump last found it.
    pub fn is_ready(&self) -> bool {
        self.lock().ready
    }

    /// How many fresh conversations the session was given ([`AgentSession::reset`]).
    pub fn resets(&self) -> u64 {
        self.lock().resets
    }

    /// [`lamp_path`] for this session: a task of its engine on the lamp's
    /// machine while it serves ([`serves_now`]), or one of its own turns
    /// that has ended, so the end an agent switch gave it ([`SWITCHED`]) is
    /// read from here, never asked of Harness, which never had it. A turn
    /// still running does not keep the path when the default changes under
    /// it (#8431 kept it; the owner's switch rule ends it instead,
    /// [`switch_check`]).
    fn path(&self, engine: &str, machine: &str, agent: &str) -> bool {
        engine == self.engine.bus_name()
            && (serves_now(self.engine) || self.ended(agent))
            && self.lamp_machine().as_deref() == Some(machine)
    }

    /// Whether `agent` is one of this session's turns and has ended.
    fn ended(&self, agent: &str) -> bool {
        self.lock().find(agent).is_some_and(|run| run.end.is_some())
    }

    /// Whether an agent switch to this session's engine is under way: its
    /// gateway has not answered ready since ([`gateway_said`]).
    pub fn switching(&self) -> bool {
        self.lock().switch_since.is_some()
    }

    /// The lamp's default agent was switched to this session's engine: its
    /// gateway is looked at again at once (the pump woken, the last "ready"
    /// it found no longer trusted: lamp-update is starting it now), and
    /// the switch is under way until it answers ([`Self::became_ready`]).
    fn look_again_for_switch(&self) {
        let mut inner = self.lock();
        inner.ready = false;
        inner.not_ready_since = Some(Instant::now());
        inner.told_getting_ready = false;
        inner.not_ready_why = format!(
            "{}'s gateway is starting after the agent switch",
            label(self.engine)
        );
        inner.switch_since = Some(Instant::now());
        drop(inner);
        self.changed.notify_all();
    }

    /// The lamp's default agent was switched away from this session's
    /// engine (to `to`): every turn not yet ended, queued or running, ends
    /// at once with [`SWITCHED`] (told as a stopped task); a running one is
    /// also aborted on its gateway, best effort, on a thread of its own so
    /// the caller (lamp-tasks' main loop) never waits behind the session.
    /// The number of turns ended.
    fn end_for_switch(self: &Arc<Self>, to: Option<EngineKind>) -> usize {
        let mut inner = self.lock();
        inner.switch_since = None;
        let mut aborts = Vec::new();
        let mut names = Vec::new();
        for run in inner.runs.iter_mut().filter(|run| run.end.is_none()) {
            if let Some(id) = run.run.clone() {
                aborts.push(id);
            }
            run.end = Some(End::Failed(SWITCHED.into()));
            names.push(run.name.clone());
        }
        drop(inner);
        self.changed.notify_all();
        for name in &names {
            eprintln!(
                "tasks: {name}: ended: the lamp's default agent was switched from {} to {}",
                label(self.engine),
                to.map_or("none", label)
            );
        }
        if !aborts.is_empty() {
            let me = self.clone();
            let spawned = std::thread::Builder::new()
                .name(format!("{}-switch", self.engine.argument()))
                .spawn(move || {
                    for run in aborts {
                        if let Err(error) = me.session().abort(&run) {
                            eprintln!(
                                "tasks: direct: run {} not aborted at the agent switch: {error}",
                                run.0
                            );
                        }
                    }
                });
            if let Err(error) = spawned {
                eprintln!("tasks: direct: the switch's aborts not started: {error}");
            }
        }
        names.len()
    }

    /// The agents of this session's turns whose run has ended (answered,
    /// failed or stopped), for the store's read at once
    /// (`Engine::push_direct_ended`).
    pub fn ended_agents(&self) -> Vec<String> {
        self.lock()
            .runs
            .iter()
            .filter(|run| run.end.is_some())
            .map(|run| run.agent.clone())
            .collect()
    }

    /// Whether `agent` is one of this session's turns.
    pub fn owns(&self, agent: &str) -> bool {
        self.lock().find(agent).is_some()
    }

    /// A turn with agent id `agent`, answered `answer`, for the crate's
    /// tests of the store.
    #[cfg(test)]
    pub(crate) fn answered_for_tests(&self, agent: &str, answer: &str) {
        self.ended_for_tests(agent, End::Answered(answer.to_owned()));
    }

    /// A turn of `agent` ended `end`, for tests of the store.
    #[cfg(test)]
    pub(crate) fn ended_for_tests(&self, agent: &str, end: End) {
        let engine = self.engine;
        self.lock().runs.push(Run {
            engine,
            name: "lamp-test".into(),
            agent: agent.to_owned(),
            kind: TurnKind::Spoken,
            message: String::new(),
            asked: String::new(),
            queued_at: SystemTime::now(),
            queued_since: Instant::now(),
            tries: 0,
            reset_spent: false,
            run: Some(RunId("run-test".into())),
            progress: None,
            end: Some(end),
        });
    }

    fn pump(&self) {
        let mut backoff = self.retry_first;
        let mut ready_at = Instant::now();
        while !self.stop.load(Ordering::Acquire) {
            let ready = self.lock().ready;
            let idle_look_due = ready && ready_at.elapsed() >= IDLE_READY_EVERY;
            let busy = self.lock().runs.iter().any(|run| run.end.is_none());
            if !ready || (idle_look_due && !busy) {
                let result = self.session().ready(self.ready_look);
                ready_at = Instant::now();
                match result {
                    Ok(()) => {
                        self.became_ready();
                        backoff = self.retry_first;
                    }
                    Err(error) => {
                        let waiting = self.not_ready(&error);
                        self.expire_queued();
                        // A run in flight waits too: never a long retry
                        // while its end is owed; nor while this is the
                        // lamp's own agent, whose gateway lamp-update runs.
                        let most = retry_most(waiting || busy, default_is(self.engine));
                        self.rest(backoff.min(most));
                        backoff = backoff.saturating_mul(2).min(most);
                        continue;
                    }
                }
            }
            let in_flight = self
                .lock()
                .runs
                .iter()
                .find(|run| run.in_flight())
                .map(|run| {
                    (
                        run.agent.clone(),
                        run.run.clone().unwrap_or_else(|| RunId(String::new())),
                    )
                });
            if let Some((agent, run)) = in_flight {
                let (waited, progress, stuck) = {
                    let mut session = self.session();
                    let waited = session.wait(&run, self.wait_slice);
                    let progress = session.progress(&run);
                    let stuck =
                        matches!(waited, Err(Error::TimedOut)) && self.is_stuck(progress.as_ref());
                    if stuck {
                        // Best effort: the run ends here whatever the abort.
                        if let Err(error) = session.abort(&run) {
                            eprintln!("tasks: direct: stuck run {} not aborted: {error}", run.0);
                        }
                    }
                    (waited, progress, stuck)
                };
                if stuck {
                    let limit = progress.as_ref().map_or(self.stuck_after, |progress| {
                        quiet_limit(self.stuck_after, progress)
                    });
                    eprintln!(
                        "tasks: direct: run {} had no event in {} s and is stuck; ended, the next turn goes on",
                        run.0,
                        limit.as_secs()
                    );
                    let mut inner = self.lock();
                    if let Some(entry) = inner.find_mut(&agent) {
                        if progress.is_some() {
                            entry.progress = progress;
                        }
                        entry.end.get_or_insert(End::Failed(STUCK_WHY.into()));
                    }
                    drop(inner);
                    self.changed.notify_all();
                } else {
                    self.after_wait(&agent, waited, progress);
                }
                continue;
            }
            let next = {
                let inner = self.lock();
                // Spoken tasks go ahead of skill runs and makes, as in the
                // one session's queue; else the oldest first.
                inner
                    .runs
                    .iter()
                    .find(|run| run.queued() && run.kind == TurnKind::Spoken)
                    .or_else(|| inner.runs.iter().find(|run| run.queued()))
                    .map(|run| run.agent.clone())
            };
            if let Some(agent) = next {
                self.hand_over(&agent);
                continue;
            }
            self.rest(Duration::from_secs(1));
        }
    }

    /// Waits up to `pause` for a change (a turn queued, a launch waiting
    /// for readiness, a switch, a stop); at most [`RETRY_WAITING_MOST`]
    /// when something waits already (one that came after the pump last
    /// counted the waiters, before it rests, whose wake-up it missed).
    fn rest(&self, pause: Duration) {
        let inner = self.lock();
        let pause = if inner.waiting() {
            pause.min(RETRY_WAITING_MOST)
        } else {
            pause
        };
        let _ = self
            .changed
            .wait_timeout(inner, pause)
            .unwrap_or_else(PoisonError::into_inner);
    }

    fn became_ready(&self) {
        let mut inner = self.lock();
        if !inner.ready {
            eprintln!(
                "tasks: direct: {}'s gateway is ready, the kept session open",
                label(self.engine)
            );
        }
        inner.ready = true;
        inner.not_ready_since = None;
        inner.told_getting_ready = false;
        // The agent switch's end: the new default's gateway answers. This
        // line is the "is ready" event (step 4's voice line keys on the
        // `gateway` field going `ready`, [`gateway_said`]).
        if let Some(since) = inner.switch_since.take() {
            eprintln!(
                "tasks: direct: agent switch: {} is ready, {} ms after the switch",
                label(self.engine),
                since.elapsed().as_millis()
            );
        }
        drop(inner);
        self.changed.notify_all();
    }

    /// The session not ready (`error`): whether something waits for it.
    fn not_ready(&self, error: &Error) -> bool {
        let mut inner = self.lock();
        if inner.ready || inner.not_ready_since.is_none() {
            eprintln!("tasks: direct: {error}");
        }
        inner.ready = false;
        inner.not_ready_since.get_or_insert_with(Instant::now);
        inner.not_ready_why = error.to_string();
        let waiting = inner.waiting();
        let tell = self.tell_due(&mut inner);
        drop(inner);
        self.tell(tell);
        self.changed.notify_all();
        waiting
    }

    /// Whether the getting-ready hook is due: once a not-ready stretch,
    /// past its bound while waited on. Told by [`Self::tell`], unlocked.
    fn tell_due(&self, inner: &mut Inner) -> Option<String> {
        if !inner.ready
            && !inner.told_getting_ready
            && inner.waiting()
            && inner
                .not_ready_since
                .is_some_and(|since| since.elapsed() >= self.getting_ready_after)
        {
            inner.told_getting_ready = true;
            return Some(inner.not_ready_why.clone());
        }
        None
    }

    fn tell(&self, why: Option<String>) {
        if let Some(why) = why {
            (self.getting_ready)(&why);
        }
    }

    /// Queued turns that waited [`QUEUED_MOST`] for a gateway not ready,
    /// counted from when it stopped being ready, end failed.
    fn expire_queued(&self) {
        let mut inner = self.lock();
        let Some(since) = inner.not_ready_since else {
            return;
        };
        let most = self.queued_most;
        let mut ended = Vec::new();
        for run in inner.runs.iter_mut().filter(|run| run.queued()) {
            let from = if run.queued_since > since {
                run.queued_since
            } else {
                since
            };
            if from.elapsed() >= most {
                run.end = Some(End::Failed(QUEUED_NOT_READY.into()));
                ended.push(run.name.clone());
            }
        }
        drop(inner);
        for name in ended {
            eprintln!(
                "tasks: {name}: the gateway was not ready in {} s; ended",
                most.as_secs()
            );
        }
        self.changed.notify_all();
    }

    /// Ready, or waited for until `until` or `cancelled`
    /// ([`crate::gateway::Probe::ready_until`]).
    fn wait_ready(&self, until: Instant, cancelled: &dyn Fn() -> bool) -> bool {
        let mut inner = self.lock();
        inner.waiting_ready += 1;
        // The pump may be resting through a long back-off (nothing waited
        // before): woken, so it looks at once and every
        // [`RETRY_WAITING_MOST`] while this waits, not at its next retry.
        self.changed.notify_all();
        let result = loop {
            if inner.ready {
                break true;
            }
            inner.not_ready_since.get_or_insert_with(Instant::now);
            if let Some(why) = self.tell_due(&mut inner) {
                drop(inner);
                self.tell(Some(why));
                inner = self.lock();
                continue;
            }
            let now = Instant::now();
            if cancelled() || now >= until || self.stop.load(Ordering::Acquire) {
                break false;
            }
            let left = until
                .saturating_duration_since(now)
                .min(Duration::from_millis(50));
            inner = self
                .changed
                .wait_timeout(inner, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        };
        inner.waiting_ready -= 1;
        result
    }

    /// Queues one turn; its agent id. The pump hands it over in order.
    pub fn enqueue(&self, name: &str, kind: TurnKind, words: &str) -> Result<String, String> {
        let agent = new_agent_id()?;
        let engine = self.engine;
        let mut inner = self.lock();
        inner.runs.push(Run {
            engine,
            name: name.to_owned(),
            agent: agent.clone(),
            kind,
            message: crate::one_session::hand_over(name, words),
            asked: words.to_owned(),
            queued_at: SystemTime::now(),
            queued_since: Instant::now(),
            tries: 0,
            reset_spent: false,
            run: None,
            progress: None,
            end: None,
        });
        inner.prune();
        drop(inner);
        self.changed.notify_all();
        Ok(agent)
    }

    fn end(&self, agent: &str, end: End) {
        let mut inner = self.lock();
        if let Some(run) = inner.find_mut(agent)
            && run.end.is_none()
        {
            run.end = Some(end);
        }
        drop(inner);
        self.changed.notify_all();
    }

    fn hand_over(&self, agent: &str) {
        let Some((turn, make)) = self.lock().find(agent).map(|run| {
            (
                Turn {
                    task_id: run.name.clone(),
                    message: run.message.clone(),
                    kind: run.kind,
                },
                run.kind == TurnKind::Make,
            )
        }) else {
            return;
        };
        let mut session = self.session();
        // The skill-start fence's allow-list, read over the session before
        // each skill run (a restarted gateway may have lost it, #7280) and
        // put back when missing; a read the engine can't do never blocks.
        if turn.kind == TurnKind::SkillTask {
            let allowed = match session.skill_start_allowed() {
                Ok(true) => Ok(()),
                Ok(false) => session.allow_skill_start().map(|()| {
                    eprintln!(
                        "tasks: direct: {} put back on agent main's exec allow-list",
                        crate::skill_start::LAUNCHER
                    );
                }),
                Err(error) => {
                    eprintln!(
                        "tasks: direct: agent main's exec allow-list not read ({error}); the run is handed over"
                    );
                    Ok(())
                }
            };
            if let Err(error) = allowed {
                drop(session);
                eprintln!(
                    "tasks: direct: {}: not handed over: {} ({error})",
                    turn.task_id,
                    crate::gateway::SKILL_START_NOT_ALLOWED
                );
                self.end(
                    agent,
                    End::Failed(crate::gateway::SKILL_START_NOT_ALLOWED.into()),
                );
                return;
            }
        }
        // The context rotation as `reset()`, at the engine's bounds on the
        // gateway's own size (tokens and messages, of the engine's context);
        // where it can't be read, on the words handed over and read; for
        // Hermes also V1's turns and tokens backstop.
        let (estimate, turns) = {
            let inner = self.lock();
            (inner.tokens, inner.session_turns)
        };
        let size = session.session_size();
        let counts = session.rotation_counts().unwrap_or((turns, estimate));
        let full = near_limit(self.engine, size, estimate, counts, make);
        if full {
            self.reset_session(
                &mut **session,
                &format!("{} (a full conversation)", turn.task_id),
            );
        }
        // Tried before and maybe taken (a send that timed out or broke):
        // the run the gateway already has for this turn, never a second.
        let tried = self.lock().find(agent).is_some_and(|run| run.tries > 0);
        let adopted = if tried {
            session
                .running()
                .into_iter()
                .find(|progress| progress.task_id == turn.task_id)
                .map(|progress| progress.run)
        } else {
            None
        };
        let began = Instant::now();
        let sent = match adopted {
            Some(run) => {
                eprintln!(
                    "tasks: direct: {}: the gateway already has it as {}; not sent again",
                    turn.task_id, run.0
                );
                Ok(run)
            }
            None => session.send(&turn),
        };
        let progress = sent.as_ref().ok().and_then(|run| session.progress(run));
        drop(session);
        match sent {
            Ok(run) => {
                let mut inner = self.lock();
                inner.tokens = inner.tokens.saturating_add(tokens_in(&turn.message));
                inner.session_turns += 1;
                inner.turns += 1;
                inner.failed_sends = 0;
                let turns = inner.turns;
                // Cancelled while it was being handed over: aborted at once.
                let mut cancelled = None;
                if let Some(entry) = inner.find_mut(agent) {
                    if entry.end.is_some() {
                        cancelled = Some(run.clone());
                    }
                    entry.run = Some(run);
                    entry.progress = progress;
                }
                drop(inner);
                if let Some(run) = cancelled
                    && let Err(error) = self.session().abort(&run)
                {
                    eprintln!("tasks: direct: run {} not aborted: {error}", run.0);
                }
                eprintln!(
                    "tasks: {}: turn accepted in {} ms ({})",
                    turn.task_id,
                    began.elapsed().as_millis(),
                    crate::one_session::nth(turns)
                );
            }
            Err(error) => self.send_failed(agent, &turn.task_id, error),
        }
        self.changed.notify_all();
    }

    /// A send of `agent`'s turn failed with `error`. The gateway not ready:
    /// kept queued, sent once it is (its expiry, [`QUEUED_MOST`]). The
    /// session broken (refused, or [`BROKEN_SENDS`] failed in a row): a
    /// fresh session, and the turn tried once more. Past
    /// [`SEND_TRIES_MOST`], or broken again after its reset: failed.
    fn send_failed(&self, agent: &str, task: &str, error: Error) {
        eprintln!("tasks: direct: {task}: not handed over: {error}");
        let (broken, reset_spent, tries) = {
            let mut inner = self.lock();
            // A gateway not up is not a broken session.
            if !matches!(error, Error::NotReady(_)) {
                inner.failed_sends += 1;
            }
            let broken = matches!(error, Error::Refused(_)) || inner.failed_sends >= BROKEN_SENDS;
            match inner.find_mut(agent) {
                Some(run) => {
                    run.tries += 1;
                    (broken, run.reset_spent, run.tries)
                }
                None => return,
            }
        };
        if broken && !reset_spent {
            {
                let mut session = self.session();
                self.reset_session(&mut **session, &format!("{task} (a broken session)"));
            }
            let mut inner = self.lock();
            inner.failed_sends = 0;
            if let Some(run) = inner.find_mut(agent) {
                run.reset_spent = true;
            }
            return;
        }
        let again = matches!(error, Error::NotReady(_) | Error::Lost(_) | Error::TimedOut)
            && !broken
            && tries < SEND_TRIES_MOST;
        if again {
            if matches!(error, Error::NotReady(_) | Error::Lost(_)) {
                self.not_ready(&error);
            }
            return;
        }
        self.end(agent, End::Failed(error.to_string()));
    }

    /// A fresh conversation in place of the kept one, said why.
    fn reset_session(&self, session: &mut dyn DirectSession, why: &str) {
        match session.reset() {
            Ok(()) => {
                let mut inner = self.lock();
                eprintln!(
                    "tasks: direct: a fresh conversation for {why}, about {} tokens and {} turns in the last",
                    inner.tokens, inner.session_turns
                );
                inner.tokens = 0;
                inner.session_turns = 0;
                inner.resets += 1;
            }
            Err(error) => eprintln!("tasks: direct: the kept session not reset: {error}"),
        }
    }

    /// Whether a run, its last event `progress`, is stuck: quiet past its
    /// [`quiet_limit`] (V1's busyTTL).
    fn is_stuck(&self, progress: Option<&Progress>) -> bool {
        progress.is_some_and(|progress| quiet_past(self.stuck_after, progress, SystemTime::now()))
    }

    fn after_wait(&self, agent: &str, waited: Result<Outcome, Error>, progress: Option<Progress>) {
        let mut inner = self.lock();
        let ended = match waited {
            Ok(Outcome::Answered { text, progress }) => {
                inner.tokens = inner.tokens.saturating_add(tokens_in(&text));
                Some((End::Answered(text), Some(progress)))
            }
            Ok(Outcome::Failed { why, progress }) => {
                // The gateway dropped under the run (a restart, a crash):
                // the turns queued behind it end too, with the same "try
                // again" words, not left waiting for a recovery.
                if why == lamp_agent_gateway::DROPPED {
                    for queued in inner.runs.iter_mut().filter(|run| run.queued()) {
                        queued.end = Some(End::Failed(why.clone()));
                    }
                }
                Some((End::Failed(why), Some(progress)))
            }
            Ok(Outcome::Aborted { progress }) => Some((End::Aborted, Some(progress))),
            Err(Error::TimedOut) => None,
            Err(error @ (Error::Lost(_) | Error::NotReady(_))) => {
                drop(inner);
                // The session resyncs its open runs once connected again.
                self.not_ready(&error);
                inner = self.lock();
                None
            }
            // The gateway's link no longer knows the run (lost in a
            // restart): the person hears the drop's "try again" words.
            Err(Error::UnknownRun(_)) => {
                Some((End::Failed(lamp_agent_gateway::DROPPED.into()), None))
            }
            Err(error) => Some((End::Failed(error.to_string()), None)),
        };
        if let Some(run) = inner.find_mut(agent) {
            match ended {
                Some((end, last)) => {
                    run.progress = last.or(progress).or(run.progress.take());
                    // A cancel's end stays the one told.
                    run.end.get_or_insert(end);
                }
                None => {
                    if progress.is_some() {
                        run.progress = progress;
                    }
                }
            }
        }
        drop(inner);
        self.changed.notify_all();
    }

    /// Ends `agent`'s turn: dropped from the queue, or aborted on the
    /// gateway when handed over.
    pub fn cancel(&self, agent: &str) {
        let run = {
            let mut inner = self.lock();
            match inner.find_mut(agent) {
                Some(run) if run.queued() => {
                    run.end = Some(End::Aborted);
                    None
                }
                Some(run) if run.in_flight() => run.run.clone(),
                _ => None,
            }
        };
        if let Some(run) = run
            && let Err(error) = self.session().abort(&run)
        {
            eprintln!(
                "tasks: direct: cancelled run {} not aborted on the gateway: {error}",
                run.0
            );
        }
        self.end(agent, End::Aborted);
    }

    /// Waits for `agent`'s end until `until` or `cancelled` (then aborted,
    /// and `None`).
    pub fn wait_end(
        &self,
        agent: &str,
        until: Instant,
        cancelled: &dyn Fn() -> bool,
    ) -> Option<End> {
        let mut inner = self.lock();
        loop {
            match inner.find(agent) {
                None => return None,
                Some(run) => {
                    if let Some(end) = &run.end {
                        return Some(end.clone());
                    }
                }
            }
            if cancelled() || Instant::now() >= until || self.stop.load(Ordering::Acquire) {
                drop(inner);
                self.cancel(agent);
                return None;
            }
            inner = self
                .changed
                .wait_timeout(inner, Duration::from_millis(50))
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Whether `launch` is this session's: its engine on the lamp's own
    /// machine, no session named. Any other engine or machine: `false`.
    pub fn takes(&self, launch: &Launch) -> bool {
        launch.target.engine == self.engine
            && serves_now(self.engine)
            && launch.session.is_none()
            && self
                .lamp_machine()
                .is_some_and(|machine| machine == launch.target.machine_id)
    }

    /// A spoken task (seam 1), queued: `None` when not the direct path's.
    /// Its readiness is waited for before, by the worker's
    /// `gateway::launch_with` with [`Self::gateway`].
    pub fn spoken_launch(&self, launch: &Launch) -> Option<LaunchOutcome> {
        if !self.takes(launch) {
            return None;
        }
        Some(
            match self.enqueue(&launch.name, TurnKind::Spoken, &launch.prompt) {
                Ok(agent_id) => LaunchOutcome::Joined {
                    agent_id,
                    session: None,
                },
                Err(why) => {
                    eprintln!("tasks: {}: not queued: {why}", launch.name);
                    LaunchOutcome::NotSent {
                        code: "direct_not_queued".into(),
                    }
                }
            },
        )
    }

    /// `agent`'s turn as `harness show` gave it; `None` for one not here.
    pub fn observe(&self, agent: &str) -> Option<Observation> {
        self.lock().find(agent).map(observation_of_run)
    }

    /// Every turn here as `harness ps` rows.
    pub fn listed(&self) -> Vec<Listed> {
        self.lock().runs.iter().map(listed_of_run).collect()
    }

    /// The rows for `machine`'s `ps`: this session's when it is the lamp's.
    pub fn listed_on(&self, machine: &str) -> Vec<Listed> {
        if self.lamp_machine().as_deref() == Some(machine) {
            self.listed()
        } else {
            Vec::new()
        }
    }

    /// Each watched direct task's latest tool on the work records
    /// (`works::Board::step`), as the transcript's steps were.
    pub fn record_steps(&self, watched: &[Watched]) {
        let steps: Vec<(u64, u64, String)> = {
            let inner = self.lock();
            watched
                .iter()
                .filter_map(|task| {
                    let progress = inner.find(&task.agent_id)?.progress.as_ref()?;
                    let (n, tool) = step_of(progress)?;
                    Some((task.task_id, n, tool))
                })
                .collect()
        };
        let at = crate::works::unix_ms();
        crate::works::with(|board| {
            for (task, n, tool) in &steps {
                board.step(*task, *n, tool, at);
            }
        });
    }

    /// [`Self::refresh`] when a watched direct task's row changed since the
    /// last time (its state, line or last activity), so the bus's
    /// `tasks/status` follows the run within a turn of the loop rather than
    /// Harness's `ps` reads.
    pub fn refresh_if_changed(
        &self,
        tracker: Option<&mut crate::progress::Tracker>,
        watched: &[Watched],
    ) {
        let rows: Vec<(String, String, Option<String>, Option<u64>)> = self
            .listed()
            .into_iter()
            .filter(|row| watched.iter().any(|task| task.agent_id == row.agent_id))
            .map(|row| (row.agent_id, row.state, row.about, row.last_active_unix_ms))
            .collect();
        let sign = format!("{rows:?}");
        {
            let mut last = self.last_rows.lock().unwrap_or_else(|e| e.into_inner());
            if *last == sign {
                return;
            }
            *last = sign;
        }
        self.refresh(tracker, watched);
    }

    /// The tracker brought up to date from here for the watched direct
    /// tasks, at once, before a progress read is answered. Their steps go
    /// on the work records with the records' own refresh
    /// ([`Self::record_steps`]), not here.
    pub fn refresh(&self, tracker: Option<&mut crate::progress::Tracker>, watched: &[Watched]) {
        // A direct-path task whose run is gone here (pruned, or from
        // before a restart) is read too, as missing, as Harness's `ps`
        // would have: it is never asked of Harness ([`lamp_path`]).
        let ours: Vec<Watched> = watched
            .iter()
            .filter(|task| {
                self.owns(&task.agent_id)
                    || self.path(&task.engine, &task.machine_id, &task.agent_id)
            })
            .cloned()
            .collect();
        if ours.is_empty() {
            return;
        }
        let Some(tracker) = tracker else {
            return;
        };
        let mut machines: Vec<&str> = ours.iter().map(|task| task.machine_id.as_str()).collect();
        machines.sort_unstable();
        machines.dedup();
        for machine in machines {
            tracker.read(
                machine,
                Ok(self.listed()),
                &ours,
                crate::works::unix_ms(),
                0,
            );
        }
    }
}

/// The kept session's readiness as the Gateway's probe: a hand-over waits
/// on it through [`crate::gateway::Gateway`]'s own wait, as Harness's does
/// on the port.
impl crate::gateway::Probe for Arc<Direct> {
    fn ready(&mut self) -> bool {
        self.is_ready()
    }
    fn ready_until(&mut self, until: Instant, cancelled: &dyn Fn() -> bool) -> bool {
        self.wait_ready(until, cancelled)
    }
    fn not_ready(&self) -> String {
        let why = self.lock().not_ready_why.clone();
        if why.is_empty() {
            format!("{}'s gateway is not ready yet", label(self.engine))
        } else {
            why
        }
    }
}

impl Direct {
    /// The wait before a hand-over, ready at most `within`: OpenClaw's
    /// Gateway wait (which also marks OpenClaw in use, #8105), its probe
    /// this session's readiness.
    pub fn gateway(self: &Arc<Self>, within: Duration) -> crate::gateway::Gateway {
        crate::gateway::Gateway::with_bounds(
            Box::new(self.clone()),
            within,
            crate::gateway::READY_EVERY,
        )
    }

    /// Ready, or waited for until `until` or `cancelled`: the wait before
    /// a hand-over for an engine that is not OpenClaw (never OpenClaw's
    /// in-use mark); else why not, in words.
    pub fn ready_by(&self, until: Instant, cancelled: &dyn Fn() -> bool) -> Result<(), String> {
        if self.wait_ready(until, cancelled) {
            return Ok(());
        }
        let why = self.lock().not_ready_why.clone();
        Err(if why.is_empty() {
            format!("{}'s gateway is not ready yet", label(self.engine))
        } else {
            why
        })
    }

    /// The wait before a hand-over for this engine, to `until`: OpenClaw's
    /// [`Self::gateway`], any other [`Self::ready_by`].
    fn before_hand_over(
        self: &Arc<Self>,
        until: Instant,
        stopped: &dyn Fn() -> bool,
    ) -> Result<(), String> {
        if self.engine == EngineKind::Openclaw {
            self.gateway(crate::gateway::SERVING_WITHIN)
                .before_launch_until(until, stopped)
        } else {
            self.ready_by(
                until.min(Instant::now() + crate::gateway::SERVING_WITHIN),
                stopped,
            )
        }
    }
}

/// A spoken `launch` for a direct session that is not OpenClaw's (whose
/// wait is `gateway::launch_with`'s): its session's readiness waited for,
/// at most `within`; not ready by then, not sent, with the same
/// [`crate::gateway::NOT_READY`] code task news tells as "still getting
/// ready". `None`: no such session takes it, or it is ready.
pub fn not_ready_for(launch: &Launch, within: Duration) -> Option<LaunchOutcome> {
    let direct = lamps()
        .into_iter()
        .find(|direct| direct.engine != EngineKind::Openclaw && direct.takes(launch))?;
    let why = direct.ready_by(Instant::now() + within, &|| false).err()?;
    eprintln!(
        "tasks: {why} within {} s; the hand-over is not sent",
        within.as_secs()
    );
    Some(LaunchOutcome::NotSent {
        code: crate::gateway::NOT_READY.into(),
    })
}

/// A skill's `do.task` or a make (seam 2), in place of the gateway wait and
/// the one session's pane: `words` (the run's prompt, the skill-start line
/// in it for a skill) handed into the kept session, its end waited for to
/// `until` or `stopped`. `live` is the run's skill-start grant (none for a
/// make), `folder` the make's own folder.
#[allow(clippy::too_many_arguments)]
pub fn skill_turn(
    direct: &Arc<Direct>,
    run: &crate::skill_task::SkillRun,
    live: Option<&crate::skill_start::Live>,
    folder: &Path,
    words: &str,
    makes: bool,
    began: Instant,
    until: Instant,
    stopped: &dyn Fn() -> bool,
) -> Result<u64, String> {
    // Not the lamp's default agent (now, or after the wait: a switch under
    // it): its gateway is not run, so never waited for and never handed
    // to; refused at once, in words that say why.
    if !serves_now(direct.engine) {
        return Err(not_default_words(direct.engine));
    }
    if let Err(why) = direct.before_hand_over(until, stopped) {
        eprintln!("tasks: {}: {why}; the hand-over is not sent", run.run);
        return Err(format!(
            "the agent's gateway didn't take the run ({})",
            crate::gateway::NOT_READY
        ));
    }
    if stopped() || Instant::now() >= until {
        return Err("the skill run ended before hand-over".into());
    }
    if !serves_now(direct.engine) {
        return Err(not_default_words(direct.engine));
    }
    let kind = if makes {
        TurnKind::Make
    } else {
        TurnKind::SkillTask
    };
    let name = turn_name(run)?;
    let agent = direct.enqueue(&name, kind, words)?;
    run.started.store(true, Ordering::Release);
    let ended = direct.wait_end(&agent, until, stopped);
    let took = began.elapsed().as_secs();
    let line = format!(
        "[lamp-tasks] skill task {}: ended, {}'s gateway says idle, after {took} s; its agent: started (the lamp's default agent)",
        run.run,
        label(direct.engine)
    );
    match ended {
        Some(End::Answered(answer)) if makes => {
            eprintln!("{line}");
            if let Some((text, _)) =
                crate::skill_make::make_answer(folder, Some((answer.as_str(), false)))
            {
                let _ = run.answer.set(text);
            }
            Ok(0)
        }
        Some(End::Answered(answer)) => {
            eprintln!("{line}");
            // Failing closed, as the Harness path: the launcher claimed the
            // start and the fence said it started the script, or no answer.
            let ran = live.map_or_else(
                || crate::gateway::start_ran(false, false),
                crate::skill_start::Live::ran,
            );
            let answer = Some(answer).filter(|answer| !answer.trim().is_empty());
            if ran.is_ok()
                && let Some(answer) = answer
            {
                let _ = run.answer.set(answer);
            }
            ran.map(|()| 0)
        }
        Some(End::Failed(why)) => Err(why),
        Some(End::Aborted) | None => {
            if began.elapsed() >= run.deadline || Instant::now() >= until {
                Err(format!(
                    "the run was ended at its deadline, {} s",
                    run.deadline.as_secs()
                ))
            } else {
                Err("the run was ended as lamp-tasks stops".into())
            }
        }
    }
}

#[cfg(test)]
thread_local! {
    /// A test's own lamp session, seen on its thread only ([`lamps`]).
    static TEST_LAMP: std::cell::RefCell<Option<Arc<Direct>>> =
        const { std::cell::RefCell::new(None) };
    /// A test's lamp default, seen on its thread only ([`serves_now`]):
    /// None, no reader set.
    static TEST_DEFAULT: std::cell::Cell<Option<Option<EngineKind>>> =
        const { std::cell::Cell::new(None) };
}

/// The lamp's default engine now, as the runtime reads it from its lamp
/// side ([`set_lamp_default`]).
type DefaultReader = Box<dyn Fn() -> Option<EngineKind> + Send + Sync>;
static LAMP_DEFAULT: OnceLock<DefaultReader> = OnceLock::new();

/// Set once by the runtime: how the lamp's default engine is read now
/// (the first of `lamp_order`, the app's choice live).
pub fn set_lamp_default(read: impl Fn() -> Option<EngineKind> + Send + Sync + 'static) {
    let _ = LAMP_DEFAULT.set(Box::new(read));
}

/// A test's lamp default on its own thread (`Some(None)`: none known).
#[cfg(test)]
pub(crate) fn set_test_default(default: Option<Option<EngineKind>>) {
    TEST_DEFAULT.with(|cell| cell.set(default));
}

/// Whether `engine`'s direct session takes the lamp's work now: only while
/// it is the lamp's default, OpenClaw as Hermes. lamp-update runs one
/// agent's gateway at a time, the default's, and stops the other (it no
/// longer keeps OpenClaw's up while it is "in use" on a Hermes lamp), so a
/// task for the other agent would only wait for a gateway that never
/// starts (60 s for a spoken task, 240 s for a skill run). A task named for
/// Hermes on an OpenClaw lamp keeps Harness's path (its CLI needs no
/// gateway); one named for OpenClaw on a Hermes lamp is refused at once
/// ([`not_default_for`], [`NOT_DEFAULT`]). No reader set (no lamp side):
/// as before, it does.
fn serves_now(engine: EngineKind) -> bool {
    default_is(engine)
}

/// The words a run for `engine`, not the lamp's default agent now, is
/// refused with ([`NOT_DEFAULT`] in them).
fn not_default_words(engine: EngineKind) -> String {
    format!(
        "{} is not the lamp's default agent now, so the run was not handed over ({NOT_DEFAULT})",
        label(engine)
    )
}

/// A spoken `launch` for OpenClaw on the lamp's own machine, no session
/// named, while OpenClaw has a direct session but is not the lamp's
/// default: not sent, at once, with [`NOT_DEFAULT`], in place of the
/// Gateway's 60 s wait for a gateway lamp-update does not run (and of
/// Harness's `openclaw` pane, which needs that gateway too). `None`: not
/// such a launch, so the worker goes on as before. Hermes's on an OpenClaw
/// lamp keeps Harness's path (#8431), which runs it without its gateway.
pub fn not_default_for(launch: &Launch) -> Option<LaunchOutcome> {
    if launch.target.engine != EngineKind::Openclaw || launch.session.is_some() {
        return None;
    }
    let direct = lamp_for(EngineKind::Openclaw)?;
    if serves_now(EngineKind::Openclaw)
        || direct.lamp_machine().as_deref() != Some(launch.target.machine_id.as_str())
    {
        return None;
    }
    eprintln!(
        "tasks: {}: OpenClaw is not the lamp's default agent now; not sent ({NOT_DEFAULT})",
        launch.name
    );
    Some(LaunchOutcome::NotSent {
        code: NOT_DEFAULT.into(),
    })
}

/// The lamp's default as the runtime reads it now: `None` with no reader
/// set (no lamp side), `Some(None)` with no default.
#[cfg(not(test))]
fn current_default() -> Option<Option<EngineKind>> {
    LAMP_DEFAULT.get().map(|read| read())
}

/// In the crate's tests, a test's own default only ([`set_test_default`]).
#[cfg(test)]
fn current_default() -> Option<Option<EngineKind>> {
    TEST_DEFAULT.with(std::cell::Cell::get)
}

/// The default [`switch_check`] last saw: `None` before its first look.
#[cfg(not(test))]
static LAST_DEFAULT: Mutex<Option<Option<EngineKind>>> = Mutex::new(None);

/// `now` kept as the default last seen; the one before it.
#[cfg(not(test))]
fn last_default_swap(now: Option<EngineKind>) -> Option<Option<EngineKind>> {
    LAST_DEFAULT
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .replace(now)
}

#[cfg(test)]
thread_local! {
    /// A test's last default seen by [`switch_check`], on its thread only.
    static TEST_LAST_DEFAULT: std::cell::Cell<Option<Option<EngineKind>>> =
        const { std::cell::Cell::new(None) };
}

#[cfg(test)]
fn last_default_swap(now: Option<EngineKind>) -> Option<Option<EngineKind>> {
    TEST_LAST_DEFAULT.with(|cell| cell.replace(Some(now)))
}

/// The lamp's default agent switched: from `from` to `to`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Switched {
    pub from: Option<EngineKind>,
    pub to: Option<EngineKind>,
}

/// Looks whether the lamp's default agent changed since the last look, and
/// when it did, runs the switch (the owner's rule): every direct session of
/// another engine ends its turns, queued or running, with [`SWITCHED`]
/// ([`Direct::end_for_switch`]), and the new default's session looks at its
/// gateway again at once ([`Direct::look_again_for_switch`]), so
/// [`gateway_said`] says `switching` until it answers. The first look only
/// notes the default (a start is no switch); no reader set: nothing. Called
/// by the app's `app_engine_default` at once, and by lamp-tasks' main loop
/// every [`SWITCH_LOOK_EVERY`] for any other change of the lamp's order.
/// The journal's "agent switch started" line is the "switch started" event
/// (step 4's voice line keys on the `gateway` field going `switching`).
pub fn switch_check() -> Option<Switched> {
    // No default read (none chosen yet, or Harness's first engine report
    // not in at start-up) is never a switch, and is not kept as the last
    // default: only one engine to another ends the old agent's work, so a
    // start-up or a passing gap in the read never ends a running task.
    let now = current_default()?;
    now?;
    let from = last_default_swap(now)?;
    if from == now || from.is_none() {
        return None;
    }
    let lamps = lamps();
    let ending: usize = lamps
        .iter()
        .filter(|direct| Some(direct.engine) != now)
        .map(|direct| direct.end_for_switch(now))
        .sum();
    let to_session = lamps.iter().find(|direct| Some(direct.engine) == now);
    if let Some(direct) = to_session {
        direct.look_again_for_switch();
    }
    eprintln!(
        "tasks: direct: agent switch started: {} to {}; {ending} running task(s) of the old agent ended; {}",
        from.map_or("none", label),
        now.map_or("none", label),
        if to_session.is_some() {
            "waiting for the new agent's gateway"
        } else {
            "the new agent has no direct session here, ready as Harness's path"
        }
    );
    Some(Switched { from, to: now })
}

/// How often lamp-tasks' main loop looks for a change of the lamp's default
/// agent that did not come through `app_engine_default` ([`switch_check`]).
pub const SWITCH_LOOK_EVERY: Duration = Duration::from_secs(1);

/// The agent switch's state for the app, on the engines' default read
/// (`app_engine_default`, `app_engine_default_state`): `switching` from the
/// moment the lamp's default agent changed until its direct session's
/// gateway next answers ready, then `ready`. `ready` also when the default
/// has no direct session (Harness's path waits on its own) or no default is
/// read.
pub fn gateway_said() -> &'static str {
    let switching = current_default()
        .flatten()
        .and_then(lamp_for)
        .is_some_and(|direct| direct.switching());
    if switching { "switching" } else { "ready" }
}

/// Whether `engine` is the lamp's default now, or none is read.
#[cfg(not(test))]
fn default_is(engine: EngineKind) -> bool {
    LAMP_DEFAULT.get().is_none_or(|read| read() == Some(engine))
}

/// In the crate's tests, a test's own default only: a runtime test's
/// reader, set once for the process, never leaks into another test.
#[cfg(test)]
fn default_is(engine: EngineKind) -> bool {
    TEST_DEFAULT
        .with(std::cell::Cell::get)
        .is_none_or(|default| default == Some(engine))
}

/// The lamp's direct sessions, OpenClaw's first ([`LAMP`], [`HERMES_LAMP`]);
/// in the crate's tests, the one the test set on its own thread
/// ([`set_test_lamp`]), if any.
pub fn lamps() -> Vec<Arc<Direct>> {
    #[cfg(test)]
    if let Some(direct) = TEST_LAMP.with(|lamp| lamp.borrow().clone()) {
        return vec![direct];
    }
    [&LAMP, &HERMES_LAMP]
        .into_iter()
        .filter_map(|slot| slot.get().cloned())
        .collect()
}

/// The lamp's direct session for `engine`, when it is on.
pub fn lamp_for(engine: EngineKind) -> Option<Arc<Direct>> {
    lamps().into_iter().find(|direct| direct.engine == engine)
}

/// Whether a skill's run or a make on the lamp's default agent `engine`
/// (`gateway`: a gateway agent by its wrapper's data) goes to its direct
/// session ([`skill_turn`]): OpenClaw as a gateway agent with its switch
/// on; the lamp's Hermes with its switch on, whatever its wrapper.
pub fn takes_runs(engine: EngineKind, gateway: bool) -> bool {
    match engine {
        EngineKind::Openclaw => gateway && lamp_for(engine).is_some(),
        EngineKind::Hermes => lamp_for(engine).is_some(),
        _ => false,
    }
}

/// A test's lamp session on its own thread.
#[cfg(test)]
pub(crate) fn set_test_lamp(direct: Option<Arc<Direct>>) {
    TEST_LAMP.with(|lamp| *lamp.borrow_mut() = direct);
}

/// Whether a task of `engine` (its bus name, `openclaw`, `hermes`) on
/// `machine`, agent `agent`, is the direct path's: that engine's switch on,
/// the lamp's own machine, and the session serving now ([`serves_now`]) or
/// the turn its own and ended (an agent switch's end, [`SWITCHED`]). Its
/// progress and looks come from the gateway only, never from
/// Harness's `ps` or `show` (each ~2 s on the lamp, on the one worker a
/// hand-over waits behind). Other engines, other machines and the switch
/// off: `false`.
pub fn lamp_path(engine: &str, machine: &str, agent: &str) -> bool {
    lamps()
        .iter()
        .any(|direct| direct.path(engine, machine, agent))
}

/// A direct-path task's look ([`lamp_path`]) in place of Harness's `show`:
/// its run's, or `observe_failed` (what `show` of an agent Harness never
/// had says) once the run is gone here (pruned, or from before a restart).
/// `None`: not the direct path's, so Harness is asked as before.
pub fn lamp_observe(
    engine: &str,
    machine: &str,
    agent: &str,
) -> Option<Result<Observation, String>> {
    let direct = lamps()
        .into_iter()
        .find(|direct| direct.path(engine, machine, agent))?;
    Some(
        direct
            .observe(agent)
            .ok_or_else(|| "observe_failed".to_owned()),
    )
}

/// `watched` without the direct-path tasks ([`lamp_path`]): what Harness's
/// `wait` and `watch` may follow. A direct task's end is read from its run
/// (`Engine::push_direct_ended`); Harness never had it (`UNKNOWN_HARNESS`).
pub fn harness_watched(watched: &[Watched]) -> Vec<Watched> {
    watched
        .iter()
        .filter(|task| !lamp_path(&task.engine, &task.machine_id, &task.agent_id))
        .cloned()
        .collect()
}

/// The lamp's direct session whose turn `agent` is.
pub fn lamp_owns(agent: &str) -> Option<Arc<Direct>> {
    lamps().into_iter().find(|direct| direct.owns(agent))
}

fn tokens_in(text: &str) -> u64 {
    u64::try_from(text.len() / 4).unwrap_or(u64::MAX)
}

fn unix_ms(at: SystemTime) -> Option<u64> {
    at.duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .and_then(|since| u64::try_from(since.as_millis()).ok())
}

/// A fresh agent id, a lowercase UUID (v4), as Harness's are.
fn new_agent_id() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| "no random bytes".to_string())?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    ))
}

/// `text` at most `most` bytes, cut at a whole character; and whether cut.
fn cut_bytes(text: &str, most: usize) -> (String, bool) {
    let end = text.floor_char_boundary(most);
    (text[..end].to_owned(), end < text.len())
}

/// The state `harness ps`/`show` would say for a turn: queued here or on
/// the gateway `starting`, at work `working`, answered `idle`, failed
/// `failed`, aborted `stopped`. Never `idle` before the run ended.
pub fn state_of(queued: bool, phase: Option<&Phase>, end: Option<&End>) -> &'static str {
    match (end, phase) {
        (Some(End::Answered(text)), _) if !text.trim().is_empty() => "idle",
        // An end with no answer is no answer to tell: failed, never an idle
        // agent the store would wait on for ever.
        (Some(End::Answered(_) | End::Failed(_)), _) => "failed",
        (Some(End::Aborted), _) => "stopped",
        // Not yet handed over: starting. Handed over, the gateway has it:
        // working from then on, its first event or not (ee17, 13:58:48: the
        // bus read `starting` while `get_progress` read `working`).
        (None, _) if queued => "starting",
        (None, None | Some(Phase::Queued)) => "working",
        (None, Some(Phase::Thinking | Phase::Tool | Phase::Writing)) => "working",
        (None, Some(Phase::Done)) => "working",
        (None, Some(Phase::Failed { .. })) => "failed",
        (None, Some(Phase::Aborted)) => "stopped",
    }
}

/// A skill run's or make's task id on the gateway and in the journal: its
/// run id, or, with none (a make whose folder has no run id in its name,
/// ee17 14:04:35), the skill's name and a fresh short id.
fn turn_name(run: &crate::skill_task::SkillRun) -> Result<String, String> {
    if !run.run.trim().is_empty() {
        return Ok(run.run.clone());
    }
    let id = new_agent_id()?;
    let short: String = id.chars().take(8).collect();
    let skill = if run.skill.trim().is_empty() {
        "skill"
    } else {
        run.skill.as_str()
    };
    Ok(format!("{skill}-{short}"))
}

/// The line of a turn waiting for the one before it.
pub const QUEUED_ABOUT: &str = "waiting for the task before it to end";

/// An untrusted progress line made safe to say: file paths (any word with
/// a `/` or `\\`, or starting `~`) and every word the sign matcher reads
/// as the work done (`progress::signs_in`'s `Sign::Done`) left out, so no
/// unasked turn is started mid-run.
fn scrub(line: &str) -> String {
    line.split_whitespace()
        .filter(|word| {
            !(word.contains('/')
                || word.contains('\\')
                || word.starts_with('~')
                || crate::progress::signs_in(word).contains(&lamp_msgs::Sign::Done))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The progress line (`about`), from the run's own events: its plan's
/// "on step k of n" with the step at hand when it keeps one, the tool it
/// runs (or ran last) with its brief, else the tail of what it has
/// written; at most [`ABOUT_MOST`] characters, scrubbed ([`scrub`]) and
/// made plain (`now::step`): it is the agent's, so untrusted.
pub fn about(progress: &Progress) -> Option<String> {
    let mut parts = Vec::new();
    if !progress.plan.is_empty() {
        let done = progress.plan.iter().filter(|item| item.done).count();
        let n = progress.plan.len();
        match progress.plan.iter().find(|item| !item.done) {
            Some(item) => parts.push(format!(
                "on step {} of {n}: {}",
                (done + 1).min(n),
                scrub(&item.text)
            )),
            None => parts.push(format!("past step {n} of {n}")),
        }
    }
    if let Some(tool) = &progress.last_tool {
        let brief = scrub(&tool.brief);
        let doing = if tool.running { "running" } else { "ran" };
        parts.push(if brief.is_empty() {
            format!("{doing} {}", tool.name)
        } else {
            format!("{doing} {}: {brief}", tool.name)
        });
    }
    let line = if parts.is_empty() {
        // The tail of what it wrote: its newest words.
        let text = scrub(progress.last_text.as_deref()?);
        let skip = text.chars().count().saturating_sub(ABOUT_MOST);
        text.chars().skip(skip).collect()
    } else {
        parts.join("; ")
    };
    crate::now::step(&line, ABOUT_MOST)
}

/// The board's step for a run: the tool at hand (or last), numbered by the
/// tools done so far plus the one running.
pub fn step_of(progress: &Progress) -> Option<(u64, String)> {
    let tool = progress.last_tool.as_ref()?;
    let n = u64::from(progress.tools_done) + u64::from(tool.running);
    Some((n, crate::now::step(&tool.name, 64)?)).filter(|_| n > 0)
}

fn listed_of_run(run: &Run) -> Listed {
    Listed {
        agent_id: run.agent.clone(),
        state: state_of(
            run.queued(),
            run.progress.as_ref().map(|p| &p.phase),
            run.end.as_ref(),
        )
        .to_owned(),
        last_active_unix_ms: run
            .progress
            .as_ref()
            .map_or(unix_ms(run.queued_at), |p| unix_ms(p.updated_at)),
        // Waiting for the turn before it (one at a time): said so.
        about: if run.queued() {
            Some(QUEUED_ABOUT.to_owned())
        } else {
            run.progress.as_ref().and_then(about)
        },
        agent: Some(run.engine.argument().to_owned()),
    }
}

/// A run's row as `harness ps` gave it ([`listed_of_run`]), for tests and
/// the docs: `engine`'s session, `progress` the gateway's, `end` lamp-tasks'.
pub fn listed_of(
    engine: EngineKind,
    agent: &str,
    progress: Option<&Progress>,
    end: Option<&End>,
) -> Listed {
    listed_of_run(&Run {
        engine,
        name: String::new(),
        agent: agent.to_owned(),
        kind: TurnKind::Spoken,
        message: String::new(),
        asked: String::new(),
        queued_at: SystemTime::now(),
        queued_since: Instant::now(),
        tries: 0,
        reset_spent: false,
        run: progress.map(|p| p.run.clone()),
        progress: progress.cloned(),
        end: end.cloned(),
    })
}

fn observation_of_run(run: &Run) -> Observation {
    let (answer, truncated) = match &run.end {
        Some(End::Answered(text)) => cut_bytes(text, MAX_ANSWER_BYTES),
        // A drop's failure carries the person's "try again" words as its
        // last words: the voice's news of a stopped task says them, in the
        // person's language. An agent switch's end is told the same way.
        Some(End::Failed(why)) if why == lamp_agent_gateway::DROPPED || why == SWITCHED => {
            (why.clone(), false)
        }
        _ => (String::new(), false),
    };
    Observation {
        agent_id: run.agent.clone(),
        state: state_of(
            run.queued(),
            run.progress.as_ref().map(|p| &p.phase),
            run.end.as_ref(),
        )
        .to_owned(),
        ask: Some(cut_bytes(&run.asked, MAX_ASK_BYTES).0).filter(|ask| !ask.is_empty()),
        answer,
        truncated,
        tools: run.progress.as_ref().map_or(0, |p| u64::from(p.tools_done)),
    }
}

/// A run's observation as `harness show` gave it ([`observation_of_run`]).
pub fn observation_of(agent: &str, progress: Option<&Progress>, end: Option<&End>) -> Observation {
    observation_of_run(&Run {
        engine: EngineKind::Openclaw,
        name: String::new(),
        agent: agent.to_owned(),
        kind: TurnKind::Spoken,
        message: String::new(),
        asked: String::new(),
        queued_at: SystemTime::now(),
        queued_since: Instant::now(),
        tries: 0,
        reset_spent: false,
        run: progress.map(|p| p.run.clone()),
        progress: progress.cloned(),
        end: end.cloned(),
    })
}

#[cfg(test)]
pub(crate) mod tests;
