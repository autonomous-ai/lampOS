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
//!   [`still_making_line`], nothing made.
//! - **Done:** the same checks and repairs as any agent make. A finished
//!   long make is kept but stays off, `ready_untold` on its `made.json`
//!   entry (what the app reads of made skills), until its person has heard
//!   [`ready_line`] and its read-back, as the lamp's own turn ([`Tells`]):
//!   their face seen, not mid-conversation, never in the quiet hours, never
//!   to an empty room ([`may_tell`]). Only once that turn was played through
//!   ([`heard_through`], [`heard`]) does it go on as a finished make does;
//!   cut, it stays off and is told again. One with code of its own still
//!   waits for the spoken yes, and one that clashes for the clash's choice,
//!   each asked then ([`told`]). Not back today, it is told the next time.
//! - **A failure, the 10 minutes run out, or a restart** while it ran (the
//!   mark [`FILE`] kept `making`, [`after_start`]): one plain line,
//!   [`not_finished_line`], at the next such moment, and nothing kept. Never
//!   DeepSeek's draft.
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
            _ => None,
        }
    }
}

/// The mark in `directory`, as kept; a `making` one past [`MAKE_WITHIN`]
/// and its grace at `now_ms` is read as run out.
pub fn read(directory: &Path, now_ms: u64) -> Option<State> {
    let bytes = std::fs::read(directory.join(FILE)).ok()?;
    let state = State::of_json(&serde_json::from_slice(&bytes).ok()?)?;
    match state {
        State::Making { began_ms, name }
            if now_ms.saturating_sub(began_ms)
                > MAKE_WITHIN.as_millis() as u64 + RAN_OUT_GRACE_MS =>
        {
            Some(State::NotMade {
                name,
                why: NotFinished::RanOut,
                ended_ms: now_ms,
            })
        }
        state => Some(state),
    }
}

/// Keep `state` as the mark in `directory`, written whole and moved over it.
fn write(directory: &Path, state: &State) {
    let kept = std::fs::create_dir_all(directory).and_then(|()| {
        let new = directory.join(format!(".{FILE}.new"));
        std::fs::write(&new, state.to_json().to_string())?;
        std::fs::rename(&new, directory.join(FILE))
    });
    if let Err(error) = kept {
        eprintln!("lamp-voice: a long make's mark was not kept: {error}");
    }
}

/// The mark in `directory` removed: told, or nothing to tell.
fn clear(directory: &Path) {
    let _ = std::fs::remove_file(directory.join(FILE));
}

/// As the second route's worker starts: a make marked `making` was cut by
/// the restart, and its person is told so once ([`not_finished_line`]).
pub fn after_start(directory: &Path, now_ms: u64) {
    if let Some(State::Making { name, .. }) = read(directory, now_ms) {
        eprintln!("lamp-voice: a long make was cut by a restart: its person is told so");
        write(
            directory,
            &State::NotMade {
                name,
                why: NotFinished::Restart,
                ended_ms: now_ms,
            },
        );
    }
}

/// A make the voice may let run on, by its request: under way, let go to
/// run on, or settled before it was let go.
enum Phase {
    Running {
        directory: PathBuf,
        name: Option<String>,
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

/// The voice turn waiting on `request`'s make lets it run on, at `now_ms`:
/// true when it was still under way, its mark then kept `making`; false
/// when it settled already or is no such make (the turn waits for it).
pub fn let_go(request: u64, now_ms: u64) -> bool {
    let mut phases = phases();
    let Some((_, phase)) = phases.iter_mut().find(|(of, _)| *of == request) else {
        return false;
    };
    let Phase::Running { directory, name } = phase else {
        return false;
    };
    let directory = directory.clone();
    write(
        &directory,
        &State::Making {
            began_ms: now_ms,
            name: name.clone(),
        },
    );
    *phase = Phase::LetGo { directory };
    true
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
/// (`Err`, whether it ran out), nothing kept. Its person is told at the
/// next moment ([`Tells`]).
pub fn settled(directory: &Path, made: Result<(&str, &str), bool>, now_ms: u64) {
    let name = match read(directory, now_ms) {
        Some(State::Making { name, .. }) => name,
        _ => None,
    };
    match made {
        Ok((name, sha256)) => {
            crate::made_skill::mark(directory, sha256, READY_UNTOLD, Some(json!(true)));
            eprintln!("lamp-voice: a long make was made: kept off until its person hears it");
            write(
                directory,
                &State::Made {
                    name: name.to_owned(),
                    sha256: sha256.to_owned(),
                },
            );
        }
        Err(ran_out) => {
            eprintln!(
                "lamp-voice: a long make made nothing ({}): its person is told so",
                if ran_out { "ran out" } else { "failed" }
            );
            write(
                directory,
                &State::NotMade {
                    name,
                    why: if ran_out {
                        NotFinished::RanOut
                    } else {
                        NotFinished::Failed
                    },
                    ended_ms: now_ms,
                },
            );
        }
    }
}

/// What a make ask is told while a long make runs on in `directory` at
/// `now_ms`; none when none does.
pub fn still_making(directory: &Path, now_ms: u64) -> Option<Value> {
    let Some(State::Making { name, .. }) = read(directory, now_ms) else {
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

/// "Is my skill ready?": the long make's state in `directory` at `now_ms`,
/// for `make_skill`'s list; names only when `names` (the owner hears).
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
        None => json!({"state": "none_being_made"}),
    }
}

/// What the model is told to do with [`status`].
pub const STATUS_NEXT: &str = "If they asked whether a skill they asked for is ready, answer \
     truthfully from long_make: made and ready (it is told and read back in a moment), still \
     being made (for how long so far), or not made.";

/// Whether a long make's news may be told now: its person's face seen
/// (`owner_seen`), not mid-conversation (`quiet`), not the quiet hours
/// (`night`), and someone at the lamp (`someone_there`).
pub fn may_tell(owner_seen: bool, quiet: bool, night: bool, someone_there: bool) -> bool {
    owner_seen && quiet && !night && someone_there
}

/// Whether the owner's own face is in view now, whatever room it is: the
/// eyes' known face one of the owner's memory ids.
pub fn owner_seen_now() -> bool {
    crate::known_faces::known_from_environment()
        .map(|name| crate::known_faces::memory_person(&name))
        .is_some_and(|seen| crate::person::owner_ids().contains(&seen))
}

/// The coordinator's side: the mark read off the made skills' folder, and
/// the telling offered last.
pub struct Tells {
    directory: PathBuf,
    read_at: Option<Instant>,
    state: Option<State>,
    offered: Option<(String, Instant)>,
}

impl Tells {
    /// With the switch on and made skills kept here, the product's folder.
    pub fn from_environment() -> Option<Self> {
        let keeping = crate::made_skill::Keeping::from_environment()?;
        on().then(|| Self::at(keeping.directory))
    }

    /// Reading the mark in `directory`.
    pub fn at(directory: PathBuf) -> Self {
        Self {
            directory,
            read_at: None,
            state: None,
            offered: None,
        }
    }

    /// The written turn's note that tells the news due at `now` and
    /// `now_ms`, once per [`OFFER_AGAIN_AFTER`]; none with nothing to tell.
    /// Asked only once [`may_tell`] holds.
    pub fn next(&mut self, now: Instant, now_ms: u64) -> Option<String> {
        if self
            .read_at
            .is_none_or(|at| now.duration_since(at) >= READ_EVERY)
        {
            self.read_at = Some(now);
            self.state = read(&self.directory, now_ms);
        }
        let key = key(self.state.as_ref()?)?;
        if self.offered.as_ref().is_some_and(|(offered, at)| {
            *offered == key && now.duration_since(*at) < OFFER_AGAIN_AFTER
        }) {
            return None;
        }
        self.offered = Some((key.clone(), now));
        Some(format!(
            "{OPENS}{key}] Say only, in a few words: {})",
            match self.state.as_ref()? {
                State::Made { name, .. } => ready_line(name),
                State::NotMade { name, .. } => not_finished_line(name.as_deref()),
                State::Making { .. } => return None,
            }
        ))
    }
}

/// A telling's key: `ready <sha256>` or `not_made <ended_ms>`.
fn key(state: &State) -> Option<String> {
    match state {
        State::Made { sha256, .. } => Some(format!("ready {sha256}")),
        State::NotMade { ended_ms, .. } => Some(format!("not_made {ended_ms}")),
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

/// A written turn `request`'s `text` on the second route, with the switch
/// on: a long make's telling ([`Tells::next`]), the model given what to
/// say; nothing goes on and nothing is cleared here, only once the turn was
/// heard whole ([`heard`]). Ready: [`ready_line`] and its read-back, with
/// the link ask for a service not linked yet (`linked`, as a finished make
/// is told); with code of its own, its yes awaited from this request on,
/// as an app make's is; clashing with a skill on the lamp, the clash's
/// question asked ([`crate::made_clash::asked`]), as a finished make's is.
/// Not made: [`not_finished_line`]. Any other text as it is.
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
    let state = read(directory, now_ms);
    if state.as_ref().and_then(key).as_deref() != Some(asked) {
        // Told already, or another's: nothing more to say.
        return "(Nobody has spoken. Say nothing, and call nothing.)".to_owned();
    }
    match state {
        Some(State::Made { name, sha256 }) => {
            let Some(made) = crate::made_skill::kept_by_sha256(directory, &sha256) else {
                eprintln!("lamp-voice: request {request}: a long make's skill is gone: told so");
                return say_only(&not_finished_line(Some(&name)));
            };
            let ready = ready_line(&made.name);
            let link = crate::made_skill::needs_link(&made.uses, linked)
                .as_deref()
                .and_then(crate::made_skill::needs_link_ask)
                .map(|ask| format!(" {ask}"))
                .unwrap_or_default();
            if made.uses.code {
                let id = crate::skill_write_yes::wait_for_yes_noted(
                    crate::make_skill_local::code_asks(&made),
                    request,
                    now,
                );
                eprintln!(
                    "lamp-voice: request {request}: a long make's skill with code is told ready; its yes is awaited"
                );
                return format!(
                    "{} {}",
                    say_only(&format!(
                        "{ready} {}{link}",
                        crate::made_skill::on_line(&made)
                    )),
                    crate::skill_write_yes::then_wait(&id)
                );
            }
            let clashes = crate::made_clash::found(keeping, &made);
            if !clashes.is_empty() {
                let asked = crate::made_clash::asked(keeping, &made, &clashes, request, now);
                eprintln!(
                    "lamp-voice: request {request}: a long make's skill is told ready; it clashes, so the person is asked"
                );
                return format!(
                    "(Nobody has spoken. Say first, in the language of the conversation: \
                     \"{ready}\"{link} Then: {})",
                    asked["next"].as_str().unwrap_or_default()
                );
            }
            eprintln!(
                "lamp-voice: request {request}: a long make's skill is told ready and read back; it goes on once heard whole"
            );
            say_only(&format!(
                "{ready} {}{link}",
                crate::made_skill::on_line(&made)
            ))
        }
        Some(State::NotMade { name, why, .. }) => {
            eprintln!(
                "lamp-voice: request {request}: a long make that made nothing ({}) is told",
                why.word()
            );
            say_only(&not_finished_line(name.as_deref()))
        }
        _ => text,
    }
}

/// [`told`] on the lamp: the made skills' own folder, what is linked as
/// a voice make reads it.
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

/// The turn that told the news `key` was heard whole (the coordinator's
/// proof a say's `spoken` takes): its person knows. A ready skill with no
/// code and no clash goes on now (`put_on`, as a finished make goes on);
/// one with code waits for its yes and one that clashes for the person's
/// choice, as asked in that turn. `ready_untold` is taken off and the mark
/// cleared, so it is told once. A turn cut or not heard whole calls
/// nothing: the skill stays off, untold, and is told again next time.
pub fn heard(
    keeping: &crate::made_skill::Keeping,
    told: &str,
    now_ms: u64,
    put_on: impl FnOnce(&crate::made_skill::Made) -> Result<(), String>,
) {
    let directory = &keeping.directory;
    let state = read(directory, now_ms);
    if state.as_ref().and_then(key).as_deref() != Some(told) {
        return;
    }
    if let Some(State::Made { sha256, .. }) = &state {
        if let Some(made) = crate::made_skill::kept_by_sha256(directory, sha256)
            && !made.uses.code
            && crate::made_clash::found(keeping, &made).is_empty()
            && let Err(why) = put_on(&made)
        {
            eprintln!("lamp-voice: a long make's skill, told ready, was not put on: {why}");
        }
        crate::made_skill::mark(directory, sha256, READY_UNTOLD, None);
    }
    eprintln!("lamp-voice: a long make's news was heard whole: told once");
    clear(directory);
}

/// Whether the turn that finished heard the news through: it is the very
/// turn that told it (`same_turn`), every word played with nobody's turn
/// taken over it (`played_through`, the coordinator's `heard_through`).
pub fn heard_through(same_turn: bool, played_through: bool) -> bool {
    same_turn && played_through
}

/// [`heard`] on the lamp, off the coordinator's loop: the made skills' own
/// folder and manage helper.
pub fn heard_here(told: String) {
    let Some(keeping) = crate::made_skill::Keeping::from_environment() else {
        return;
    };
    let spawned = std::thread::Builder::new()
        .name("lamp-voice-long-make".into())
        .spawn(move || {
            heard(&keeping, &told, crate::app_make::now_unix_ms(), |made| {
                crate::made_skill::put_on(&keeping, made)
            });
        });
    if spawned.is_err() {
        eprintln!("lamp-voice: a long make's news was heard, but no thread was had to act on it");
    }
}

#[cfg(test)]
mod tests;
