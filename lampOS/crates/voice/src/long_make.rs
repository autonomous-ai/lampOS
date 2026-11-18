//! An agent make past a minute runs on, and its person is told when it is
//! ready (#6811: Fourteen's ask at 17:21 and the first builder's yes at
//! 17:23, 18 Nov). With `LAMP_MAKE_MAY_RUN_ON=on` ([`ENVIRONMENT`], off
//! unless set, in no unit), beside `LAMP_MAKE_SKILL_AGENT`.
//!
//! - **At a minute.** A voice make handed to the lamp's agent still has no
//!   answer [`RUN_ON_AFTER`] after its waiting line: the lamp says
//!   [`RUN_ON_LINE`] once, in place of the long-wait line, and the voice
//!   turn ends ([`let_go`]); the person is free to talk about anything.
//! - **It runs on** in lamp-tasks, up to [`MAKE_WITHIN`] (10 minutes), for
//!   a make marked "may run on" alone (`"may_run_on":true` on its line,
//!   `lamp_tasks::skill_make::RUN_ON_MOST_WITHIN`); every other make keeps
//!   120 s. One at a time: a second make ask while one runs is told
//!   [`still_making_line`], nothing made. Each make's news keeps a mark of
//!   its own in [`FILE`] ([`marks`]), so a later make never takes an untold
//!   one's place; past [`MOST_MARKS`] untold, the oldest is left to the
//!   app's list.
//! - **Done:** the same checks and repairs as any agent make. A finished
//!   long make is kept but stays off, `ready_untold` on its `made.json`
//!   entry (what the app reads of made skills), until its person has heard
//!   its telling, as the lamp's own turn ([`Tells`]): one line,
//!   [`ready_on_line`] with its undo and no yes asked (the first builder, 18
//!   Nov 18:41), or, for code that keeps its yes, its read-back (below).
//!   Told only with their face seen alone (or the private room, as a
//!   read-back is held), not mid-conversation, never in the quiet hours,
//!   never to an empty room ([`may_tell`]), and never while a yes or a
//!   clash's choice is still awaited (Heron's P2 on #7774: it would take
//!   that wait's place); then it says nothing, its offer given back, and is
//!   told at a later chance ([`told`]). Offered at most [`MOST_OFFERS`]
//!   times, then left to `ready_untold` in the app's list, still off. Only once
//!   that turn was played through ([`heard_through`], [`heard`]) does it go
//!   on as a finished make does; cut, it stays off and is told again. Not
//!   put on then, its person is told why ([`not_on_line`]). One with code of
//!   its own goes on the same way, by its yes's path with no yes asked, only
//!   as a finished make's would (`made_skill::code_goes_on_at_once`); one
//!   that keeps its yes (every one while `LAMP_MADE_CODE_ON_AT_ONCE` is
//!   off; one that sends, or with the camera, the microphone or hearing,
//!   always) is read back as an app make's is, its yes awaited, never said
//!   on; one the lamp can't run is said ready and off. One that clashes is
//!   asked the clash's choice as a finished make is ([`told`]). Not back today, it is told the next time.
//! - **A failure, the 10 minutes run out, or a restart** while it ran (the
//!   mark kept `making`, [`after_start`]; run out, kept so once): one plain
//!   line, [`not_finished_line`], at the next such moment, and nothing kept.
//!   Never DeepSeek's draft.
//! - **"Is my skill ready?"** is told the truth ([`status`], through
//!   `make_skill`'s list of the person's skills): made, still making and
//!   for how long, or not made.
//!
//! Off: every make, its lines and its limits are as before, byte for byte.
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};

/// The switch: `on`, and nothing else.
pub const ENVIRONMENT: &str = "LAMP_MAKE_MAY_RUN_ON";
/// From a make's waiting line to [`RUN_ON_LINE`] (the first builder: "At
/// 60 s, not 90").
pub const RUN_ON_AFTER: Duration = Duration::from_secs(60);
/// Said once, at [`RUN_ON_AFTER`]; the voice turn ends after it.
pub const RUN_ON_LINE: &str = "This one's taking a while. I'll tell you when it's ready.";
/// The longest a make that may run on goes on, its repairs included
/// (lamp-tasks' `RUN_ON_MOST_WITHIN`).
pub const MAKE_WITHIN: Duration = Duration::from_secs(600);
/// The longest one of its answers is waited for: a repair is begun only
/// while one more still ends within [`MAKE_WITHIN`].
pub const WRITE_WITHIN: Duration = Duration::from_secs(300);
/// The mark beside the made skills' index: the long make's state.
pub const FILE: &str = "long-make.json";
/// The key on a made skill's `made.json` entry while its person hasn't
/// heard it is ready: shown in the app at once.
pub const READY_UNTOLD: &str = "ready_untold";
/// A `making` mark this much older than [`MAKE_WITHIN`] ran out with
/// nobody to settle it.
const RAN_OUT_GRACE_MS: u64 = 60_000;
/// How often the coordinator reads the mark, at most.
const READ_EVERY: Duration = Duration::from_secs(3);
/// A telling offered and not taken (its turn cut, or not begun) is offered
/// again after this.
const OFFER_AGAIN_AFTER: Duration = Duration::from_secs(120);
/// The admission basis of the turn that tells a long make's news.
pub const BASIS: &str = "long_make_told";
/// How a telling's note opens; then its key.
const OPENS: &str = "(Nobody has spoken. [long-make ";

std::thread_local! {
    static HERE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Whether the switch is on.
pub fn on() -> bool {
    if let Some(on) = HERE.with(std::cell::Cell::get) {
        return on;
    }
    on_with(std::env::var(ENVIRONMENT).ok().as_deref())
}

/// Whether `value` turns it on: `on`, and nothing else.
pub fn on_with(value: Option<&str>) -> bool {
    value == Some("on")
}

/// [`on`] for the calling thread alone, for tests; `None` reads the
/// environment again.
#[doc(hidden)]
pub fn set_on_here(on: Option<bool>) {
    HERE.with(|here| here.set(on));
}

/// "your water reminder skill", or "your skill" with no name.
fn your(name: Option<&str>) -> String {
    match name.map(crate::made_skill::spoken_name) {
        Some(name) if !name.trim().is_empty() => format!("your {} skill", name.trim()),
        _ => "your skill".to_owned(),
    }
}

/// What a second make ask is told while one runs on.
pub fn still_making_line(name: Option<&str>) -> String {
    format!("I'm still making {}.", your(name))
}

/// What the lamp says first when a long make is ready.
pub fn ready_line(name: &str) -> String {
    let your = your(Some(name));
    let mut letters = your.chars();
    match letters.next() {
        Some(first) => format!("{}{} is ready.", first.to_uppercase(), letters.as_str()),
        None => String::new(),
    }
}

/// What the lamp says of a long make that failed, ran out or was cut by a
/// restart.
pub fn not_finished_line(name: Option<&str>) -> String {
    format!("I couldn't finish {}.", your(name))
}

/// Why a long make ended with nothing kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotFinished {
    Failed,
    RanOut,
    Restart,
}

impl NotFinished {
    fn word(self) -> &'static str {
        match self {
            Self::Failed => "failed",
            Self::RanOut => "ran_out",
            Self::Restart => "restart",
        }
    }

    fn of(word: &str) -> Self {
        match word {
            "ran_out" => Self::RanOut,
            "restart" => Self::Restart,
            _ => Self::Failed,
        }
    }
}

/// What the lamp says of a long make kept and told ready that then
/// couldn't be put on, `why` in [`crate::made_skill::put_on`]'s plain words.
pub fn not_on_line(name: &str, why: &str) -> String {
    format!(
        "I couldn't turn {} on: {}.",
        your(Some(name)),
        why.trim().trim_end_matches('.')
    )
}

/// The most long makes' marks kept at once: a make let run on past it
/// drops the oldest untold one, its skill left `ready_untold` in the
/// app's list, so the marks stay few and no ask is ever refused for them.
pub const MOST_MARKS: usize = 3;

/// A long make's state, as its mark ([`FILE`]) keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// Under way since `began_ms`; `name` DeepSeek's draft's, when it had one.
    Making { began_ms: u64, name: Option<String> },
    /// Made, kept and off, its person not told yet.
    Made { name: String, sha256: String },
    /// Nothing kept, its person not told yet.
    NotMade {
        name: Option<String>,
        why: NotFinished,
        ended_ms: u64,
    },
    /// Told ready and heard, but it couldn't be put on (`why`, in plain
    /// words): still `ready_untold`, its person not told that yet.
    NotOn {
        name: String,
        sha256: String,
        why: String,
        ended_ms: u64,
    },
}

impl State {
    fn to_json(&self) -> Value {
        match self {
            Self::Making { began_ms, name } => {
                json!({"state": "making", "began_ms": began_ms, "name": name})
            }
            Self::Made { name, sha256 } => json!({"state": "made", "name": name, "sha256": sha256}),
            Self::NotMade {
                name,
                why,
                ended_ms,
            } => {
                json!({"state": "not_made", "name": name, "why": why.word(), "ended_ms": ended_ms})
            }
            Self::NotOn {
                name,
                sha256,
                why,
                ended_ms,
            } => json!({
                "state": "not_on",
                "name": name,
                "sha256": sha256,
                "why": why,
                "ended_ms": ended_ms
            }),
        }
    }

    fn of_json(value: &Value) -> Option<Self> {
        let name = value["name"].as_str().map(str::to_owned);
        match value["state"].as_str()? {
            "making" => Some(Self::Making {
                began_ms: value["began_ms"].as_u64()?,
                name,
            }),
            "made" => Some(Self::Made {
                name: name?,
                sha256: value["sha256"].as_str()?.to_owned(),
            }),
            "not_made" => Some(Self::NotMade {
                name,
                why: NotFinished::of(value["why"].as_str().unwrap_or_default()),
                ended_ms: value["ended_ms"].as_u64().unwrap_or_default(),
            }),
            "not_on" => Some(Self::NotOn {
                name: name?,
                sha256: value["sha256"].as_str()?.to_owned(),
                why: value["why"].as_str().unwrap_or_default().to_owned(),
                ended_ms: value["ended_ms"].as_u64().unwrap_or_default(),
            }),
            _ => None,
        }
    }
}

/// Every long make's mark in `directory`, as kept, one per make not told
/// yet (so a second never takes an untold first's place, Twelve's P1 on
/// #7774), a `making` one at most. A `making` one past [`MAKE_WITHIN`]
/// and its grace at `now_ms` ran out with nobody to settle it: it is kept
/// so at once, its `ended_ms` fixed, so its telling's key stays one
/// (Twelve's other P1).
pub fn marks(directory: &Path, now_ms: u64) -> Vec<State> {
    crate::made_skill::index_locked(directory, || marks_in(directory, now_ms))
}

/// [`marks`], the index's lock held (`made_skill::index_locked`: the
/// coordinator and the worker both write the marks).
fn marks_in(directory: &Path, now_ms: u64) -> Vec<State> {
    let Some(value) = std::fs::read(directory.join(FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
    else {
        return Vec::new();
    };
    let kept: Vec<State> = match &value {
        Value::Array(marks) => marks.iter().filter_map(State::of_json).collect(),
        mark => State::of_json(mark).into_iter().collect(),
    };
    let mut ran_out = false;
    let marks: Vec<State> = kept
        .into_iter()
        .map(|state| match state {
            State::Making { began_ms, name }
                if now_ms.saturating_sub(began_ms)
                    > MAKE_WITHIN.as_millis() as u64 + RAN_OUT_GRACE_MS =>
            {
                ran_out = true;
                State::NotMade {
                    name,
                    why: NotFinished::RanOut,
                    ended_ms: now_ms,
                }
            }
            state => state,
        })
        .collect();
    if ran_out {
        eprintln!("lamp-voice: a long make's mark ran out with nobody to settle it: kept so");
        write_all(directory, &marks);
    }
    marks
}

/// The mark that matters in `directory` at `now_ms`: the make under way,
/// else the first not told.
pub fn read(directory: &Path, now_ms: u64) -> Option<State> {
    let marks = marks(directory, now_ms);
    marks
        .iter()
        .find(|state| matches!(state, State::Making { .. }))
        .cloned()
        .or_else(|| marks.into_iter().next())
}

/// Keep `marks` as the mark file in `directory`, written whole and moved
/// over it; none removes it.
fn write_all(directory: &Path, marks: &[State]) {
    // The offers counted for tellings no longer marked go with them, so no
    // stale count is left (Twelve's 20:26 P3 on #7774).
    let mut offered = offers(directory);
    if !offered.is_empty() {
        let marked: Vec<String> = marks.iter().filter_map(key).collect();
        let before = offered.len();
        offered.retain(|offered, _| marked.contains(offered));
        if offered.len() != before {
            write_offers(directory, &offered);
        }
    }
    if marks.is_empty() {
        let _ = std::fs::remove_file(directory.join(FILE));
        return;
    }
    let marks = Value::Array(marks.iter().map(State::to_json).collect());
    let kept = std::fs::create_dir_all(directory).and_then(|()| {
        let new = directory.join(format!(".{FILE}.new"));
        std::fs::write(&new, marks.to_string())?;
        std::fs::rename(&new, directory.join(FILE))
    });
    if let Err(error) = kept {
        eprintln!("lamp-voice: a long make's mark was not kept: {error}");
    }
}

/// As the second route's worker starts: a make marked `making` was cut by
/// the restart, and its person is told so once ([`not_finished_line`]).
/// A known limit: its run in lamp-tasks isn't stopped, and may go on up to
/// [`WRITE_WITHIN`] for an answer thrown away, lamp-tasks refusing a new
/// make `busy` until then (`docs/skills.md`).
pub fn after_start(directory: &Path, now_ms: u64) {
    crate::made_skill::index_locked(directory, || after_start_in(directory, now_ms));
}

/// [`after_start`], the index's lock held.
fn after_start_in(directory: &Path, now_ms: u64) {
    let mut marks = marks(directory, now_ms);
    let mut cut = false;
    for state in &mut marks {
        if let State::Making { name, .. } = state {
            cut = true;
            *state = State::NotMade {
                name: name.take(),
                why: NotFinished::Restart,
                ended_ms: now_ms,
            };
        }
    }
    if cut {
        eprintln!("lamp-voice: a long make was cut by a restart: its person is told so");
        write_all(directory, &marks);
    }
}

/// A make the voice may let run on, by its request: under way, let go to
/// run on, or settled before it was let go.
enum Phase {
    Running {
        directory: PathBuf,
        name: Option<String>,
        /// Handed to the agent with the long make's time ([`to_agent`]).
        agent: bool,
    },
    LetGo {
        directory: PathBuf,
    },
    Settled,
}

/// The voice's makes that may run on, by request.
static PHASES: Mutex<Vec<(u64, Phase)>> = Mutex::new(Vec::new());

fn phases() -> std::sync::MutexGuard<'static, Vec<(u64, Phase)>> {
    PHASES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A voice make for `request` begins, kept in `directory`: it may be let
/// run on ([`let_go`]) until it settles ([`settle`]). With the switch on
/// only.
pub fn begin(request: u64, directory: &Path) {
    if request == 0 {
        return;
    }
    let mut phases = phases();
    phases.retain(|(of, phase)| *of != request && !matches!(phase, Phase::Settled));
    phases.push((
        request,
        Phase::Running {
            directory: directory.to_path_buf(),
            name: None,
            agent: false,
        },
    ));
}

/// The name of DeepSeek's draft for `request`'s make, said in its lines
/// before the agent's is known.
pub fn hint(request: u64, name: &str) {
    for (of, phase) in phases().iter_mut() {
        if *of == request
            && let Phase::Running { name: kept, .. } = phase
        {
            *kept = Some(name.to_owned());
        }
    }
}

/// `request`'s make is handed to the lamp's agent with the long make's
/// time (`"may_run_on":true`): cut before its minute, it is let run on
/// all the same ([`cut`]).
pub fn to_agent(request: u64) {
    for (of, phase) in phases().iter_mut() {
        if *of == request
            && let Phase::Running { agent, .. } = phase
        {
            *agent = true;
        }
    }
}

/// `request`'s turn was cut at `now_ms`, its make still under way: one
/// handed to the agent with the long make's time is let run on, so its
/// outcome goes through the same gate ([`settled`], [`Tells`]) and is never
/// put on, nor said with no gate (El's P2 on #7774). Whether it was.
pub fn cut(request: u64, now_ms: u64) -> bool {
    let agent = phases()
        .iter()
        .any(|(of, phase)| *of == request && matches!(phase, Phase::Running { agent: true, .. }));
    agent && let_go(request, now_ms)
}

/// Held by the voice turn waiting on `request`'s make: dropped before
/// [`Self::answered`], the turn was cut, and the make is let run on at
/// once ([`cut`]), its `making` mark written then, not only when it ends;
/// so a mute, sleep or worker restart before it ends tells it as not
/// finished ([`after_start`]), never loses it untold. A make not handed to
/// the agent yet goes as before (its end checks the cut again).
pub struct CutAtDrop {
    request: u64,
    waiting: bool,
}

impl CutAtDrop {
    /// The turn waits on `request`'s make.
    pub fn new(request: u64) -> Self {
        Self {
            request,
            waiting: true,
        }
    }

    /// The turn took the make's answer: no cut.
    pub fn answered(mut self) {
        self.waiting = false;
    }
}

impl Drop for CutAtDrop {
    fn drop(&mut self) {
        if self.waiting && cut(self.request, crate::app_make::now_unix_ms()) {
            eprintln!(
                "lamp-voice: request {}: a long make's turn was cut before its minute: it runs on, marked making",
                self.request
            );
        }
    }
}

/// The voice turn waiting on `request`'s make lets it run on, at `now_ms`:
/// true when it was still under way, its mark then kept `making` beside
/// any other make's not told yet; false when it settled already, is no
/// such make, or another runs on (the turn waits for it).
pub fn let_go(request: u64, now_ms: u64) -> bool {
    let mut phases = phases();
    let Some((_, phase)) = phases.iter_mut().find(|(of, _)| *of == request) else {
        return false;
    };
    let Phase::Running {
        directory, name, ..
    } = phase
    else {
        return false;
    };
    let directory = directory.clone();
    let marked = crate::made_skill::index_locked(&directory, || {
        let mut marks = marks(&directory, now_ms);
        if marks
            .iter()
            .any(|state| matches!(state, State::Making { .. }))
        {
            return false;
        }
        // Few marks: the oldest untold one is left to the app's list.
        while marks.len() >= MOST_MARKS {
            marks.remove(0);
        }
        marks.push(State::Making {
            began_ms: now_ms,
            name: name.clone(),
        });
        write_all(&directory, &marks);
        true
    });
    if marked {
        *phase = Phase::LetGo { directory };
    }
    marked
}

/// Whether `request`'s make was let run on.
pub fn is_let_go(request: u64) -> bool {
    phases()
        .iter()
        .any(|(of, phase)| *of == request && matches!(phase, Phase::LetGo { .. }))
}

/// `request`'s make has its outcome: the folder its outcome is kept in
/// when it was let run on ([`settled`] then), else none, and the turn
/// still waiting takes it as before.
pub fn settle(request: u64) -> Option<PathBuf> {
    let mut phases = phases();
    let at = phases.iter().position(|(of, _)| *of == request)?;
    match std::mem::replace(&mut phases[at].1, Phase::Settled) {
        Phase::LetGo { directory } => {
            phases.remove(at);
            Some(directory)
        }
        _ => None,
    }
}

/// A long make settled at `now_ms` in `directory`: made (`Ok`, the kept
/// skill's name and SHA-256), kept off and marked [`READY_UNTOLD`]; or not
/// (`Err`, whether it ran out), nothing kept. Its `making` mark takes the
/// outcome, every other make's mark kept; its person is told at the next
/// moment ([`Tells`]).
pub fn settled(directory: &Path, made: Result<(&str, &str), bool>, now_ms: u64) {
    crate::made_skill::index_locked(directory, || settled_in(directory, made, now_ms));
}

/// [`settled`], the index's lock held.
fn settled_in(directory: &Path, made: Result<(&str, &str), bool>, now_ms: u64) {
    let mut marks = marks(directory, now_ms);
    let making = marks
        .iter()
        .position(|state| matches!(state, State::Making { .. }));
    let name = match making.map(|at| &marks[at]) {
        Some(State::Making { name, .. }) => name.clone(),
        _ => None,
    };
    let outcome = match made {
        Ok((name, sha256)) => {
            crate::made_skill::mark(directory, sha256, READY_UNTOLD, Some(json!(true)));
            eprintln!("lamp-voice: a long make was made: kept off until its person hears it");
            // The same package made again is told once.
            marks.retain(
                |state| !matches!(state, State::Made { sha256: kept, .. } if kept == sha256),
            );
            State::Made {
                name: name.to_owned(),
                sha256: sha256.to_owned(),
            }
        }
        Err(ran_out) => {
            eprintln!(
                "lamp-voice: a long make made nothing ({}): its person is told so",
                if ran_out { "ran out" } else { "failed" }
            );
            State::NotMade {
                name,
                why: if ran_out {
                    NotFinished::RanOut
                } else {
                    NotFinished::Failed
                },
                ended_ms: now_ms,
            }
        }
    };
    let making = marks
        .iter()
        .position(|state| matches!(state, State::Making { .. }));
    match making {
        Some(at) => marks[at] = outcome,
        None => marks.push(outcome),
    }
    write_all(directory, &marks);
}

/// What a make ask is told in `directory` at `now_ms` while a long make
/// runs on ([`still_making_line`]); none otherwise, any other make's
/// untold news kept as it is.
pub fn still_making(directory: &Path, now_ms: u64) -> Option<Value> {
    let Some(State::Making { name, .. }) = marks(directory, now_ms)
        .into_iter()
        .find(|state| matches!(state, State::Making { .. }))
    else {
        return None;
    };
    let line = still_making_line(name.as_deref());
    eprintln!("lamp-voice: a make asked while a long make runs on: told so, nothing made");
    Some(json!({
        "error": "a skill is still being made; one is made at a time",
        "next": format!("Say only this, in the language of the conversation, and nothing else: {line}"),
    }))
}

/// The voice turn's answer once its make was let run on: it was told
/// [`RUN_ON_LINE`] already.
pub fn let_go_told() -> Value {
    json!({
        "running_on": true,
        "next": "They were told already that it takes a while and that you'll tell them when \
                 it's ready. Say nothing more about it now, and call nothing.",
    })
}

/// "Is my skill ready?": the long make's state in `directory` at `now_ms`
/// ([`read`]), for `make_skill`'s list; names only when `names` (the owner
/// hears).
pub fn status(directory: &Path, now_ms: u64, names: bool) -> Value {
    let named = |name: Option<String>| {
        name.filter(|_| names)
            .map(|name| crate::made_skill::spoken_name(&name))
    };
    match read(directory, now_ms) {
        Some(State::Making { began_ms, name }) => json!({
            "state": "still_making",
            "minutes_so_far": now_ms.saturating_sub(began_ms).div_ceil(60_000),
            "most_minutes": MAKE_WITHIN.as_secs() / 60,
            "name": named(name),
        }),
        Some(State::Made { name, .. }) => json!({"state": "made", "name": named(Some(name))}),
        Some(State::NotMade { name, .. }) => json!({"state": "not_made", "name": named(name)}),
        Some(State::NotOn { name, .. }) => {
            json!({"state": "made_not_on", "name": named(Some(name))})
        }
        None => json!({"state": "none_being_made"}),
    }
}

/// What the model is told to do with [`status`].
pub const STATUS_NEXT: &str = "If they asked whether a skill they asked for is ready, answer \
     truthfully from long_make: made and ready (it is told in a moment and goes on then, or, if \
     it needs their yes, is read back for it), still \
     being made (for how long so far), made but not on (they're told why in a moment), or not \
     made.";

/// Whether a long make's news may be told now: its person's face seen
/// alone, or the private room (`owner_alone`,
/// `services::owner_sees_alone_now`, as a read-back is held, since one can
/// name what a skill reads), not mid-conversation (`quiet`), not the quiet
/// hours (`night`), and someone at the lamp (`someone_there`).
pub fn may_tell(owner_alone: bool, quiet: bool, night: bool, someone_there: bool) -> bool {
    owner_alone && quiet && !night && someone_there
}

/// The most times one telling is offered (El's P2 on #7774): then it is
/// left to `ready_untold` in the app's list and to "Is my skill ready?",
/// the skill off until told, never said again and again. Counted in
/// [`OFFERS_FILE`], so a restart doesn't begin it again.
pub const MOST_OFFERS: u32 = 3;

/// Beside the marks: how many times each telling was offered, by its key.
pub const OFFERS_FILE: &str = "long-make-offers.json";

/// The offers kept in `directory`, by key.
fn offers(directory: &Path) -> std::collections::BTreeMap<String, u32> {
    std::fs::read(directory.join(OFFERS_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// One more offer of the telling `key` kept in `directory`, the counts of
/// tellings no longer marked (`marked`) dropped: how many now.
fn offer_counted(directory: &Path, key: &str, marked: &[String]) -> u32 {
    crate::made_skill::index_locked(directory, || {
        let mut offers = offers(directory);
        offers.retain(|offered, _| marked.contains(offered));
        let times = offers.entry(key.to_owned()).or_default();
        *times += 1;
        let times = *times;
        write_offers(directory, &offers);
        times
    })
}

/// The offer of the telling `key` in `directory` given back, not said
/// ([`told`] while a yes or a clash's choice is awaited): it counts as
/// never offered.
fn offer_given_back(directory: &Path, key: &str) {
    crate::made_skill::index_locked(directory, || {
        let mut offers = offers(directory);
        let Some(times) = offers.get_mut(key) else {
            return;
        };
        *times = times.saturating_sub(1);
        if *times == 0 {
            offers.remove(key);
        }
        write_offers(directory, &offers);
    });
}

/// `offers` kept as [`OFFERS_FILE`] in `directory`, written whole and moved
/// over it; none removes it.
fn write_offers(directory: &Path, offers: &std::collections::BTreeMap<String, u32>) {
    if offers.is_empty() {
        let _ = std::fs::remove_file(directory.join(OFFERS_FILE));
        return;
    }
    let kept = serde_json::to_vec(offers)
        .map_err(std::io::Error::other)
        .and_then(|bytes| {
            let new = directory.join(format!(".{OFFERS_FILE}.new"));
            std::fs::write(&new, bytes)?;
            std::fs::rename(&new, directory.join(OFFERS_FILE))
        });
    if let Err(error) = kept {
        eprintln!("lamp-voice: a long make's offers were not kept: {error}");
    }
}

/// The coordinator's side: the marks read off the made skills' folder,
/// each telling's offers so far ([`OFFERS_FILE`]), and when each was
/// offered last.
pub struct Tells {
    directory: PathBuf,
    read_at: Option<Instant>,
    marks: Vec<State>,
    times: std::collections::BTreeMap<String, u32>,
    last: Vec<(String, Instant)>,
}

impl Tells {
    /// With the switch on and made skills kept here, the product's folder.
    pub fn from_environment() -> Option<Self> {
        let keeping = crate::made_skill::Keeping::from_environment()?;
        on().then(|| Self::at(keeping.directory))
    }

    /// Reading the marks in `directory`.
    pub fn at(directory: PathBuf) -> Self {
        Self {
            directory,
            read_at: None,
            marks: Vec::new(),
            times: std::collections::BTreeMap::new(),
            last: Vec::new(),
        }
    }

    /// The telling due at `now` and `now_ms`, its key and line: the first
    /// mark not under way that wasn't offered within [`OFFER_AGAIN_AFTER`],
    /// nor [`MOST_OFFERS`] times. The marks are read at most once per
    /// [`READ_EVERY`]; one whose skill was removed in the app since goes
    /// quietly (El's P3 on #7774).
    fn due(&mut self, now: Instant, now_ms: u64) -> Option<(String, String)> {
        if self
            .read_at
            .is_none_or(|at| now.duration_since(at) >= READ_EVERY)
        {
            self.read_at = Some(now);
            self.marks = marks(&self.directory, now_ms);
            let directory = &self.directory;
            self.marks.retain(|state| {
                if !removed(directory, state) {
                    return true;
                }
                eprintln!("lamp-voice: a long make's skill was removed: its mark goes quietly");
                if let Some(key) = key(state) {
                    let _ = take(directory, &key, now_ms);
                }
                false
            });
            self.times = if self.marks.is_empty() {
                Default::default()
            } else {
                offers(directory)
            };
            let keys: Vec<String> = self.marks.iter().filter_map(key).collect();
            self.last.retain(|(key, _)| keys.contains(key));
        }
        self.marks.iter().find_map(|state| {
            let key = key(state)?;
            let held = self
                .times
                .get(&key)
                .is_some_and(|times| *times >= MOST_OFFERS)
                || self.last.iter().any(|(offered, last)| {
                    *offered == key && now.duration_since(*last) < OFFER_AGAIN_AFTER
                });
            (!held).then_some(key).zip(tell_line(state))
        })
    }

    /// Whether a telling is due ([`Self::next`] would give one), asked
    /// before anything else of the moment is read.
    pub fn has_news(&mut self, now: Instant, now_ms: u64) -> bool {
        self.due(now, now_ms).is_some()
    }

    /// The written turn's note that tells the news due at `now` and
    /// `now_ms`, counted as offered; none with nothing to tell. Asked only
    /// once [`may_tell`] holds.
    pub fn next(&mut self, now: Instant, now_ms: u64) -> Option<String> {
        let (told, line) = self.due(now, now_ms)?;
        let marked: Vec<String> = self.marks.iter().filter_map(key).collect();
        let times = offer_counted(&self.directory, &told, &marked);
        self.times.insert(told.clone(), times);
        self.last.retain(|(offered, _)| *offered != told);
        self.last.push((told.clone(), now));
        Some(format!("{OPENS}{told}] Say only, in a few words: {line})"))
    }
}

/// Whether the skill a mark tells of was removed in the app since: a
/// made one, or one not put on, no longer kept.
fn removed(directory: &Path, state: &State) -> bool {
    match state {
        State::Made { sha256, .. } | State::NotOn { sha256, .. } => {
            crate::made_skill::kept_by_sha256(directory, sha256).is_none()
        }
        State::NotMade { .. } | State::Making { .. } => false,
    }
}

/// What a mark's telling says first; none for one under way.
fn tell_line(state: &State) -> Option<String> {
    match state {
        State::Made { name, .. } => Some(ready_line(name)),
        State::NotMade { name, .. } => Some(not_finished_line(name.as_deref())),
        State::NotOn { name, why, .. } => Some(not_on_line(name, why)),
        State::Making { .. } => None,
    }
}

/// A telling's key: `ready <sha256>`, `not_made <ended_ms>` or
/// `not_on <sha256>`.
fn key(state: &State) -> Option<String> {
    match state {
        State::Made { sha256, .. } => Some(format!("ready {sha256}")),
        State::NotMade { ended_ms, .. } => Some(format!("not_made {ended_ms}")),
        State::NotOn { sha256, .. } => Some(format!("not_on {sha256}")),
        State::Making { .. } => None,
    }
}

/// How the line the worker gives the model opens and closes.
fn say_only(line: &str) -> String {
    format!(
        "(Nobody has spoken. Say only this, word for word, then stop; if the conversation is in \
         another language, say it in that language: {line})"
    )
}

/// The key of a telling's note ([`Tells::next`]); none for any other text.
pub fn key_of(note: &str) -> Option<&str> {
    note.strip_prefix(OPENS)?
        .split_once(']')
        .map(|(key, _)| key)
}

/// What [`told`] says of a skill with code of its own when this lamp runs
/// none.
const NO_CODE_HERE: &str = "this lamp doesn't run a skill's own code";

/// How the "ready" line ends (the first builder, 18 Nov 18:41: everything
/// is on, nothing is asked; a made skill is said in one line with its
/// undo, never read back for a yes).
pub const UNDO: &str = "Say 'remove it' to undo.";

/// What the lamp says when a long make is ready and goes on: one line,
/// with its undo ([`UNDO`]).
pub fn ready_on_line(name: &str) -> String {
    ready_on_with(name, "")
}

/// [`ready_on_line`] with `also` said before its undo.
fn ready_on_with(name: &str, also: &str) -> String {
    format!(
        "{} is ready and on.{also} {UNDO}",
        ready_line(name).trim_end_matches(" is ready.")
    )
}

/// Whether the ready skill `made`, its telling heard whole, goes on then
/// ([`heard`]): with no code of its own and no clash; or with code that may
/// go on with no yes (`LAMP_MADE_CODE_ON_AT_ONCE`,
/// [`crate::made_skill::code_goes_on_at_once`], by the path a yes puts it
/// on by, `make_skill_local::code_on_at_once`), exactly as a finished make
/// does. One that keeps its yes (Twelve's P2 on #7774: every skill with
/// code while `LAMP_MADE_CODE_ON_AT_ONCE` is off; one that sends, or with
/// the camera, the microphone or hearing, always) is never put on here.
fn goes_on_once_heard(
    keeping: &crate::made_skill::Keeping,
    made: &crate::made_skill::Made,
) -> bool {
    if made.uses.code {
        crate::made_skill::code_goes_on_at_once(made)
    } else {
        crate::made_clash::found(keeping, made).is_empty()
    }
}

/// A written turn `request`'s `text` on the second route, with the switch
/// on: a long make's telling ([`Tells::next`]), the model given what to
/// say; nothing goes on and nothing is cleared here, only once the turn was
/// heard whole ([`heard`]). Its gate holds the owner seen alone
/// ([`may_tell`]). Ready: one line, [`ready_on_line`], no yes asked (the
/// first builder, 18 Nov 18:41), with the link ask for a service not
/// linked yet (`linked`, as a finished make is told). With code that keeps
/// its yes ([`goes_on_once_heard`]), read back as an app make's is and its
/// yes awaited, never said on. With code while this lamp runs none, or
/// clashing with a skill on the lamp, it isn't said on: off and why, or the
/// clash's question asked ([`crate::made_clash::asked`]) as a finished
/// make's is. Not made, or not put on: its line. While a yes or a clash's
/// choice is still awaited (Heron's P2 on #7774), nothing: a telling would
/// replace that wait, so it says nothing, its mark kept and its offer given
/// back, to be told at a later chance. Any other text as it is.
pub fn told(
    keeping: &crate::made_skill::Keeping,
    request: u64,
    text: String,
    (now_ms, now): (u64, Instant),
    linked: impl FnOnce() -> Option<Vec<String>>,
) -> String {
    let Some(asked) = key_of(&text) else {
        return text;
    };
    let directory = &keeping.directory;
    let Some(state) = marks(directory, now_ms)
        .into_iter()
        .find(|state| key(state).as_deref() == Some(asked))
    else {
        // Told already, or another's: nothing more to say.
        return "(Nobody has spoken. Say nothing, and call nothing.)".to_owned();
    };
    // A yes still awaited (an earlier telling's say-back, or a quick
    // make's) or a clash's choice still asked: a telling now would take its
    // place, the skill it waits for stranded off (Heron's P2 on #7774). It
    // says nothing, its mark kept and its offer given back, and is told at
    // a later chance; a silent turn is never heard through, so nothing is
    // taken.
    if crate::skill_write_yes::awaited(now) || crate::made_clash::awaited(now) {
        eprintln!(
            "lamp-voice: request {request}: a long make's news waits: a yes or a clash's choice is still awaited"
        );
        offer_given_back(directory, asked);
        return "(Nobody has spoken. Say nothing, and call nothing.)".to_owned();
    }
    match state {
        State::Made { sha256, .. } => {
            let Some(made) = crate::made_skill::kept_by_sha256(directory, &sha256) else {
                // Removed in the app since: its mark goes quietly, never
                // told "couldn't finish" (El's P3 on #7774).
                eprintln!(
                    "lamp-voice: request {request}: a long make's skill is gone: its mark goes quietly"
                );
                let _ = take(directory, asked, now_ms);
                return "(Nobody has spoken. Say nothing, and call nothing.)".to_owned();
            };
            let link = crate::made_skill::needs_link(&made.uses, linked)
                .as_deref()
                .and_then(crate::made_skill::needs_link_ask)
                .map(|ask| format!(" {ask}"))
                .unwrap_or_default();
            let on = crate::made_skill::is_on(directory, &sha256);
            if !on && made.uses.code && !crate::made_skill::scripts_on() {
                eprintln!(
                    "lamp-voice: request {request}: a long make's skill with code is told ready and off: this lamp runs no skill's code"
                );
                return say_only(&format!(
                    "{} {}",
                    ready_line(&made.name),
                    crate::made_skill::not_on_line(NO_CODE_HERE)
                ));
            }
            // Code that keeps its yes (every skill with code while
            // `LAMP_MADE_CODE_ON_AT_ONCE` is off; one that sends, or with the
            // camera, the microphone or hearing, always): never said on nor
            // put on with no yes, as a finished make's isn't (Twelve's 19:40
            // P2 on #7774). Said back as an app make's is
            // (`make_skill_local::app_code_asked`), its yes awaited from
            // this request on (Twelve's 20:26 P2); `may_tell` holds the
            // owner seen alone.
            if !on && made.uses.code && !crate::made_skill::code_goes_on_at_once(&made) {
                let id = crate::skill_write_yes::wait_for_yes_noted(
                    crate::make_skill_local::code_asks(&made),
                    request,
                    now,
                );
                eprintln!(
                    "lamp-voice: request {request}: a long make's skill with code that keeps its yes is told ready and said back; its yes is awaited"
                );
                return format!(
                    "(Nobody has spoken. {} {})",
                    crate::skill_write_yes::say_whole(&format!(
                        "{} {}{link}",
                        ready_line(&made.name),
                        crate::made_skill::on_line(&made)
                    )),
                    crate::skill_write_yes::then_wait(&id)
                );
            }
            if !on && !made.uses.code {
                let clashes = crate::made_clash::found(keeping, &made);
                if !clashes.is_empty() {
                    let asked = crate::made_clash::asked(keeping, &made, &clashes, request, now);
                    eprintln!(
                        "lamp-voice: request {request}: a long make's skill is told ready; it clashes, so the person is asked"
                    );
                    return format!(
                        "(Nobody has spoken. Say first, in the language of the conversation: \
                         \"{}\"{link} Then: {})",
                        ready_line(&made.name),
                        asked["next"].as_str().unwrap_or_default()
                    );
                }
            }
            // Its code, once on, may run nowhere yet (a skill's tasks off),
            // as a skill put on at once is said.
            let nowhere = if made.uses.code && !crate::skill_task_yes::on() {
                crate::made_skill::CODE_RUNS_NOWHERE_YET
            } else {
                ""
            };
            eprintln!(
                "lamp-voice: request {request}: a long make's skill is told ready and on, in one line; it goes on once heard whole"
            );
            say_only(&format!("{}{link}", ready_on_with(&made.name, nowhere)))
        }
        State::NotMade { name, why, .. } => {
            eprintln!(
                "lamp-voice: request {request}: a long make that made nothing ({}) is told",
                why.word()
            );
            say_only(&not_finished_line(name.as_deref()))
        }
        State::NotOn {
            name, why, sha256, ..
        } => {
            if crate::made_skill::kept_by_sha256(directory, &sha256).is_none() {
                eprintln!(
                    "lamp-voice: request {request}: a long make's skill is gone: its mark goes quietly"
                );
                let _ = take(directory, asked, now_ms);
                return "(Nobody has spoken. Say nothing, and call nothing.)".to_owned();
            }
            eprintln!("lamp-voice: request {request}: a long make's skill not put on is told why");
            say_only(&not_on_line(&name, &why))
        }
        State::Making { .. } => text,
    }
}

/// [`told`] on the lamp: the made skills' own folder, and what is linked
/// as a voice make reads it.
pub fn told_here(request: u64, text: String) -> String {
    if !on() || !text.starts_with(OPENS) {
        return text;
    }
    let Some(keeping) = crate::made_skill::Keeping::from_environment() else {
        return text;
    };
    told(
        &keeping,
        request,
        text,
        (crate::app_make::now_unix_ms(), Instant::now()),
        || crate::services::linked_codes(Path::new(crate::services::FOLDER)),
    )
}

/// The mark `key` in `directory` at `now_ms` taken off, every other kept:
/// it, or none when told already.
fn take(directory: &Path, key_of_it: &str, now_ms: u64) -> Option<State> {
    replace(directory, key_of_it, None, now_ms)
}

/// The mark `key` in `directory` at `now_ms` taken off, `with` kept in
/// its place when some, in one write under the index's lock: it, or none
/// when told already (nothing then changes).
fn replace(directory: &Path, key_of_it: &str, with: Option<State>, now_ms: u64) -> Option<State> {
    crate::made_skill::index_locked(directory, || {
        let mut marks = marks(directory, now_ms);
        let at = marks
            .iter()
            .position(|state| key(state).as_deref() == Some(key_of_it))?;
        let taken = match with {
            Some(with) => std::mem::replace(&mut marks[at], with),
            None => marks.remove(at),
        };
        write_all(directory, &marks);
        Some(taken)
    })
}

/// The turn that told the news `key` was heard whole (the coordinator's
/// proof a say's `spoken` takes): its person knows. A ready skill that
/// goes on then ([`goes_on_once_heard`]) is put on first (`put_on`, as a
/// finished make goes on), the index's lock not held while the helper is
/// waited for; only then is its mark taken off, every other make's kept,
/// so a crash between leaves it told again, never lost (Twelve's P3 on
/// #7774). Put on, or on already, `ready_untold` is taken off. Its put-on
/// failing, it stays off and `ready_untold`, and its mark becomes why
/// ([`not_on_line`]), told at the next moment, never left thinking it is
/// on. One that clashes waits for the choice asked in that turn, and one
/// with code that keeps its yes for the yes asked in it: each stays off
/// until then, and loses `ready_untold`, as told. A turn cut or not heard
/// whole calls nothing: the skill stays off, untold, and is told again
/// next time.
pub fn heard(
    keeping: &crate::made_skill::Keeping,
    told: &str,
    now_ms: u64,
    put_on: impl FnOnce(&crate::made_skill::Made) -> Result<(), String>,
) {
    let directory = &keeping.directory;
    let Some(state) = marks(directory, now_ms)
        .into_iter()
        .find(|state| key(state).as_deref() == Some(told))
    else {
        return;
    };
    eprintln!("lamp-voice: a long make's news was heard whole: told once");
    match state {
        State::Made { sha256, .. } => {
            let made = crate::made_skill::kept_by_sha256(directory, &sha256);
            let not_on = match &made {
                Some(made)
                    if !crate::made_skill::is_on(directory, &sha256)
                        && goes_on_once_heard(keeping, made) =>
                {
                    put_on(made).err()
                }
                _ => None,
            };
            match (made, not_on) {
                (Some(made), Some(why)) => {
                    eprintln!(
                        "lamp-voice: a long make's skill, told ready, was not put on: {why}; its person is told why"
                    );
                    replace(
                        directory,
                        told,
                        Some(State::NotOn {
                            name: made.name,
                            sha256,
                            why,
                            ended_ms: now_ms,
                        }),
                        now_ms,
                    );
                }
                _ => {
                    crate::made_skill::mark(directory, &sha256, READY_UNTOLD, None);
                    take(directory, told, now_ms);
                }
            }
        }
        State::NotOn { sha256, .. } => {
            crate::made_skill::mark(directory, &sha256, READY_UNTOLD, None);
            take(directory, told, now_ms);
        }
        State::NotMade { .. } | State::Making { .. } => {
            take(directory, told, now_ms);
        }
    }
}

/// Whether the turn that finished heard the news through: it is the very
/// turn that told it (`same_turn`), every word played with nobody's turn
/// taken over it (`played_through`, the coordinator's `heard_through`).
pub fn heard_through(same_turn: bool, played_through: bool) -> bool {
    same_turn && played_through
}

/// [`heard`] on the lamp, off the coordinator's loop: the made skills' own
/// folder and manage helper; one with code of its own that goes on at once
/// by that path (`make_skill_local::code_on_at_once`).
pub fn heard_here(told: String) {
    let Some(keeping) = crate::made_skill::Keeping::from_environment() else {
        return;
    };
    let spawned = std::thread::Builder::new()
        .name("lamp-voice-long-make".into())
        .spawn(move || {
            heard(&keeping, &told, crate::app_make::now_unix_ms(), |made| {
                if made.uses.code {
                    crate::make_skill_local::code_on_at_once(&keeping, made).map(|_| ())
                } else {
                    crate::made_skill::put_on(&keeping, made)
                }
            });
        });
    if spawned.is_err() {
        eprintln!("lamp-voice: a long make's news was heard, but no thread was had to act on it");
    }
}

#[cfg(test)]
mod tests;
