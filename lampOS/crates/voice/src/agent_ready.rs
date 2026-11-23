//! The lamp's own agent's readiness as the voice keeps it (`tasks/agent`,
//! #8138, `docs/tasks.md`, "The lamp's own agent not ready").
//!
//! lamp-tasks publishes one [`AgentReadiness`] for the lamp's own agent on
//! every change. The voice uses it three ways:
//!
//! - the provider worker keeps the latest ([`watch`], [`latest`]): a task
//!   for the lamp's own agent while it is not `ready` or `working` is
//!   refused at once, before anything is asked of lamp-tasks, with
//!   [`PROBLEM_LINE`] ([`refuses_now`], [`refusal`]); lamp-tasks' own
//!   `agent_not_ready` at prepare is told the same way; `harness_status`
//!   says the state ([`status_entry`]);
//! - a refusal, or a task of the lamp's own ended on a fault (Task news),
//!   is noted ([`note_refused`], [`note_failed`]);
//! - the coordinator follows it ([`ReadyAgain`]): when the agent comes back
//!   `ready` after being down, and someone was refused or had a task fail
//!   within [`REFUSED_WITHIN_MS`] of it, [`READY_AGAIN_LINE`] is offered
//!   once as task news, so it waits for a quiet moment like other news.
//!   Otherwise nothing is said: unprompted speech stays small.
use lamp_msgs::{AgentReadiness, Readiness};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// What the person hears, at once, when the lamp's own agent can't take
/// their task: no engine, no technical words, nothing promised later.
pub const PROBLEM_LINE: &str = "I'm having a problem right now; ask me again in a moment.";

/// [`PROBLEM_LINE`] for a wait of minutes or more (`error` other than
/// signed out: the Gateway down 6 min or more, opens failed past their
/// tries, a hold only the person can clear): never "in a moment".
pub const LATER_LINE: &str = "I'm having a problem right now; ask me again later.";

/// What the person hears while the lamp's own agent is being installed or
/// updated (`installing`: minutes to an hour).
pub const UPDATING_LINE: &str = "I'm being updated right now; ask me again later.";

/// What the person hears when a task already handed to the lamp's own
/// agent ended on a fault (`one_session_stuck`, `one_session_not_started`,
/// `agent_broken`): told as not done, and never done later.
pub const NOT_DONE_LINE: &str =
    "I couldn't finish that; something went wrong. Ask me again in a moment.";

/// What the lamp says once its own agent is back, only after someone was
/// refused or had a task fail while it was down ([`ReadyAgain`]).
pub const READY_AGAIN_LINE: &str = "I'm ready again; you can ask me now.";

/// A refusal or a failure counts for [`READY_AGAIN_LINE`] this long.
pub const REFUSED_WITHIN_MS: u64 = 15 * 60_000;

/// [`READY_AGAIN_LINE`] is let go if no quiet moment came this long after
/// the agent came back: later, it is old news.
pub const READY_AGAIN_FRESH: Duration = Duration::from_secs(5 * 60);

/// lamp-tasks' codes for a task of the lamp's own ended on a fault after
/// it was handed over (#7906, #7919, #8138): told as [`NOT_DONE_LINE`].
pub const FAULT_CODES: [&str; 3] = [
    "one_session_stuck",
    "one_session_not_started",
    "agent_broken",
];

/// lamp-tasks' refusal at prepare (or confirm) while the lamp's own agent
/// is not ready.
pub const NOT_READY_CODE: &str = "agent_not_ready";

/// The latest readiness the provider worker read.
static LATEST: Mutex<Option<AgentReadiness>> = Mutex::new(None);
/// Once a process: [`watch`].
static WATCHING: OnceLock<()> = OnceLock::new();
/// When a task of the lamp's own last ended on a fault, as the
/// coordinator's Task news saw it, in milliseconds since 1970.
#[cfg(not(test))]
static FAILED_AT_MS: Mutex<Option<u64>> = Mutex::new(None);

#[cfg(test)]
thread_local! {
    /// The readiness for a test on this thread only ([`latest`]).
    pub(crate) static LATEST_HERE: std::cell::RefCell<Option<AgentReadiness>> =
        const { std::cell::RefCell::new(None) };
    /// How many refusals a test on this thread noted ([`note_refused`]).
    pub(crate) static REFUSED_HERE: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// A fault a test on this thread noted ([`note_failed`]).
    pub(crate) static FAILED_HERE: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

/// Now, in milliseconds since 1970.
pub fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

/// Follow `tasks/agent` on a thread of its own, once a process, keeping the
/// latest for [`latest`]. Without a bus nothing is kept, and lamp-tasks'
/// own refusal at prepare still tells the person.
pub fn watch() {
    if WATCHING.set(()).is_err() {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("lamp-voice-agent-ready".into())
        .spawn(|| {
            loop {
                let subscribed = lamp_bus::Bus::from_environment().and_then(|bus| {
                    bus.subscribe_typed(AgentReadiness::TOPIC, "voice", AgentReadiness::TYPE)
                });
                match subscribed {
                    Ok(mut readiness) => loop {
                        match readiness.recv(Duration::from_secs(1)) {
                            Ok(Some(message)) => {
                                if let Some(read) = checked(message.parse().ok())
                                    && let Ok(mut latest) = LATEST.lock()
                                {
                                    *latest = Some(read);
                                }
                            }
                            Ok(None) => {}
                            Err(error) => {
                                eprintln!(
                                    "lamp-voice: {} could not be read: {error}",
                                    AgentReadiness::TOPIC
                                );
                                break;
                            }
                        }
                    },
                    Err(error) => eprintln!(
                        "lamp-voice: {} cannot be followed: {error}",
                        AgentReadiness::TOPIC
                    ),
                }
                std::thread::sleep(Duration::from_secs(30));
            }
        });
    if spawned.is_err() {
        eprintln!("lamp-voice: no thread to follow the lamp's own agent");
    }
}

/// `read` if it passes its own check.
pub(crate) fn checked(read: Option<AgentReadiness>) -> Option<AgentReadiness> {
    read.filter(|read| read.check().is_ok())
}

/// The latest readiness of the lamp's own agent; none before the first
/// read, or without a bus.
pub fn latest() -> Option<AgentReadiness> {
    #[cfg(test)]
    if let Some(here) = LATEST_HERE.with(|here| here.borrow().clone()) {
        return Some(here);
    }
    LATEST.lock().ok().and_then(|latest| latest.clone())
}

/// The readiness that refuses a task for the lamp's own agent now, if
/// any: the latest read says it takes none (not `ready` or `working`), for
/// the engine `named` or, with none named, the lamp's default. An engine
/// other than the one it reads is never refused here; lamp-tasks decides.
pub fn refuses_now(named: Option<&str>) -> Option<AgentReadiness> {
    let latest = latest()?;
    if latest.state.takes_tasks() || latest.state == Readiness::Off {
        return None;
    }
    named
        .is_none_or(|named| named == latest.engine)
        .then_some(latest)
}

/// The `why` lamp-tasks gives a signed-out agent.
pub const SIGNED_OUT_WHY: &str = "signed_out";

/// What the model is told when a task for the lamp's own agent is refused
/// in `state` with `why` (#8138): not installed, the not-set-up line;
/// signed out, the lamp's signed-out line ([`crate::language::SIGNED_OUT`]);
/// `installing`, [`UPDATING_LINE`]; any other `error`, [`LATER_LINE`]; any
/// other ([`Readiness::Starting`] with `tasks_restarting` among them),
/// [`refusal`]'s problem line. All but the first two are noted for the
/// ready-again line.
pub fn refused(state: Readiness, why: Option<&str>) -> Value {
    // Lasting states keep their existing lines, never "in a moment".
    if state == Readiness::NotInstalled {
        return json!({"error": crate::agent_tasks::LAMP_NOT_SET_UP});
    }
    if state == Readiness::Error && why == Some(SIGNED_OUT_WHY) {
        return json!({"error": format!(
            "Nothing was handed over: you are signed out. Tell them in these words, and nothing more: \"{}\"",
            crate::language::SIGNED_OUT
        )});
    }
    note_refused();
    match state {
        Readiness::Installing => refusal_with(UPDATING_LINE),
        // A normal start (a cold boot's Gateway takes minutes): the lamp's
        // own getting-ready line, as a skill run hears (#7351), never "a
        // problem". `tasks_restarting` and an unknown why stay a problem.
        Readiness::Starting if why.is_some_and(|why| STARTING_WHYS.contains(&why)) => {
            refusal_with(crate::skill_words::NOT_READY_LINE)
        }
        Readiness::Error => refusal_with(LATER_LINE),
        _ => refusal(),
    }
}

/// The whys of a normal start (lamp-tasks' `agent::state_of`): the Gateway
/// not answering yet since the boot, the one session opening, Harness's
/// own start. Told [`crate::skill_words::NOT_READY_LINE`].
const STARTING_WHYS: [&str; 3] = ["gateway_starting", "session_starting", "harness_starting"];

/// [`refused`] for lamp-tasks' `agent_not_ready` `answer`: its `readiness`
/// and `why`, the latest read's `why` when the answer has none and the
/// states agree; a state it can't read is told as a problem of the moment.
pub fn refused_by(answer: &Value) -> Value {
    let state = answer
        .get("readiness")
        .and_then(|said| serde_json::from_value::<Readiness>(said.clone()).ok())
        .unwrap_or(Readiness::Recovering);
    let why = answer
        .get("why")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            latest()
                .filter(|latest| latest.state == state)
                .and_then(|latest| latest.why)
        });
    refused(state, why.as_deref())
}

/// What the model is told when a task for the lamp's own agent is refused
/// because it is not ready: [`PROBLEM_LINE`] at once, nothing handed over
/// and nothing done later.
pub fn refusal() -> Value {
    refusal_with(PROBLEM_LINE)
}

/// [`refusal`] with `line` in place of [`PROBLEM_LINE`].
fn refusal_with(line: &str) -> Value {
    json!({"error": format!(
        "Nothing was handed over: you can't take work right now, and it will not be done later. Tell them at once in these words, and nothing more: \"{line}\""
    )})
}

/// The file the provider worker notes its refusals in, for the
/// coordinator's [`ReadyAgain`]: beside Task news' file of what was told.
pub fn refused_file() -> PathBuf {
    refused_beside(&crate::task_news::told_file())
}

/// [`refused_file`] beside the file of what was told, `told`.
pub fn refused_beside(told: &Path) -> PathBuf {
    told.with_file_name("agent-refused")
}

/// Note a refusal for want of the lamp's own agent, now ([`refused_file`]).
pub fn note_refused() {
    #[cfg(test)]
    REFUSED_HERE.with(|here| here.set(here.get() + 1));
    #[cfg(not(test))]
    if let Err(error) = std::fs::write(refused_file(), unix_ms().to_string()) {
        eprintln!("lamp-voice: a refusal of the lamp's own agent was not noted: {error}");
    }
}

/// When the last refusal was noted in `file`, in milliseconds since 1970.
pub fn refused_at_ms(file: &Path) -> Option<u64> {
    std::fs::read_to_string(file).ok()?.trim().parse().ok()
}

/// Note, in the coordinator, that a task of the lamp's own ended on a
/// fault now.
pub fn note_failed() {
    #[cfg(test)]
    FAILED_HERE.with(|here| here.set(Some(unix_ms())));
    #[cfg(not(test))]
    if let Ok(mut failed) = FAILED_AT_MS.lock() {
        *failed = Some(unix_ms());
    }
}

/// When a task of the lamp's own last ended on a fault ([`note_failed`]).
pub fn failed_at_ms() -> Option<u64> {
    #[cfg(test)]
    return FAILED_HERE.with(std::cell::Cell::get);
    #[cfg(not(test))]
    FAILED_AT_MS.lock().ok().and_then(|failed| *failed)
}

/// The lamp's own agent for `harness_status`: its state, and, when it
/// takes no task now, what to say if asked. Never its `why` or engine.
pub fn status_entry() -> Option<Value> {
    let latest = latest()?;
    let state = serde_json::to_value(latest.state).ok()?;
    let next = match latest.state {
        Readiness::Ready | Readiness::Working => "You can take work now; say so plainly if asked.",
        Readiness::Off | Readiness::NotInstalled => {
            "Your own helper is not set up on this lamp; say so plainly if asked."
        }
        Readiness::Installing => {
            "You are being updated and can't take work now; if asked, say so plainly and to ask again later."
        }
        Readiness::Starting => "You are still getting ready to take work; say so plainly if asked.",
        Readiness::Recovering => {
            "You can't take work right now; if asked, say you're having a problem right now and to ask again in a moment, with no technical words."
        }
        Readiness::Error => {
            "You can't take work right now; if asked, say you're having a problem right now and to ask again later, with no technical words."
        }
    };
    Some(json!({"state": state, "next": next}))
}

/// The coordinator's follower of the readiness, for [`READY_AGAIN_LINE`].
#[derive(Debug, Default)]
pub struct ReadyAgain {
    /// The state last seen; none before the first.
    last: Option<Readiness>,
    /// The agent was seen down (taking no task) since it was last `ready`.
    down: bool,
    /// The refusal or failure already answered by a ready-again line, so
    /// that one is never answered twice.
    answered_ms: Option<u64>,
}

impl ReadyAgain {
    /// `read` seen at `now_ms`, with the latest refusal or failure at
    /// `refused_ms`: whether [`READY_AGAIN_LINE`] is due now. Due only when
    /// the agent goes from down to `ready`, with a refusal or failure
    /// within [`REFUSED_WITHIN_MS`] not answered before. Never at the first
    /// value seen, nor for `working`: a task under way is its own news.
    pub fn seen(&mut self, read: Readiness, refused_ms: Option<u64>, now_ms: u64) -> bool {
        self.last = Some(read);
        if !read.takes_tasks() {
            // Down, at the voice's start too: a refusal from then on counts.
            self.down = true;
            return false;
        }
        if read != Readiness::Ready || !self.down {
            return false;
        }
        self.down = false;
        let due = refused_ms.is_some_and(|refused| {
            now_ms.saturating_sub(refused) <= REFUSED_WITHIN_MS
                && self.answered_ms.is_none_or(|answered| refused > answered)
        });
        if due {
            self.answered_ms = refused_ms;
        }
        due
    }

    /// Whether the agent is down now, as last seen.
    pub fn down(&self) -> bool {
        self.last.is_some_and(|last| !last.takes_tasks())
    }
}

/// The note Task news gives the model for [`READY_AGAIN_LINE`].
pub fn ready_again_note() -> String {
    crate::task_news::unprompted(&format!(
        "You are working again after a problem that kept you from taking a request a little while ago. Tell them in these words, and nothing more: \"{READY_AGAIN_LINE}\""
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const AT: u64 = 1_800_000_000_000;

    /// #8138: ready again is due once, only after the agent was down and
    /// someone was refused within 15 minutes; never otherwise.
    #[test]
    fn ready_again_is_due_once_and_only_after_a_refusal() {
        let mut follow = ReadyAgain::default();
        // Ready at the start: nothing.
        assert!(!follow.seen(Readiness::Ready, None, AT));
        // Down and back with nobody refused: nothing.
        assert!(!follow.seen(Readiness::Recovering, None, AT + 1));
        assert!(!follow.seen(Readiness::Ready, None, AT + 2));
        // Down, someone refused, back: due once.
        assert!(!follow.seen(Readiness::Recovering, None, AT + 3));
        assert!(follow.seen(Readiness::Ready, Some(AT + 4), AT + 5));
        // The same ready again, or another value: not again.
        assert!(!follow.seen(Readiness::Ready, Some(AT + 4), AT + 6));
        assert!(!follow.seen(Readiness::Working, Some(AT + 4), AT + 7));
        assert!(!follow.seen(Readiness::Ready, Some(AT + 4), AT + 8));
        // Down again; the old refusal was answered: nothing.
        assert!(!follow.seen(Readiness::Error, Some(AT + 4), AT + 9));
        assert!(!follow.seen(Readiness::Ready, Some(AT + 4), AT + 10));
        // A refusal older than 15 minutes: nothing.
        assert!(!follow.seen(Readiness::Starting, Some(AT + 11), AT + 11));
        assert!(!follow.seen(Readiness::Ready, Some(AT + 11), AT + 12 + REFUSED_WITHIN_MS));
        // Back as working first, then ready: due once ready.
        assert!(!follow.seen(Readiness::Recovering, None, AT + 20));
        assert!(!follow.seen(Readiness::Working, Some(AT + 21), AT + 22));
        assert!(follow.seen(Readiness::Ready, Some(AT + 21), AT + 23));
    }

    /// Down at the voice's start counts as down: a refusal then is answered.
    #[test]
    fn down_at_the_start_counts() {
        let mut follow = ReadyAgain::default();
        assert!(!follow.seen(Readiness::Recovering, None, AT));
        assert!(follow.down());
        assert!(follow.seen(Readiness::Ready, Some(AT + 1), AT + 2));
    }

    fn read(state: Readiness) -> AgentReadiness {
        AgentReadiness {
            engine: if state == Readiness::Off {
                String::new()
            } else {
                "openclaw".into()
            },
            state,
            why: None,
            since_unix_ms: AT,
            read_unix_ms: AT,
        }
    }

    /// Only a state that takes no task refuses, for the default or the
    /// engine it reads; another engine named is lamp-tasks' to decide.
    #[test]
    fn only_a_down_agent_refuses_and_only_its_own_engine() {
        for (state, refuses) in [
            (Readiness::Ready, false),
            (Readiness::Working, false),
            (Readiness::Off, false),
            (Readiness::NotInstalled, true),
            (Readiness::Installing, true),
            (Readiness::Starting, true),
            (Readiness::Recovering, true),
            (Readiness::Error, true),
        ] {
            LATEST_HERE.with(|here| *here.borrow_mut() = Some(read(state)));
            assert_eq!(refuses_now(None).is_some(), refuses, "{state:?}");
            assert_eq!(refuses_now(Some("openclaw")).is_some(), refuses);
            assert_eq!(refuses_now(Some("hermes")), None);
        }
        LATEST_HERE.with(|here| *here.borrow_mut() = None);
        assert_eq!(refuses_now(None), None, "nothing read refuses nothing");
    }

    /// #8138, the adversarial read: not installed and signed out keep their
    /// lasting lines, never "ask me again in a moment", and are no refusal
    /// for the ready-again line; every other down state (`tasks_restarting`
    /// among them) gets the problem line and is noted.
    #[test]
    fn lasting_states_keep_their_own_lines() {
        let said = |state: Readiness, why: Option<&str>| {
            let before = REFUSED_HERE.with(std::cell::Cell::get);
            let told = refused(state, why)["error"].as_str().unwrap().to_owned();
            (told, REFUSED_HERE.with(std::cell::Cell::get) > before)
        };
        let (told, noted) = said(Readiness::NotInstalled, None);
        assert_eq!(told, crate::agent_tasks::LAMP_NOT_SET_UP);
        assert!(!noted);
        let (told, noted) = said(Readiness::Error, Some(SIGNED_OUT_WHY));
        assert!(told.contains(crate::language::SIGNED_OUT), "{told}");
        assert!(!told.contains(PROBLEM_LINE) && !noted, "{told}");
        for (state, why) in [
            (Readiness::Starting, Some("tasks_restarting")),
            (Readiness::Starting, None),
            (Readiness::Recovering, Some("stuck")),
        ] {
            let (told, noted) = said(state, why);
            assert!(
                told.contains(PROBLEM_LINE) && noted,
                "{state:?} {why:?}: {told}"
            );
        }
        // A normal start is the getting-ready line, never "a problem"
        // (the Reviewer-lead's P3 on #8162: a cold boot's minutes).
        for why in STARTING_WHYS {
            let (told, noted) = said(Readiness::Starting, Some(why));
            assert!(
                told.contains(crate::skill_words::NOT_READY_LINE)
                    && !told.contains(PROBLEM_LINE)
                    && noted,
                "{why}: {told}"
            );
        }
        // lamp-tasks' answer: its readiness and why, else the latest's why.
        let by = |answer: Value| refused_by(&answer)["error"].as_str().unwrap().to_owned();
        assert_eq!(
            by(json!({"ok":false,"error":"agent_not_ready","readiness":"not_installed"})),
            crate::agent_tasks::LAMP_NOT_SET_UP
        );
        LATEST_HERE.with(|here| {
            *here.borrow_mut() = Some(AgentReadiness {
                why: Some(SIGNED_OUT_WHY.into()),
                ..read(Readiness::Error)
            })
        });
        assert!(
            by(json!({"ok":false,"error":"agent_not_ready","readiness":"error"}))
                .contains(crate::language::SIGNED_OUT)
        );
        LATEST_HERE.with(|here| *here.borrow_mut() = None);
        assert!(by(json!({"ok":false,"error":"agent_not_ready","readiness":"starting","why":"tasks_restarting"})).contains(PROBLEM_LINE));
        assert!(by(json!({"ok":false,"error":"agent_not_ready"})).contains(PROBLEM_LINE));
    }

    /// #8138 review (C4): a wait of minutes or more is never told "in a
    /// moment": `installing` (an install takes minutes to an hour) gets the
    /// updating line, and `error` (the Gateway down 6 min or more, opens
    /// failed past their tries, a hold only the person can clear) the
    /// problem line with "later"; `starting` and `recovering` keep "in a
    /// moment". All are noted for the ready-again line.
    #[test]
    fn a_long_wait_is_never_told_in_a_moment() {
        let said = |state: Readiness, why: Option<&str>| {
            let before = REFUSED_HERE.with(std::cell::Cell::get);
            let told = refused(state, why)["error"].as_str().unwrap().to_owned();
            assert!(
                REFUSED_HERE.with(std::cell::Cell::get) > before,
                "{state:?}"
            );
            told
        };
        assert!(said(Readiness::Installing, Some("installing")).contains(UPDATING_LINE));
        for why in [
            "gateway_down",
            "open_failed",
            "stuck_again",
            "held_approval",
        ] {
            assert!(
                said(Readiness::Error, Some(why)).contains(LATER_LINE),
                "{why}"
            );
        }
        for (state, why) in [
            (Readiness::Installing, Some("installing")),
            (Readiness::Error, Some("gateway_down")),
        ] {
            assert!(!said(state, why).contains("in a moment"), "{state:?}");
        }
        assert!(said(Readiness::Recovering, Some("held")).contains(PROBLEM_LINE));
        assert!(said(Readiness::Starting, None).contains(PROBLEM_LINE));
        for state in [Readiness::Installing, Readiness::Error] {
            LATEST_HERE.with(|here| *here.borrow_mut() = Some(read(state)));
            let next = status_entry().unwrap()["next"].as_str().unwrap().to_owned();
            assert!(!next.contains("in a moment"), "{state:?}: {next}");
        }
        LATEST_HERE.with(|here| *here.borrow_mut() = None);
    }

    /// The lines carry no engine or technical words.
    #[test]
    fn the_lines_name_no_engine() {
        for line in [
            PROBLEM_LINE,
            LATER_LINE,
            UPDATING_LINE,
            NOT_DONE_LINE,
            READY_AGAIN_LINE,
        ] {
            for word in [
                "OpenClaw", "Hermes", "agent", "session", "gateway", "Harness",
            ] {
                assert!(!line.contains(word), "{line}");
            }
        }
        assert!(refusal()["error"].as_str().unwrap().contains(PROBLEM_LINE));
        assert!(!refusal().to_string().contains("look into"));
        assert!(ready_again_note().contains(READY_AGAIN_LINE));
    }
}
