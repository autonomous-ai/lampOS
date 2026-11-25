//! A voice make handed to the lamp's own Hermes in a skill run's namespace
//! (#6643, the first builder's guard, 12 Nov 18:15): the voice's make that
//! DeepSeek couldn't keep (`lamp_voice::make_skill_agent`,
//! `LAMP_MAKE_SKILL_AGENT`) is written by Hermes started exactly as a
//! skill's `do.task` run is ([`crate::skill_task::HermesRunner`],
//! row-6443's path), through its namespace wrapper: only its own folder,
//! its work folder and the skills, lamp-control's `control.sock` alone and
//! never `admin.sock`. Its terminal and tools work only inside that
//! namespace. No token is minted: a make has no grants. Hermes only
//! ([`MakeHermes`], the first builder's ruling, 12 Nov 19:14): only Hermes
//! has a namespace runner today. Held or not, the wrapper and the namespace
//! script must carry lamp-update's make-home lines ([`WRAPPER_HOME_LINE`],
//! [`NS_HOME_LINE`]), or, with lamp-update's one run path, the one run
//! path's ([`ONE_PATH_HOME_LINE`], [`ONE_PATH_NS_MAKE_LINES`]), or the make
//! is refused [`NO_NAMESPACE`].
//!
//! Each make has a fresh home of its own, `make-<run id>/home`, 0700, as
//! `HOME` and `HERMES_HOME`: empty but for the provider config (its key
//! named by its variable, never a key) and two links to Hermes's prepared
//! runtime ([`RUNTIME_LINKS`]), so a make never prepares dependencies of
//! its own; never Hermes's memory, sessions or state. It goes with the
//! make. lamp-update's namespace script hides
//! Hermes's lasting home from a make (an empty read-only tmpfs over it,
//! only the checkout, its Python tools and its dependency record shown
//! again, read-only) and the rest of the work
//! folder's `lamp` (only the make's folder shown, read-write), and leaves
//! Hermes's folder and the work folder read-only.
//!
//! Its network ([`NET_SWITCH`], the owner's ask, 12 Nov): kept by default,
//! as V1's agent had it. `LAMP_MAKE_AGENT_NET=held` exactly gives the
//! strict run, [`crate::skill_task::NET_ENV`] `run`: a network namespace of
//! its own with only `lo` (and the kernel's `@NONE` tunnels), the model
//! only through [`crate::net_bridge::MODEL_SOCKET`]; refused when the
//! wrapper can't hold it off ([`crate::skill_task::NOT_HELD_OFF`]).
//!
//! - **The line** ([`MakeLine`]): `{"op":"skill_make","rules","within_ms"}`,
//!   the system prompt and the brief as one text, and how long the run may
//!   take (at most [`MOST_WITHIN`], 10 minutes since #7245; up to [`RUN_ON_MOST_WITHIN`], 10
//!   minutes, for a make marked `"may_run_on":true`, lamp-voice's
//!   `LAMP_MAKE_MAY_RUN_ON`, #6811), checked whole ([`MakeLine::parse`]);
//!   with `"guide":true` too
//!   (lamp-voice's `LAMP_MAKE_AGENT_GUIDE`), the lamp's skill-creator guide
//!   is given ([`GUIDE_FILE`]).
//! - **The guide** ([`GUIDE`], only when the line asks for it): a lamp
//!   version of V1's `skill-creator` (Apache-2.0), written 0600 beside the
//!   rules as [`GUIDE_FILE`]: take the intent from the person's words, draft
//!   the skill in `draft/NAME` in the make's folder, check it with the
//!   lamp's own check, `lamp-skills pack` ([`CHECK_PROGRAM`], the very
//!   check the voice packs a made skill with, its output in the run's own
//!   `/tmp`), fix it at most twice by the check's words, then answer
//!   ([`guided_prompt`]). Without it the run's words and files are as
//!   before, byte for byte ([`prompt`]).
//! - **The check's switches** (`"switches"`, only with `"guide":true`,
//!   Heron's P2 on #6768): the run is started with a cleared environment,
//!   so the guide's `lamp-skills pack` would load the draft with none of
//!   the lamp's switches and refuse a step the voice's own check (run with
//!   lamp-voice's) accepts. The line carries lamp-voice's values of the
//!   switches `lamp-skills` reads as it loads a skill to pack it,
//!   [`CHECK_SWITCHES`] alone, each `"on"` or `"off"`
//!   ([`CheckSwitches`]); each `on` is put in the run's environment, as
//!   lamp-voice's worker has it, so the two checks agree.
//! - **The make's folder**: `make-<run id>` in the lamp's work folder (the
//!   one a skill's run starts in, shown to Hermes in its namespace), 0700,
//!   with the rules in [`RULES_FILE`], 0600. The run starts there and is
//!   told to write its answer to [`ANSWER_FILE`] there and nothing else.
//!   The answer is read back with `openat` from the folder's own
//!   descriptor, `O_NOFOLLOW` (a regular file, never a link, at most
//!   [`MOST_ANSWER_BYTES`]) once the run has ended and its whole process
//!   group is gone, and the folder removed however the make ends. The
//!   voice puts the answer through the same checks as DeepSeek's. A
//!   gateway make with no answer file takes the agent's final chat answer
//!   instead, only when it is a make's answer ([`make_answer`], #7427).
//! - **The answers**: `{"ok":true,"started":true}` once the run is about
//!   to start (so the voice says its waiting line only then), then
//!   `{"ok":true,"answer"}`; or one refusal: `off`, `bad_request`,
//!   `too_long`, `busy`, `not_held_off`, `no_namespace`, `run_failed` or
//!   `no_answer`.
//! - **The socket** ([`MakeHandOvers`]): with `LAMP_SKILL_TASKS` on (the
//!   switch lamp-update's namespace edits are written with), `serve` binds
//!   [`MAKE_SOCKET`] for lamp-voice alone, as root, one make at a time,
//!   sweeping old make folders first ([`sweep`]). It is not bound only
//!   when the hand-over is on: that switch, `LAMP_MAKE_SKILL_AGENT`, is
//!   lamp-voice's alone (set in its unit, which the unit census keeps), so
//!   lamp-tasks can't see it.
use crate::protocol::refusal;
use crate::skill_task::{
    NOT_HELD_OFF, NOT_HELD_OFF_ONE_PATH, Runner, SkillRun, read_line_within, run_id, write_answer,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

/// lamp-tasks' make socket, beside its others in its own runtime folder
/// (`lampos`'s, 0700), 0600: lamp-voice, run as root, reaches it as it does
/// `request.sock` (`LAMP_TASKS_DIR`).
pub const MAKE_SOCKET: &str = "/run/lampos-tasks/skill-make.sock";
/// The one unit whose lines are taken, run as root.
pub const CALLER_UNIT: &str = "lamp-voice.service";
/// The most bytes of the rules: the system prompt and the brief.
pub const MOST_RULES_BYTES: usize = 48 * 1024;
/// The most bytes of a line: the rules, every byte escaped at worst once.
pub const MOST_LINE_BYTES: usize = 2 * MOST_RULES_BYTES + 256;
/// The most bytes of an answer read back, as the voice's own model's.
pub const MOST_ANSWER_BYTES: u64 = 20 * 1024;
/// The least and most a run may be given (`within_ms`).
pub const LEAST_WITHIN: Duration = Duration::from_secs(1);
// #7245: a gateway agent's make may take up to 10 minutes (the first
// builder's call on #6811, 18 Nov 17:23, the owner's yes).
pub const MOST_WITHIN: Duration = Duration::from_secs(10 * 60);
/// The most a make marked "may run on" may be given (`"may_run_on":true`,
/// lamp-voice's `LAMP_MAKE_MAY_RUN_ON`, #6811): ten minutes, the same as
/// [`MOST_WITHIN`] since #7245.
pub const RUN_ON_MOST_WITHIN: Duration = Duration::from_secs(600);
/// The rules' file in a make's folder.
pub const RULES_FILE: &str = "rules.md";
/// The one file the run writes in a make's folder.
pub const ANSWER_FILE: &str = "answer.md";
/// How a make's folder's name begins.
pub const FOLDER_HEAD: &str = "make-";
/// Said when the run ended with no answer to read.
pub const NO_ANSWER: &str = "the lamp's Hermes wrote no answer";
/// What a second make is told while one runs.
pub const BUSY: &str = "a skill is already being made on this lamp; one is made at a time";
/// How long a connection has to send its line, and to take each answer.
pub const LINE_WAIT: Duration = Duration::from_secs(5);
/// The most connections read or answered at once: a make, the lamp's own
/// news ask and a person's ask that cancels it (`crate::voice_ask`).
pub const MOST_CONNECTIONS: usize = 3;
/// How often the socket is looked at while no one connects.
const ACCEPT_TICK: Duration = Duration::from_millis(20);

/// One make handed over by the voice, as its line says it.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MakeLine {
    pub op: MakeOp,
    pub rules: String,
    pub within_ms: u64,
    /// Whether the lamp's skill-creator guide is given ([`GUIDE_FILE`]):
    /// only when the line says `true`.
    #[serde(default)]
    pub guide: bool,
    /// The voice's values of the check's switches ([`CheckSwitches`]),
    /// only beside `"guide":true`; `null` is not one.
    #[serde(default, deserialize_with = "given")]
    pub switches: Option<CheckSwitches>,
    /// #7245: lamp-voice's `LAMP_MAKE_AGENT_ONE_PATH` on, so the make goes
    /// to the lamp's default agent as its data says ([`MakeGateway`]):
    /// only when the line says `true`. Without it, the lamp's Hermes in
    /// its namespace ([`MakeHermes`]) as before, byte for byte.
    #[serde(default)]
    pub one_path: bool,
    /// #7245: the lamp's agent as the voice saw it (`"Hermes"`,
    /// `"OpenClaw"`), sent beside `"one_path":true`. A make that would
    /// fall back to Hermes on a lamp whose agent isn't Hermes is told
    /// [`NOT_READY_LINE`] instead (Harness hasn't looked yet).
    #[serde(default)]
    pub engine: Option<String>,
    /// #6811: lamp-voice's `LAMP_MAKE_MAY_RUN_ON` on, so the voice let the
    /// person go after a minute and the make may run on: its `within_ms`
    /// may be up to [`RUN_ON_MOST_WITHIN`]. Only when the line says `true`;
    /// without it, [`MOST_WITHIN`] as before.
    #[serde(default)]
    pub may_run_on: bool,
}

/// The line's `switches` when given: an object, never `null`.
fn given<'de, D: serde::Deserializer<'de>>(switches: D) -> Result<Option<CheckSwitches>, D::Error> {
    CheckSwitches::deserialize(switches).map(Some)
}

/// The switches `lamp-skills` reads as it loads a skill folder to pack it,
/// so that change what the guide's check refuses, and the only ones a
/// make's line may carry: `LAMP_AGENT_SERVICES` (a `do.task`'s linked
/// service grants, `lamp_skills::task::AGENT_SERVICES_ENVIRONMENT`) and
/// `LAMP_SKILL_WRITES` (a `write` step or a `"write"` permission,
/// `lamp_skills::service_write::ENVIRONMENT`). The others it reads at
/// `pack` change no verdict (`LAMP_NUDGES_PRESENT`: both forms are checked)
/// or are read only as a skill runs or goes on (`LAMP_SKILL_TASKS`,
/// `LAMP_SKILL_READS`, `LAMP_MADE_SCRIPTS`). lamp-skills' and lamp-voice's
/// tests hold this list to their own names.
pub const CHECK_SWITCHES: [&str; 2] = ["LAMP_AGENT_SERVICES", "LAMP_SKILL_WRITES"];

/// The asks switch the agent's check is given when lamp-tasks has it on
/// ([`gateway_checked_prompt`], #7245).
pub const ASKS_ENVIRONMENT: &str = "LAMP_MADE_SKILL_ASKS";

/// One of the check's switches as the line gives it: `"on"` or `"off"`,
/// nothing else.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Switch {
    On,
    #[default]
    Off,
}

/// The check's switches a make's line carries ([`CHECK_SWITCHES`]): any
/// other name, a name twice or a value but `"on"` or `"off"` is no make's
/// line; one not given is off.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CheckSwitches {
    #[serde(rename = "LAMP_AGENT_SERVICES", default)]
    pub agent_services: Switch,
    #[serde(rename = "LAMP_SKILL_WRITES", default)]
    pub skill_writes: Switch,
}

impl CheckSwitches {
    /// The run's environment lines for them: `NAME=on` for each one on,
    /// none for one off, as lamp-voice's worker is given only an exact
    /// `on`.
    pub fn env(&self) -> Vec<(&'static str, String)> {
        [
            (CHECK_SWITCHES[0], self.agent_services),
            (CHECK_SWITCHES[1], self.skill_writes),
        ]
        .into_iter()
        .filter(|(_, switch)| *switch == Switch::On)
        .map(|(name, _)| (name, "on".to_owned()))
        .collect()
    }
}

/// The line's `op`: `skill_make` only.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MakeOp {
    SkillMake,
}

impl MakeLine {
    /// The line, checked whole: the refusal's code and words otherwise.
    pub fn parse(line: &[u8]) -> Result<Self, (&'static str, String)> {
        if line.len() > MOST_LINE_BYTES {
            return Err(("too_long", "the line is too long".into()));
        }
        let make: Self = serde_json::from_slice(line)
            .map_err(|_| ("bad_request", "not a make's line".to_owned()))?;
        if make.rules.trim().is_empty() || make.rules.len() > MOST_RULES_BYTES {
            return Err(("bad_request", "the rules are empty or too long".into()));
        }
        if make.rules.contains('\0') {
            return Err(("bad_request", "the rules have a NUL".into()));
        }
        let most = if make.may_run_on {
            RUN_ON_MOST_WITHIN
        } else {
            MOST_WITHIN
        };
        if !(LEAST_WITHIN..=most).contains(&make.within()) {
            return Err(("bad_request", "within_ms is out of its bounds".into()));
        }
        // The switches are for the guide's check alone: without the guide
        // the run is told to run nothing.
        if make.switches.is_some() && !make.guide {
            return Err((
                "bad_request",
                "switches are given only with the guide".into(),
            ));
        }
        Ok(make)
    }

    pub fn within(&self) -> Duration {
        Duration::from_millis(self.within_ms)
    }
}

/// The run's words: where the rules are and the one file to write.
pub fn prompt(folder: &Path) -> String {
    format!(
        "Everything you need is in the file {rules} in this folder, {folder}. Read it and \
         answer exactly as it says: the fenced blocks, or its one line when the skill can't be \
         made. Write that answer, and nothing else, to the file {answer} in this folder. Write \
         no other file anywhere, install nothing and run nothing else.",
        rules = RULES_FILE,
        answer = ANSWER_FILE,
        folder = folder.display(),
    )
}

/// The guide's file in a make's folder, beside the rules, only when the
/// line asks for it.
pub const GUIDE_FILE: &str = "guide.md";
/// The lamp's skill-creator guide, as [`GUIDE_FILE`] holds it: a lamp
/// version of V1's `skills/skill-creator/SKILL.md` (Apache-2.0; its first
/// line names that origin and license), without V1's evals, benchmarks and
/// viewer.
pub const GUIDE: &str = include_str!("skill_make_guide.md");

/// #7245: the skill format's whole reference, as a gateway agent's make
/// folder holds it ([`FORMAT`]): everything a skill can do, read with the
/// agent's own read tool, so it costs nothing against Harness's task
/// bound.
pub const FORMAT_FILE: &str = "format.md";
/// `docs/skill-format.md` as this build has it: always the build's own.
pub const FORMAT: &str = include_str!("../../../docs/skill-format.md");
/// #7245: the lamp's Python SDK reference in a gateway agent's make folder,
/// so a skill's code may use any of its calls, not only the few the rules
/// name.
pub const SDK_FILE: &str = "sdk.md";
/// `sdk/python/README.md` as this build has it.
pub const SDK: &str = include_str!("../../../sdk/python/README.md");
/// #7245: the folder of complete example skills in a gateway agent's make
/// folder ([`MAKE_EXAMPLES`]).
pub const EXAMPLES_FOLDER: &str = "examples";
/// #7245: complete example skills, each file's path under
/// [`EXAMPLES_FOLDER`] and its text: a light and a say on its own words, a
/// light show of three colours in turn, a task with a script that
/// keeps a note, a say again every 30 minutes while someone stays, and a
/// reminder set in the lamp's schedule, and a script that leads breaths
/// with the light and the voice, and a reminder every 30 minutes started
/// by words; a linked-service read started by words (its helper script
/// reads the calendar, the lamp's agent sums it up; #8157), and `run`
/// steps whose script does the whole job and says it (a coin, breaths, a
/// `net` script that fetches one public value); and a `net` task a time
/// starts, whose script only prints for the told answer (#8610).
/// lamp-skills' `tests/it/make_check.rs` packs each with the real
/// `lamp-skills pack`, so none can be one the check refuses.
pub const MAKE_EXAMPLES: [(&str, &str); 28] = [
    (
        "desk-light/skill.toml",
        include_str!("make_examples/desk-light/skill.toml"),
    ),
    (
        "desk-light/SKILL.md",
        include_str!("make_examples/desk-light/SKILL.md"),
    ),
    (
        "stretch-break/skill.toml",
        include_str!("make_examples/stretch-break/skill.toml"),
    ),
    (
        "stretch-break/SKILL.md",
        include_str!("make_examples/stretch-break/SKILL.md"),
    ),
    (
        "coin-flip/skill.toml",
        include_str!("make_examples/coin-flip/skill.toml"),
    ),
    (
        "coin-flip/scripts/main.py",
        include_str!("make_examples/coin-flip/scripts/main.py"),
    ),
    (
        "coin-flip/SKILL.md",
        include_str!("make_examples/coin-flip/SKILL.md"),
    ),
    (
        "water-while-here/skill.toml",
        include_str!("make_examples/water-while-here/skill.toml"),
    ),
    (
        "water-while-here/SKILL.md",
        include_str!("make_examples/water-while-here/SKILL.md"),
    ),
    (
        "break-timer/skill.toml",
        include_str!("make_examples/break-timer/skill.toml"),
    ),
    (
        "break-timer/SKILL.md",
        include_str!("make_examples/break-timer/SKILL.md"),
    ),
    (
        "breathing-coach/skill.toml",
        include_str!("make_examples/breathing-coach/skill.toml"),
    ),
    (
        "breathing-coach/SKILL.md",
        include_str!("make_examples/breathing-coach/SKILL.md"),
    ),
    (
        "breathing-coach/scripts/main.py",
        include_str!("make_examples/breathing-coach/scripts/main.py"),
    ),
    (
        "exchange-rate/skill.toml",
        include_str!("make_examples/exchange-rate/skill.toml"),
    ),
    (
        "exchange-rate/SKILL.md",
        include_str!("make_examples/exchange-rate/SKILL.md"),
    ),
    (
        "exchange-rate/scripts/main.py",
        include_str!("make_examples/exchange-rate/scripts/main.py"),
    ),
    (
        "water-breaks/skill.toml",
        include_str!("make_examples/water-breaks/skill.toml"),
    ),
    (
        "water-breaks/SKILL.md",
        include_str!("make_examples/water-breaks/SKILL.md"),
    ),
    (
        "agenda-count/skill.toml",
        include_str!("make_examples/agenda-count/skill.toml"),
    ),
    (
        "agenda-count/SKILL.md",
        include_str!("make_examples/agenda-count/SKILL.md"),
    ),
    (
        "agenda-count/scripts/main.py",
        include_str!("make_examples/agenda-count/scripts/main.py"),
    ),
    (
        "sun-times/skill.toml",
        include_str!("make_examples/sun-times/skill.toml"),
    ),
    (
        "sun-times/SKILL.md",
        include_str!("make_examples/sun-times/SKILL.md"),
    ),
    (
        "sun-times/scripts/main.py",
        include_str!("make_examples/sun-times/scripts/main.py"),
    ),
    (
        "morning-headline/skill.toml",
        include_str!("make_examples/morning-headline/skill.toml"),
    ),
    (
        "morning-headline/SKILL.md",
        include_str!("make_examples/morning-headline/SKILL.md"),
    ),
    (
        "morning-headline/scripts/main.py",
        include_str!("make_examples/morning-headline/scripts/main.py"),
    ),
];

/// [`MAKE_EXAMPLES`] written under [`EXAMPLES_FOLDER`] in `at`, each folder
/// 0700 and each file 0600, never through a link.
fn examples_in(at: &rustix::fd::OwnedFd) -> std::io::Result<()> {
    let examples = folder_in(at, EXAMPLES_FOLDER)?;
    let mut made: Vec<(String, rustix::fd::OwnedFd)> = Vec::new();
    for (path, text) in MAKE_EXAMPLES {
        let (folder, name) = path.rsplit_once('/').unwrap_or(("", path));
        let mut fd = None;
        let mut so_far = String::new();
        for part in folder.split('/').filter(|part| !part.is_empty()) {
            so_far.push_str(part);
            so_far.push('/');
            let parent = fd.as_ref().unwrap_or(&examples);
            let next = match made.iter().position(|(done, _)| *done == so_far) {
                Some(at) => made[at].1.try_clone()?,
                None => {
                    let opened = folder_in(parent, part)?;
                    made.push((so_far.clone(), opened.try_clone()?));
                    opened
                }
            };
            fd = Some(next);
        }
        file_in(fd.as_ref().unwrap_or(&examples), name, text)?;
    }
    Ok(())
}
/// The lamp's own check of a skill's folder the guide names: the skills
/// part's `pack`, the very check the voice packs a made skill with
/// (`lamp_voice::made_skill::PROGRAM`), seen inside the make's namespace
/// as it is outside (`/opt` is neither hidden nor written to). It reads the
/// folder and writes its package only under `--out`, the run's own `/tmp`.
pub const CHECK_PROGRAM: &str = "/opt/lampOS/skills/current/lamp-skills";
/// Where the guide's check writes its package: the namespace's own `/tmp`,
/// gone with the run.
pub const CHECK_OUT: &str = "/tmp/lamp-check";

/// The run's words with the guide ([`GUIDE_FILE`]): the guide read first,
/// the rules winning where they differ, the draft checked with
/// [`CHECK_PROGRAM`] alone and fixed at most twice, then the answer
/// written as the rules say, to [`ANSWER_FILE`] alone.
pub fn guided_prompt(folder: &Path) -> String {
    guided_prompt_with(folder, "")
}

/// [`guided_prompt`], the check's command after `before` (empty for
/// Hermes's make, byte for byte; a gateway make's `cd` and switches,
/// [`gateway_checked_prompt`]).
fn guided_prompt_with(folder: &Path, before: &str) -> String {
    format!(
        "Everything you need is in two files in this folder, {folder}: {guide}, how to make a \
         lamp skill, and {rules}, the skill's form and the person's request. Read {guide} first, \
         then {rules}, and follow both; {rules} wins where they differ. Draft the skill in \
         {folder}/draft/NAME and check it with the lamp's own check, run from this folder as \
         {before}{check} pack --out {out} draft/NAME, fixing what it says at most twice. Then write the \
         answer exactly as {rules} says (the fenced blocks, or its one line when the skill can't \
         be made), and nothing else, to the file {answer} in this folder. Write no file outside \
         this folder but the check's own under {out}, install nothing and run no other command.",
        guide = GUIDE_FILE,
        rules = RULES_FILE,
        answer = ANSWER_FILE,
        check = CHECK_PROGRAM,
        out = CHECK_OUT,
        folder = folder.display(),
    )
}

/// #7245: a gateway make's words with the check, V1's tools on
/// ([`MakeGateway::checked`]): [`guided_prompt`]'s, the check's command
/// run as `cd <folder> && [switches] lamp-skills pack ...`, since an
/// OpenClaw exec starts in the agent's workspace, not the make's folder,
/// and the line's check switches ([`CheckSwitches::env`]) can't reach the
/// gateway's environment otherwise. In OpenClaw's namespace `/opt` is not
/// hidden, so [`CHECK_PROGRAM`] runs as in Hermes's; its package goes to
/// the namespace's own writable `/tmp` ([`CHECK_OUT`]).
pub fn gateway_checked_prompt(folder: &Path, switches: &CheckSwitches) -> String {
    let mut before = format!("cd {} && ", folder.display());
    for (name, value) in switches.env() {
        before.push_str(&format!("{name}={value} "));
    }
    // #7245 (lamp-c0a4, 18 Nov): the agent's own check must take a skill
    // started by its own words (`asks`) as the voice's keep does, which
    // lamp-skills allows only with LAMP_MADE_SKILL_ASKS on; lamp-tasks
    // carries it in its unit when the product has asks on.
    if std::env::var(ASKS_ENVIRONMENT).is_ok_and(|value| value == "on") {
        before.push_str(&format!("{ASKS_ENVIRONMENT}=on "));
    }
    // #7742: with a gateway run's own way out on, a made skill's code may be
    // granted `net`, which lamp-skills' check takes only with LAMP_TASK_NET
    // on, so the agent's check is given it as the run would be.
    let on = |name: &str| std::env::var(name).is_ok_and(|value| value == "on");
    if on(crate::skill_task::NET_SWITCH) && on(crate::skill_task::GATEWAY_NET_SWITCH) {
        before.push_str(&format!("{}=on ", crate::skill_task::NET_SWITCH));
    }
    // The skill.toml form inline too, as in [`gateway_prompt`].
    // #7245 (lamp-c0a4, 18 Nov 13:27): Harness's `new` refused the checked
    // words at ~2,030 bytes (its own task cap is 2,000), so the form is
    // left to guide.md and examples/, which this make's folder holds.
    format!("{GATEWAY_CAN}\n\n{}", guided_prompt_with(folder, &before))
}

/// #7245: the guide as a gateway agent's make is given it without V1's
/// tools ([`MakeGateway::checked`] off): [`GUIDE`] without its check,
/// section 3: with only the skill-start launcher in OpenClaw's exec
/// allow-list, the guide's `lamp-skills pack` ([`CHECK_PROGRAM`]) can't
/// run there. The voice's own checks and repairs of the answer still
/// apply.
pub fn gateway_guide() -> String {
    match (GUIDE.find("## 3. "), GUIDE.find("## 4. ")) {
        (Some(check), Some(answer)) if check < answer => format!(
            "{}{}",
            &GUIDE[..check],
            GUIDE[answer..].replacen("## 4. ", "## 3. ", 1)
        ),
        _ => without_check(GUIDE),
    }
}

/// `text` without any line naming the lamp's own check ([`CHECK_PROGRAM`]
/// or `lamp-skills pack`), which a gateway agent's make can't run
/// (#7245): the rules as a gateway make is given them, the rest byte for
/// byte.
pub fn without_check(text: &str) -> String {
    text.split_inclusive('\n')
        .filter(|line| !line.contains(CHECK_PROGRAM) && !line.contains("lamp-skills pack"))
        .collect()
}

/// #7245: the most bytes of [`gateway_form`], the form a gateway make's
/// words carry inline: a short block, so the words stay well within
/// Harness's task bound (`harness::MAX_PROMPT_BYTES`, 3,000 since #7245).
pub const MOST_FORM_BYTES: usize = 1024;

/// #7245: the one minimal `skill.toml` a gateway make's words show
/// ([`gateway_form`]): `asks` on the first line, then `kind`, `version`,
/// `[permissions]` and one `[[do]]` step, a skill started by the person's
/// own words alone. lamp-skills' `tests/it/make_check.rs` packs it with the
/// real `lamp-skills pack`. Given back in chat, it is refused as the rules'
/// example is ([`is_example`]).
pub const FORM_TOML: &str = "asks = [\"desk glow on\"]
kind = \"installable\"
version = \"1.0.0\"

[permissions]
light = true

[[do]]
light = { colour = \"warm\" }
";

/// #7245: skill.toml's order, in one line, by its real key names: `asks`
/// first, then `kind`, `version`, `[permissions]`, then the trigger,
/// `[ask]` and `[[do]]`. The rest (grants, triggers, steps, limits) stays
/// in the rules alone.
pub const FORM_ORDER: &str = "The skill.toml form in short; rules.md has the rest (grants, \
     triggers, steps, limits). Its order: asks = [...] first, at the very top (only when \
     rules.md offers asks); then kind, version, grants = [...] (only for its own code) and any other \
     top-level key rules.md gives; then [permissions] (only for its steps); then [trigger], [ask] and [[do]], each as rules.md says.";

/// #7245: the one rule a gateway make's words state (lamp-c0a4, 18 Nov
/// 04:35 and 04:40: OpenClaw wrote `asks` after `[permissions]`, and the
/// skill was refused on "unknown field").
pub const FORM_RULE: &str = "Never write asks under [permissions] or any other table: a key \
     after a table's header belongs to that table, so every top-level key goes before the first \
     table.";

/// #7245: the skill.toml form a gateway agent's make carries in its own
/// words, so OpenClaw has it even if it never opens the rules: the order
/// ([`FORM_ORDER`]), the rule ([`FORM_RULE`]) and one minimal skill.toml
/// ([`FORM_TOML`]), at most [`MOST_FORM_BYTES`].
/// #7245: the gateway make's first words, before what to read: the lamp's
/// own OpenClaw answered `cannot:` on 4 of 5 makes on lamp-c0a4 (18 Nov,
/// deepseek-flash), each with a reason of its own ("knowing facts about
/// apples on my own", "writing notes and reading them back"), while the
/// same rules.md sent to the same model alone made the skill 10 of 10. So
/// the words say first that the skill can be made, what a made skill can
/// do, and that `cannot` is only for a limit rules.md names. And its
/// bounds (the owner, 18 Nov, on #7737: the kept session keeps V1's full
/// grants, so the make's own words name them): only its folder, and no
/// command but the pack check.
pub const GATEWAY_CAN: &str = "You are making a skill for this lamp, and it can be made. Read \
     guide.md when it is here, then format.md for every step a skill can do, sdk.md for its code, examples/ \
     (whole skills the check takes), then follow rules.md. Words you know now need no lookup. \
     Answer cannot only for a limit rules.md names; if unsure, make it. Each make is new: \
     read these files again, format.md before any cannot, and write no tool key. Use only this \
     make's folder below; run nothing but the check it names.";

pub fn gateway_form() -> String {
    format!(
        "{FORM_ORDER} {FORM_RULE} A minimal skill.toml the lamp's check takes, the form only \
         and never the answer:\n\n```skill.toml\n{FORM_TOML}```"
    )
}

/// #7245: a gateway agent's make's words, with the guide when `guide`
/// ([`gateway_guide`]): as [`prompt`] and [`guided_prompt`], but no check
/// is run and no command at all; and either way the skill.toml form
/// inline after them ([`gateway_form`]). The person's request stays in the
/// rules alone. Hermes's make's words ([`prompt`], [`guided_prompt`]) are
/// as before.
pub fn gateway_prompt(folder: &Path, guide: bool) -> String {
    let words = if guide {
        gateway_guided_words(folder)
    } else {
        prompt(folder)
    };
    format!("{GATEWAY_CAN}\n\n{words}\n\n{}", gateway_form())
}

/// [`gateway_prompt`]'s words with the guide, before the form.
fn gateway_guided_words(folder: &Path) -> String {
    format!(
        "Everything you need is in two files in this folder, {folder}: {guide}, how to make a \
         lamp skill, and {rules}, the skill's form and the person's request. Read {guide} first, \
         then {rules}, and follow both; {rules} wins where they differ. Draft the skill in \
         {folder}/draft/NAME. Then write the answer exactly as {rules} says (the fenced blocks, \
         or its one line when the skill can't be made), and nothing else, to the file {answer} \
         in this folder. Write no file outside this folder, install nothing and run no command.",
        guide = GUIDE_FILE,
        rules = RULES_FILE,
        answer = ANSWER_FILE,
        folder = folder.display(),
    )
}

/// #7245: a gateway agent's make in `folder` ([`gateway_prompt`]), at
/// most `within`, started in its folder: no home, no switches, no token,
/// no script (nothing for the skill-start launcher to claim).
pub fn gateway_run(folder: &Path, within: Duration, guide: bool) -> SkillRun {
    gateway_run_with(folder, within, guide, false, &CheckSwitches::default())
}

/// [`gateway_run`], with the guide's check in its words when `checked`
/// and `guide` ([`gateway_checked_prompt`], V1's tools on), the line's
/// `switches` on its command; the run's own environment stays empty.
pub fn gateway_run_with(
    folder: &Path,
    within: Duration,
    guide: bool,
    checked: bool,
    switches: &CheckSwitches,
) -> SkillRun {
    let mut run = make_run(folder, within);
    run.prompt = if guide && checked {
        gateway_checked_prompt(folder, switches)
    } else {
        gateway_prompt(folder, guide)
    };
    run.env = Vec::new();
    run
}

/// The run of a make in `folder`: its words, at most `within`, started in
/// its folder, with its own fresh home ([`HOME_FOLDER`]) as `HOME`,
/// `HERMES_HOME` under it and [`HOME_ENV`] naming it, and nothing else of
/// its own (no token, no grant).
pub fn make_run(folder: &Path, within: Duration) -> SkillRun {
    make_run_with(folder, within, false, &CheckSwitches::default())
}

/// [`make_run`], with the guide's words ([`guided_prompt`]) when `guide`,
/// and the check's `switches` that are on in its environment
/// ([`CheckSwitches::env`]).
pub fn make_run_with(
    folder: &Path,
    within: Duration,
    guide: bool,
    switches: &CheckSwitches,
) -> SkillRun {
    SkillRun {
        // The make's own run id, from its folder's name (`make-<run id>`).
        run: folder
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_prefix(FOLDER_HEAD))
            .unwrap_or_default()
            .into(),
        skill: "make".into(),
        prompt: if guide {
            guided_prompt(folder)
        } else {
            prompt(folder)
        },
        folder: folder.to_path_buf(),
        // A make has no skill script yet, so it is never a gateway agent's
        // run (#6882): a gateway refuses an empty script, as a fixed write.
        script: PathBuf::new(),
        task: String::new(),
        env: home_env(folder).into_iter().chain(switches.env()).collect(),
        deadline: within,
        started: Default::default(),
        write: false,
        not_made: Default::default(),
        start_in: Some(folder.to_path_buf()),
        env_file: None,
        answer: Default::default(),
        net: false,
        tells: false,
        cancel: Default::default(),
    }
}

/// The variables naming a run's fresh home in `folder` ([`fresh_home`]):
/// `HOME`, `HERMES_HOME` under it and [`HOME_ENV`], the wrapper's mark.
pub(crate) fn home_env(folder: &Path) -> [(&'static str, String); 3] {
    let home = folder.join(HOME_FOLDER);
    let shown = |path: &Path| path.to_string_lossy().into_owned();
    [
        ("HOME", shown(&home)),
        (crate::hold::HERMES_HOME, shown(&home.join(HERMES_FOLDER))),
        (HOME_ENV, shown(&home)),
    ]
}

/// A make's own home in its folder (the first builder's 19:14 ruling on
/// #6643): a fresh, empty folder, 0700, the run's `HOME`, removed with the
/// make.
pub const HOME_FOLDER: &str = "home";
/// Hermes's own folder in it, `HERMES_HOME`, holding only its provider
/// config ([`CONFIG_FILE`]) and the links of [`RUNTIME_LINKS`].
pub const HERMES_FOLDER: &str = ".hermes";
/// The provider config copied in: the lamp's provider, its key a
/// variable's name only (`key_env`, [`crate::hermes_config::TOKEN_ENV`]),
/// never a key, memory, sessions or state.
pub const CONFIG_FILE: &str = "config.yaml";
/// Hermes's prepared runtime, each a link in a make's `HERMES_HOME` to
/// the lasting home's own (#6643, lamp-c0a4): `installs`, its dependency
/// record (`installs/<key>/facts.json`) and environments, and `tools`, its
/// uv, Python and package manager. Hermes 0.22.2 keeps dependency state
/// per data root (`pm/environments.py`, `<HERMES_HOME>/installs/<key>`);
/// with none in a fresh home it syncs one first ("preparing dependencies
/// for this data root"), which a make's run cannot finish, so it ended
/// `run_failed` in under a second. Links, not binds: Hermes takes a
/// recorded environment only under `<HERMES_HOME>/installs/<key>/environments`
/// once resolved, and a link resolves to the lasting path the record
/// names. lamp-update's namespace script ([`NS_HOME_LINE`]) shows both
/// targets read-only, so nothing in them can be changed but what Hermes
/// writes at every start, bound back read-write: each
/// `installs/<key>/.install.lock` (`runtime_lock`) and each lease-managed
/// generation's `installs/<key>/environments/<gen>/.leases`
/// (`lease_directory`), and `installs/<key>/pm-runtime/.prepare.lock`
/// (`pm/runtime.py` `prepare_runtime`) with its generations' `.leases`;
/// and nothing else of the lasting home: never its
/// memory, sessions or state.
pub const RUNTIME_LINKS: [(&str, &str); 2] = [
    ("installs", "/var/lib/lampos-hermes/.hermes/installs"),
    ("tools", "/var/lib/lampos-hermes/.hermes/tools"),
];
/// lamp-update's four Hermes settings (`lamp_update::hermes_install::CONFIG`,
/// a test holds them equal), written into each make's fresh config too:
/// no update checks of its own over the kept network, no language servers,
/// and a `chat -q` run that ends with its turn, not up to 600 s later
/// (which `within_ms` would cut, its answer lost as `run_failed`).
pub const MAKE_CONFIG: [(&str, &str); 4] = [
    ("updates.check", "false"),
    ("plugins.auto_update_check_hours", "0"),
    ("lsp.enabled", "false"),
    ("terminal.oneshot_completion_wait_seconds", "0"),
];
/// The work folder the wrapper takes a make's home in
/// (`lamp_update::hermes_install::MAKE_HOME_GLOB`'s, `make-*/home` in it):
/// a make whose work folder is any other is refused [`NO_NAMESPACE`], as
/// the wrapper would run it in Hermes's lasting home.
pub const MAKE_WORK: &str = "/var/lib/lampos-tasks/harnesses/lamp-tasks/lamp";
/// The variable naming the make's home to Hermes's wrapper
/// (`lamp_update::hermes_install::HOME_ENV`).
pub const HOME_ENV: &str = "LAMP_NS_HOME";
/// The namespace script Hermes's wrapper runs (`lamp_update::hermes_install::NS`).
pub const NS: &str = "/usr/local/lib/lampos/hermes-ns";
/// The wrapper's line for a make's home (`lamp_update::hermes_install::WRAPPER_HOME_LINE`,
/// held equal to it by a test): its mark, checked before every make, held or not.
pub const WRAPPER_HOME_LINE: &str = "case ${LAMP_NS_HOME-} in *..*) unset LAMP_NS_HOME ;; /var/lib/lampos-tasks/harnesses/lamp-tasks/lamp/make-*/home) export HERMES_HOME=\"$LAMP_NS_HOME/.hermes\" LAMP_NS_HOME ;; *) unset LAMP_NS_HOME ;; esac\n";
/// The namespace script's line for it (`lamp_update::hermes_install::NS_HOME_LINE`).
pub const NS_HOME_LINE: &str = "if [ -n \"${LAMP_NS_HOME-}\" ]; then\n  F=${LAMP_NS_HOME%/home} X=/tmp/.lampos-mk\n  [ -d \"$LAMP_NS_HOME\" ] && [ -n \"$W\" ] && [ \"${F%/*}\" = \"$W/lamp\" ] || fail \"no $LAMP_NS_HOME\"\n  mkdir -m 0700 \"$X\" \"$X/f\" \"$X/c\" \"$X/t\" \"$X/i\" || fail \"no make staging\"\n  mount -n --bind \"$F\" \"$X/f\" || fail \"no $F\"\n  mount -n --bind \"$H/.hermes/hermes-agent\" \"$X/c\" || fail \"no Hermes checkout\"\n  mount -n --bind \"$H/.hermes/tools\" \"$X/t\" || fail \"no Hermes tools\"\n  mount -n --bind \"$H/.hermes/installs\" \"$X/i\" || fail \"no Hermes installs\"\n  k=\n  for p in \"$H/.hermes/installs\"/*/.install.lock; do\n    [ -f \"$p\" ] && [ ! -L \"$p\" ] && [ ! -L \"${p%/.install.lock}\" ] || continue\n    command exec 9<\"$p\" || fail \"Hermes install lock not shown\"\n    s=$(stat -L -c %d:%i /proc/self/fd/9) && [ ! -L \"$p\" ] && [ \"$(stat -c %d:%i \"$p\")\" = \"$s\" ] && mount -n -c --bind /proc/self/fd/9 \"$X/i/${p#\"$H/.hermes/installs/\"}\" || { exec 9<&-; fail \"Hermes install lock not shown\"; }\n    [ ! -L \"$X/i/${p#\"$H/.hermes/installs/\"}\" ] && [ \"$(stat -c %d:%i \"$X/i/${p#\"$H/.hermes/installs/\"}\")\" = \"$s\" ] || { exec 9<&-; umount -n \"$X/i/${p#\"$H/.hermes/installs/\"}\"; fail \"Hermes install lock not shown\"; }\n    exec 9<&-\n    k=1\n  done\n  [ -n \"$k\" ] || fail \"no Hermes install lock: the lasting Hermes has not run\"\n  for p in \"$H/.hermes/installs\"/*/pm-runtime; do\n    [ -d \"$p\" ] || continue\n    [ ! -L \"$p\" ] && [ ! -L \"${p%/pm-runtime}\" ] && [ -f \"$p/.prepare.lock\" ] && [ ! -L \"$p/.prepare.lock\" ] && command exec 9<\"$p/.prepare.lock\" || fail \"Hermes runtime lock not shown\"\n    s=$(stat -L -c %d:%i /proc/self/fd/9) && [ ! -L \"$p/.prepare.lock\" ] && [ \"$(stat -c %d:%i \"$p/.prepare.lock\")\" = \"$s\" ] && mount -n -c --bind /proc/self/fd/9 \"$X/i/${p#\"$H/.hermes/installs/\"}/.prepare.lock\" || { exec 9<&-; fail \"Hermes runtime lock not shown\"; }\n    [ ! -L \"$X/i/${p#\"$H/.hermes/installs/\"}/.prepare.lock\" ] && [ \"$(stat -c %d:%i \"$X/i/${p#\"$H/.hermes/installs/\"}/.prepare.lock\")\" = \"$s\" ] || { exec 9<&-; umount -n \"$X/i/${p#\"$H/.hermes/installs/\"}/.prepare.lock\"; fail \"Hermes runtime lock not shown\"; }\n    exec 9<&-\n  done\n  for p in \"$H/.hermes/installs\"/*/environments/*/.lease-managed \"$H/.hermes/installs\"/*/pm-runtime/generations/*/.lease-managed; do\n    [ -f \"$p\" ] || continue\n    g=${p%/.lease-managed} r=${p#\"$H/.hermes/installs/\"}\n    [ ! -L \"$g\" ] && [ ! -L \"${g%/*}\" ] && [ ! -L \"$H/.hermes/installs/${r%%/*}\" ] && { [ -d \"$g/.leases\" ] || mkdir \"$g/.leases\"; } && [ ! -L \"$g/.leases\" ] && command exec 9<\"$g/.leases\" || fail \"Hermes leases not shown\"\n    s=$(stat -L -c %d:%i /proc/self/fd/9) && [ ! -L \"$g/.leases\" ] && [ \"$(stat -c %d:%i \"$g/.leases\")\" = \"$s\" ] && mount -n -c --bind /proc/self/fd/9 \"$X/i/${g#\"$H/.hermes/installs/\"}/.leases\" || { exec 9<&-; fail \"Hermes leases not shown\"; }\n    [ ! -L \"$X/i/${g#\"$H/.hermes/installs/\"}/.leases\" ] && [ \"$(stat -c %d:%i \"$X/i/${g#\"$H/.hermes/installs/\"}/.leases\")\" = \"$s\" ] || { exec 9<&-; umount -n \"$X/i/${g#\"$H/.hermes/installs/\"}/.leases\"; fail \"Hermes leases not shown\"; }\n    exec 9<&-\n  done\n  mount -n -t tmpfs -o size=4k,mode=0755,nosuid,nodev,noexec none \"$H/.hermes\" || fail \"$H/.hermes not hidden\"\n  mkdir \"$H/.hermes/hermes-agent\" && mount -n --move \"$X/c\" \"$H/.hermes/hermes-agent\" && ro \"$H/.hermes/hermes-agent\" || fail \"Hermes checkout not shown\"\n  mkdir \"$H/.hermes/tools\" && mount -n --move \"$X/t\" \"$H/.hermes/tools\" && ro \"$H/.hermes/tools\" || fail \"Hermes tools not shown\"\n  mkdir \"$H/.hermes/installs\" && mount -n --move \"$X/i\" \"$H/.hermes/installs\" && ro \"$H/.hermes/installs\" || fail \"Hermes installs not shown\"\n  ro \"$H/.hermes\"; ro \"$H\"; ro \"$W\"\n  mount -n -t tmpfs -o size=4k,mode=0755,nosuid,nodev,noexec none \"$W/lamp\" || fail \"$W/lamp not hidden\"\n  mkdir \"$F\" && mount -n --move \"$X/f\" \"$F\" || fail \"$F not shown\"\n  ro \"$W/lamp\"\n  rmdir \"$X/f\" \"$X/c\" \"$X/t\" \"$X/i\" \"$X\" || fail \"make staging left\"\n  cd \"$F\" || fail \"$F not visible\"\n  HOME=$LAMP_NS_HOME\nfi\nunset LAMP_NS_HOME F X k p g r s\n";
/// With lamp-update's one run path (`LAMP_SKILL_DEFAULT_AGENT` on, #6685),
/// Hermes's wrapper is the one run path's
/// ([`crate::skill_task::wrapper_takes_runs`]) and runs
/// [`crate::skill_task::AGENT_NS`]; its data's make-home line
/// (`lamp_update::hermes_install::ONE_PATH_HOME_LINE`, held equal to it by
/// a test) hands the make's home on as `LAMP_NS_MAKE`.
pub const ONE_PATH_HOME_LINE: &str = "  case ${LAMP_NS_HOME-} in *..*) unset LAMP_NS_MAKE ;; /var/lib/lampos-tasks/harnesses/lamp-tasks/lamp/make-*/home) LAMP_NS_MAKE=$LAMP_NS_HOME HERMES_HOME=\"$LAMP_NS_HOME/.hermes\"; export LAMP_NS_MAKE HERMES_HOME ;; *) unset LAMP_NS_MAKE ;; esac\n";
/// The one agent namespace's make lines for Hermes
/// (`lamp_update::hermes_install::ONE_PATH_NS_MAKE_LINES`).
pub const ONE_PATH_NS_MAKE_LINES: &str = "  F=${LAMP_NS_MAKE%/home} Y=/tmp/.lampos-mk\n  [ -d \"$LAMP_NS_MAKE\" ] && [ -n \"$W\" ] && [ \"${F%/*}\" = \"$W/lamp\" ] || fail \"no $LAMP_NS_MAKE\"\n  mkdir -m 0700 \"$Y\" \"$Y/f\" \"$Y/c\" \"$Y/t\" \"$Y/i\" || fail \"no make staging\"\n  mount -n --bind \"$F\" \"$Y/f\" || fail \"no $F\"\n  mount -n --bind \"$H/.hermes/hermes-agent\" \"$Y/c\" || fail \"no Hermes checkout\"\n  mount -n --bind \"$H/.hermes/tools\" \"$Y/t\" || fail \"no Hermes tools\"\n  mount -n --bind \"$H/.hermes/installs\" \"$Y/i\" || fail \"no Hermes installs\"\n  k=\n  for p in \"$H/.hermes/installs\"/*/.install.lock; do\n    [ -f \"$p\" ] && [ ! -L \"$p\" ] && [ ! -L \"${p%/.install.lock}\" ] || continue\n    command exec 9<\"$p\" || fail \"Hermes install lock not shown\"\n    s=$(stat -L -c %d:%i /proc/self/fd/9) && [ ! -L \"$p\" ] && [ \"$(stat -c %d:%i \"$p\")\" = \"$s\" ] && mount -n -c --bind /proc/self/fd/9 \"$Y/i/${p#\"$H/.hermes/installs/\"}\" || { exec 9<&-; fail \"Hermes install lock not shown\"; }\n    [ ! -L \"$Y/i/${p#\"$H/.hermes/installs/\"}\" ] && [ \"$(stat -c %d:%i \"$Y/i/${p#\"$H/.hermes/installs/\"}\")\" = \"$s\" ] || { exec 9<&-; umount -n \"$Y/i/${p#\"$H/.hermes/installs/\"}\"; fail \"Hermes install lock not shown\"; }\n    exec 9<&-\n    k=1\n  done\n  [ -n \"$k\" ] || fail \"no Hermes install lock: the lasting Hermes has not run\"\n  for p in \"$H/.hermes/installs\"/*/pm-runtime; do\n    [ -d \"$p\" ] || continue\n    [ ! -L \"$p\" ] && [ ! -L \"${p%/pm-runtime}\" ] && [ -f \"$p/.prepare.lock\" ] && [ ! -L \"$p/.prepare.lock\" ] && command exec 9<\"$p/.prepare.lock\" || fail \"Hermes runtime lock not shown\"\n    s=$(stat -L -c %d:%i /proc/self/fd/9) && [ ! -L \"$p/.prepare.lock\" ] && [ \"$(stat -c %d:%i \"$p/.prepare.lock\")\" = \"$s\" ] && mount -n -c --bind /proc/self/fd/9 \"$Y/i/${p#\"$H/.hermes/installs/\"}/.prepare.lock\" || { exec 9<&-; fail \"Hermes runtime lock not shown\"; }\n    [ ! -L \"$Y/i/${p#\"$H/.hermes/installs/\"}/.prepare.lock\" ] && [ \"$(stat -c %d:%i \"$Y/i/${p#\"$H/.hermes/installs/\"}/.prepare.lock\")\" = \"$s\" ] || { exec 9<&-; umount -n \"$Y/i/${p#\"$H/.hermes/installs/\"}/.prepare.lock\"; fail \"Hermes runtime lock not shown\"; }\n    exec 9<&-\n  done\n  for p in \"$H/.hermes/installs\"/*/environments/*/.lease-managed \"$H/.hermes/installs\"/*/pm-runtime/generations/*/.lease-managed; do\n    [ -f \"$p\" ] || continue\n    g=${p%/.lease-managed} r=${p#\"$H/.hermes/installs/\"}\n    [ ! -L \"$g\" ] && [ ! -L \"${g%/*}\" ] && [ ! -L \"$H/.hermes/installs/${r%%/*}\" ] && { [ -d \"$g/.leases\" ] || mkdir \"$g/.leases\"; } && [ ! -L \"$g/.leases\" ] && command exec 9<\"$g/.leases\" || fail \"Hermes leases not shown\"\n    s=$(stat -L -c %d:%i /proc/self/fd/9) && [ ! -L \"$g/.leases\" ] && [ \"$(stat -c %d:%i \"$g/.leases\")\" = \"$s\" ] && mount -n -c --bind /proc/self/fd/9 \"$Y/i/${g#\"$H/.hermes/installs/\"}/.leases\" || { exec 9<&-; fail \"Hermes leases not shown\"; }\n    [ ! -L \"$Y/i/${g#\"$H/.hermes/installs/\"}/.leases\" ] && [ \"$(stat -c %d:%i \"$Y/i/${g#\"$H/.hermes/installs/\"}/.leases\")\" = \"$s\" ] || { exec 9<&-; umount -n \"$Y/i/${g#\"$H/.hermes/installs/\"}/.leases\"; fail \"Hermes leases not shown\"; }\n    exec 9<&-\n  done\n  mount -n -t tmpfs -o size=4k,mode=0755,nosuid,nodev,noexec none \"$H/.hermes\" || fail \"$H/.hermes not hidden\"\n  mkdir \"$H/.hermes/hermes-agent\" && mount -n --move \"$Y/c\" \"$H/.hermes/hermes-agent\" && ro \"$H/.hermes/hermes-agent\" || fail \"Hermes checkout not shown\"\n  mkdir \"$H/.hermes/tools\" && mount -n --move \"$Y/t\" \"$H/.hermes/tools\" && ro \"$H/.hermes/tools\" || fail \"Hermes tools not shown\"\n  mkdir \"$H/.hermes/installs\" && mount -n --move \"$Y/i\" \"$H/.hermes/installs\" && ro \"$H/.hermes/installs\" || fail \"Hermes installs not shown\"\n  ro \"$H/.hermes\"; ro \"$H\"; ro \"$W\"\n  mount -n -t tmpfs -o size=4k,mode=0755,nosuid,nodev,noexec none \"$W/lamp\" || fail \"$W/lamp not hidden\"\n  mkdir \"$F\" && mount -n --move \"$Y/f\" \"$F\" || fail \"$F not shown\"\n  ro \"$W/lamp\"\n  rmdir \"$Y/f\" \"$Y/c\" \"$Y/t\" \"$Y/i\" \"$Y\" || fail \"make staging left\"\n  cd \"$F\" || fail \"$F not visible\"\n  HOME=$LAMP_NS_MAKE\n  unset F Y k p g r s\n";
/// Said when Hermes's wrapper or namespace script has no make-home line.
pub const NO_NAMESPACE: &str = "the lamp's Hermes can't be given a make's own home yet (its wrapper or namespace script has no make-home line: lamp-update's LAMP_SKILL_TASKS is off or its round hasn't run), so nothing was started";

/// A make's folder, held open from when it was made, and removed however
/// the make ends: an answer, a failure, its deadline or a panic. An agent
/// skill's run starts in one of its own too (`crate::skill_task`,
/// `LAMP_UPLOAD_TO_AGENT`).
pub(crate) struct MadeFolder {
    pub(crate) path: PathBuf,
    fd: rustix::fd::OwnedFd,
}

impl Drop for MadeFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

const DIR_FLAGS: rustix::fs::OFlags = rustix::fs::OFlags::DIRECTORY
    .union(rustix::fs::OFlags::NOFOLLOW)
    .union(rustix::fs::OFlags::RDONLY)
    .union(rustix::fs::OFlags::CLOEXEC);

/// Folder `name` made 0700 in `at` and opened, never through a link.
fn folder_in(at: &rustix::fd::OwnedFd, name: &str) -> std::io::Result<rustix::fd::OwnedFd> {
    use rustix::fs::{Mode, fchmod, mkdirat, openat};
    mkdirat(at, name, Mode::from_raw_mode(0o700))?;
    let fd = openat(at, name, DIR_FLAGS, Mode::empty())?;
    fchmod(&fd, Mode::from_raw_mode(0o700))?;
    Ok(fd)
}

/// File `name` made new, 0600, in `at` with `text`, never through a link.
fn file_in(at: &rustix::fd::OwnedFd, name: &str, text: &str) -> std::io::Result<()> {
    use rustix::fs::{Mode, OFlags, openat};
    use std::io::Write;
    let fd = openat(
        at,
        name,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::from_raw_mode(0o600),
    )?;
    std::fs::File::from(fd).write_all(text.as_bytes())
}

/// The make's folder in `work` with its rules, and its fresh home with
/// only the provider config and the runtime links ([`RUNTIME_LINKS`]):
/// each folder 0700 and each file 0600, made
/// before a byte is written, each part opened from the one above, never
/// through a link.
#[cfg(test)]
fn put(work: &Path, rules: &str) -> std::io::Result<MadeFolder> {
    put_with(work, rules, false)
}

/// [`put`], with the guide ([`GUIDE`]) beside the rules as [`GUIDE_FILE`]
/// when `guide`, 0600 as they are.
fn put_with(work: &Path, rules: &str, guide: bool) -> std::io::Result<MadeFolder> {
    let made = fresh_home(work)?;
    file_in(&made.fd, RULES_FILE, rules)?;
    if guide {
        file_in(&made.fd, GUIDE_FILE, GUIDE)?;
    }
    Ok(made)
}

/// A live ask's folder in `work` (`crate::voice_ask`): a make's, its
/// rules the ask's, its fresh home's config with the lamp's agent's
/// connectors only when `services`.
pub(crate) fn put_ask(work: &Path, rules: &str, services: bool) -> std::io::Result<MadeFolder> {
    let made = fresh_home_for(work, services)?;
    file_in(&made.fd, RULES_FILE, rules)?;
    Ok(made)
}

/// A live ask's folder on a gateway agent (`crate::voice_ask`, OpenClaw):
/// `make-<run id>` in `work`, 0700, with only the ask's rules, 0600, byte
/// for byte (never [`without_check`]: the person's words are a line of
/// them), and no home, as the gateway agent runs in its own.
pub(crate) fn put_ask_on_gateway(work: &Path, rules: &str) -> std::io::Result<MadeFolder> {
    let made = fresh_folder(work, FOLDER_HEAD)?;
    file_in(&made.fd, RULES_FILE, rules)?;
    Ok(made)
}

/// #7245: a gateway agent's make's folder in `work`, `make-<run id>`,
/// 0700, with the rules and, when `guide`, the guide, each 0600: no home,
/// as the gateway agent runs in its own. With V1's tools (`checked`) the
/// rules and [`GUIDE`] whole, the check in both, as Hermes's make has
/// them; without, the rules [`without_check`] and [`gateway_guide`].
fn put_for_gateway(
    work: &Path,
    rules: &str,
    guide: bool,
    checked: bool,
) -> std::io::Result<MadeFolder> {
    let made = fresh_folder(work, FOLDER_HEAD)?;
    if checked {
        file_in(&made.fd, RULES_FILE, rules)?;
    } else {
        file_in(&made.fd, RULES_FILE, &without_check(rules))?;
    }
    if guide {
        if checked {
            file_in(&made.fd, GUIDE_FILE, GUIDE)?;
        } else {
            file_in(&made.fd, GUIDE_FILE, &gateway_guide())?;
        }
    }
    file_in(&made.fd, FORMAT_FILE, FORMAT)?;
    file_in(&made.fd, SDK_FILE, SDK)?;
    examples_in(&made.fd)?;
    Ok(made)
}

/// A run's own folder in `work`, `make-<run id>`, and its fresh home in it
/// ([`HOME_FOLDER`], [`HERMES_FOLDER`]) with only the provider config
/// ([`make_config`]): a make's, and an agent skill's run's or a `net`
/// run's (`crate::skill_task`), so the wrapper takes it as the run's home
/// and its namespace script hides Hermes's lasting home, the person's
/// memories and sessions, from the run ([`WRAPPER_HOME_LINE`],
/// [`NS_HOME_LINE`]). Removed however the run ends.
pub(crate) fn fresh_home(work: &Path) -> std::io::Result<MadeFolder> {
    fresh_home_for(work, false)
}

/// [`fresh_home`], its config with the lamp's agent's connectors
/// (`mcp_servers.lamp-services`, [`crate::hermes_config::mcp_entry`]) when
/// `services`: an agent skill's run granted a linked service
/// (`crate::skill_task`, `LAMP_AGENT_SERVICES` on in lamp-tasks), which
/// reaches it only through lamp-control under the run's one-run token.
pub(crate) fn fresh_home_for(work: &Path, services: bool) -> std::io::Result<MadeFolder> {
    fresh_home_named(work, &format!("{FOLDER_HEAD}{}", run_id()), services)
}

/// [`fresh_home_for`], its folder named by the task's run id, `make-<run>`:
/// on the one run path the wrapper finds a run granted `net` its fresh home
/// by that id (#7200, `lamp_update::agent_ns::wrapper::ONE_PATH_EGRESS_LINES`),
/// as Harness passes it no variable of lamp-tasks'.
pub(crate) fn fresh_home_for_run(
    work: &Path,
    services: bool,
    run: &str,
) -> std::io::Result<MadeFolder> {
    fresh_home_named(work, &format!("{FOLDER_HEAD}{run}"), services)
}

fn fresh_home_named(work: &Path, name: &str, services: bool) -> std::io::Result<MadeFolder> {
    let made = fresh_folder(work, name)?;
    let home = folder_in(&made.fd, HOME_FOLDER)?;
    let hermes = folder_in(&home, HERMES_FOLDER)?;
    file_in(&hermes, CONFIG_FILE, &make_config(services)?)?;
    for (name, target) in RUNTIME_LINKS {
        rustix::fs::symlinkat(target, &hermes, name)?;
    }
    Ok(made)
}

/// A fresh folder `name` (`make-<run id>`) in `work`, 0700 before anything
/// is in it, made new (never one there), opened never through a link and
/// removed however its run ends.
fn fresh_folder(work: &Path, name: &str) -> std::io::Result<MadeFolder> {
    use rustix::fs::{Mode, open};
    let at = open(work, DIR_FLAGS, Mode::empty())?;
    let fd = folder_in(&at, name)?;
    Ok(MadeFolder {
        path: work.join(name),
        fd,
    })
}

/// A make's fresh config: the lamp's provider ([`crate::hermes_config::merge_config`],
/// its key a variable's name only), the lamp's agent's connectors only when
/// `services`, and [`MAKE_CONFIG`]. A setting whose top-level block the
/// provider config already has (`terminal:`, with its `shell_init_files`)
/// goes into that block, as its first line: a second block of the same
/// name is a duplicate key, and Hermes refuses the whole file (exit 2,
/// c0a4, 14 Nov). Any other is a block of its own.
fn make_config(services: bool) -> std::io::Result<String> {
    let mut config =
        crate::hermes_config::merge_config_for("", crate::runtime::HERMES_FORWARDER_PORT, services)
            .map_err(|_| std::io::Error::other("the provider config was not made"))?;
    for (key, value) in MAKE_CONFIG {
        let (block, name) = key.split_once('.').unwrap_or(("", key));
        let head = format!("{block}:\n");
        let line = format!("  {name}: {value}\n");
        let at = if config.starts_with(&head) {
            Some(0)
        } else {
            config.find(&format!("\n{head}")).map(|at| at + 1)
        };
        match at {
            Some(at) => config.insert_str(at + head.len(), &line),
            None => config.push_str(&format!("{head}{line}")),
        }
    }
    Ok(config)
}

/// The answer the run wrote, read from the make's folder as it was made
/// (its descriptor, so a folder swapped for a link is never followed): a
/// regular file, never a link, of at most [`MOST_ANSWER_BYTES`] of text.
fn answer_read(folder: &rustix::fd::OwnedFd) -> Option<String> {
    use rustix::fs::{Mode, OFlags, openat};
    use std::io::Read;
    let fd = openat(
        folder,
        ANSWER_FILE,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .ok()?;
    let file = std::fs::File::from(fd);
    let meta = file.metadata().ok()?;
    if !meta.file_type().is_file() || meta.len() > MOST_ANSWER_BYTES {
        return None;
    }
    let mut text = String::new();
    file.take(MOST_ANSWER_BYTES + 1)
        .read_to_string(&mut text)
        .ok()?;
    answer_held(text)
}

/// An answer as a make holds it, from [`ANSWER_FILE`] or the agent's chat
/// alike: not empty, and at most [`MOST_ANSWER_BYTES`].
fn answer_held(text: String) -> Option<String> {
    (!text.trim().is_empty() && text.len() as u64 <= MOST_ANSWER_BYTES).then_some(text)
}

/// [`answer_read`] of the make's folder at `folder`, opened never through
/// a link: the one run path's read of a gateway make's answer before its
/// guard removes the folder (#7245).
pub(crate) fn answer_in(folder: &Path) -> Option<String> {
    let fd = rustix::fs::open(folder, DIR_FLAGS, rustix::fs::Mode::empty()).ok()?;
    answer_read(&fd)
}

/// How an answer begins that makes nothing, as the rules ask for it and
/// the voice reads it (`made_skill::CANNOT`).
pub const CANNOT: &str = "cannot:";

/// Where a gateway make's answer was taken from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AnswerFrom {
    /// Its folder's [`ANSWER_FILE`].
    File,
    /// The agent's final chat answer, with no [`ANSWER_FILE`] (#7427).
    Chat,
}

/// A gateway make's answer (#7245, #7427): its folder's [`ANSWER_FILE`]
/// ([`answer_in`]) whenever there is one; else the agent's final chat
/// answer `chat`, only when it is a make's answer ([`chat_answer`]). On
/// lamp-c0a4 (OpenClaw, deepseek-flash) the agent put every make's answer
/// in chat and wrote no file: a `cannot:` (13584), and a whole skill
/// (17 Nov 09:35, `head-pat-next-event`), each dropped as `no_answer`.
/// Any other chat answer is dropped, as before. `chat` comes with whether
/// Harness cut it short (`truncated`, its `show`'s `--chars`).
pub(crate) fn make_answer(
    folder: &Path,
    chat: Option<(&str, bool)>,
) -> Option<(String, AnswerFrom)> {
    if let Some(text) = answer_in(folder) {
        return Some((text, AnswerFrom::File));
    }
    chat.and_then(|(chat, truncated)| chat_answer(chat, truncated))
        .map(|text| (text, AnswerFrom::Chat))
}

/// The agent's chat answer when it is a make's answer: its `cannot:` line
/// ([`chat_cannot`]), or the whole chat when it holds a skill's files
/// ([`chat_skill`]), only when Harness didn't cut it short (`truncated`,
/// Heron on #7441: a cut answer may have lost a file of its code after two
/// whole blocks); else none.
pub(crate) fn chat_answer(chat: &str, truncated: bool) -> Option<String> {
    chat_cannot(chat, truncated).or_else(|| (!truncated).then(|| chat_skill(chat)).flatten())
}

/// The agent's chat answer's `cannot:` line, held as [`answer_read`] holds
/// the file: the first line only, beginning with [`CANNOT`] (any case,
/// after blank space) and with a reason after it, no fenced block, not
/// empty and at most [`MOST_ANSWER_BYTES`]; else none. With the chat cut
/// short (`truncated`), only a line that ended before the cut (a line
/// break follows it in what Harness showed), so a reason is never taken
/// cut mid-word.
pub(crate) fn chat_cannot(chat: &str, truncated: bool) -> Option<String> {
    let chat = chat.trim_start();
    let line = chat.lines().next()?.trim_end();
    let head = line.get(..CANNOT.len())?;
    let reason = line[CANNOT.len()..].trim();
    (head.eq_ignore_ascii_case(CANNOT)
        && !reason.is_empty()
        && !chat.contains("```")
        && (!truncated || chat.contains('\n'))
        && line.len() as u64 <= MOST_ANSWER_BYTES)
        .then(|| line.to_owned())
}

/// The agent's whole chat answer when it holds a skill as `answer.md`
/// does: held as the file is ([`answer_held`]: not empty, at most
/// [`MOST_ANSWER_BYTES`]), with one closed fenced block that is
/// `skill.toml` and one that is `SKILL.md` ([`skill_blocks`]), and not the
/// rules' own example skill given back ([`is_example`]); else none. Words
/// around the blocks are kept: the voice reads the blocks alone
/// (`made_skill::files`), through the same checks and repairs as a file's.
pub(crate) fn chat_skill(chat: &str) -> Option<String> {
    let text = answer_held(chat.to_owned())?;
    let blocks = skill_blocks(&text);
    let one = |part: Part| {
        let mut of = blocks.iter().filter(|(at, _)| *at == part);
        match (of.next(), of.next()) {
            (Some((_, body)), None) => Some(body.as_str()),
            _ => None,
        }
    };
    let (toml, card) = (one(Part::Toml)?, one(Part::Card)?);
    (!is_example(toml, card)).then_some(text)
}

/// The example skill the rules show the model whole
/// (`make_skill_local::EXAMPLE`, lamp-voice's; lamp-voice depends on
/// lamp-tasks, so it is copied here): its `SKILL.md` name, and its
/// `skill.toml`.
const EXAMPLE_NAME: &str = "example-only";
const EXAMPLE_TOML: &str = "kind = \"installable\"
version = \"1.0.0\"
needs = [\"light\"]

[permissions]
topics = [\"room/presence\"]
light = true

[trigger]
topic = \"room/presence\"
field = \"someone\"
equals = true
for = \"5s\"
cooldown = \"1h\"
rearm = \"ends\"

[ask]
policy = \"nudge\"

[[do]]
light = { colour = \"cyan\", level = 2, for = \"25s\" }
";

/// Whether a skill's two files are the rules' example given back, as the
/// voice's own maker tells it (`make_skill_local::is_example`): its
/// `skill.toml` the example's but for blank space, or its `SKILL.md` named
/// as the example is; or the gateway words' own minimal one
/// ([`FORM_TOML`], #7245) given back.
fn is_example(toml: &str, card: &str) -> bool {
    let bare = |text: &str| text.split_whitespace().collect::<String>();
    let name = card
        .lines()
        .find_map(|line| line.trim().strip_prefix("name:"))
        .map(|name| name.trim().trim_matches('"'));
    bare(toml) == bare(EXAMPLE_TOML) || bare(toml) == bare(FORM_TOML) || name == Some(EXAMPLE_NAME)
}

/// Which of a skill's two files a fenced block is, read as the voice reads
/// it (`made_skill::fence_named`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Toml,
    Card,
    /// No name: `SKILL.md` when it begins with front matter, else
    /// `skill.toml` ([`skill_blocks`]).
    Bare,
    /// A file of its code (`scripts/…`), a light show's (`shows/…`), or
    /// any other.
    Other,
}

/// A fence line, indented or not: its mark, how many (three or more), and
/// its info string, trimmed (`made_skill::fence`).
fn fence(line: &str) -> Option<(u8, usize, &str)> {
    let line = line.trim();
    let mark = *line.as_bytes().first()?;
    if mark != b'`' && mark != b'~' {
        return None;
    }
    let run = line.bytes().take_while(|&b| b == mark).count();
    (run >= 3).then(|| (mark, run, line[run..].trim()))
}

/// A fence's part and whether it names a file whole, from its info
/// string's last word (`made_skill::fence_named`).
fn fence_part(info: &str) -> (Part, bool) {
    let word = info.split_whitespace().last().unwrap_or_default();
    let word = word.rsplit_once('=').map_or(word, |(_, value)| value);
    let word = word
        .trim_matches(|c| matches!(c, '"' | '\'' | '*' | '`'))
        .trim_end_matches(':');
    // A file of its code, or a light show's (#7245): a block of its own.
    if word.starts_with("scripts/") || word.starts_with("shows/") {
        return (Part::Other, true);
    }
    match word.to_ascii_lowercase().as_str() {
        "" => (Part::Bare, false),
        "skill.toml" => (Part::Toml, true),
        "skill.md" => (Part::Card, true),
        "toml" => (Part::Toml, false),
        "md" | "markdown" => (Part::Card, false),
        _ => (Part::Other, false),
    }
}

/// The closed fenced blocks of `answer`, each as the file it is and its
/// body, found as the voice finds them (`made_skill::fenced`, line for
/// line): a block closes on a bare fence of its own mark at least as long
/// (any other bare fence is a line of its body), or at the end of its
/// line; a fence naming another file whole, or any named fence in
/// `skill.toml`, closes it and opens that one; in `SKILL.md` another named
/// fence opens a block within it, closed by the next bare fence. A bare
/// block is `SKILL.md` when it begins with front matter, else
/// `skill.toml`. A block left open at the end is not one.
fn skill_blocks(answer: &str) -> Vec<(Part, String)> {
    let mut blocks = Vec::new();
    // The block open now: its part, body, mark, run, and how many blocks
    // within it are open.
    let mut open: Option<(Part, String, u8, usize, usize)> = None;
    let mut lines = answer.lines();
    let mut again = None;
    while let Some(line) = again.take().or_else(|| lines.next()) {
        let Some((part, mut body, mark, run, mut depth)) = open.take() else {
            if let Some((mark, run, info)) = fence(line) {
                open = Some((fence_part(info).0, String::new(), mark, run, 0));
            }
            continue;
        };
        let mut closed = false;
        match fence(line) {
            Some((_, _, "")) if depth > 0 => {
                depth -= 1;
                body.push_str(line);
                body.push('\n');
            }
            Some((this, length, "")) if this == mark && length >= run => closed = true,
            Some((_, _, info)) if !info.is_empty() && depth == 0 => {
                let (named, whole) = fence_part(info);
                // A file of its code always begins a block of its own.
                let other_file = whole && (named != part || named == Part::Other);
                if other_file || part == Part::Toml {
                    closed = true;
                    again = Some(line);
                } else {
                    depth += 1;
                    body.push_str(line);
                    body.push('\n');
                }
            }
            Some((_, _, info)) if !info.is_empty() => {
                depth += 1;
                body.push_str(line);
                body.push('\n');
            }
            _ => {
                let end = String::from_utf8(vec![mark; run]).unwrap_or_default();
                match line.trim_end().strip_suffix(end.as_str()) {
                    Some(before) if depth == 0 && !before.trim().is_empty() => {
                        body.push_str(before);
                        body.push('\n');
                        closed = true;
                    }
                    _ => {
                        body.push_str(line);
                        body.push('\n');
                    }
                }
            }
        }
        if closed {
            let part = match part {
                Part::Bare if body.trim_start().starts_with("---") => Part::Card,
                Part::Bare => Part::Toml,
                part => part,
            };
            blocks.push((part, body));
        } else {
            open = Some((part, body, mark, run, depth));
        }
    }
    blocks
}

/// Make folders an earlier lamp-tasks left in `work` (stopped while a make
/// ran), removed as it starts: only folders named `make-*`, never a link.
pub fn sweep(work: &Path) {
    let Ok(entries) = std::fs::read_dir(work) else {
        return;
    };
    let mut swept = 0;
    for entry in entries.flatten() {
        if entry.file_name().to_string_lossy().starts_with(FOLDER_HEAD)
            && entry.file_type().is_ok_and(|kind| kind.is_dir())
            && std::fs::remove_dir_all(entry.path()).is_ok()
        {
            swept += 1;
        }
    }
    if swept > 0 {
        eprintln!("[lamp-tasks] skill make: {swept} old make folder(s) removed");
    }
}

/// The refusal's code for why a run can't start.
fn not_started(why: &str) -> &'static str {
    match why {
        NOT_HELD_OFF | NOT_HELD_OFF_ONE_PATH => "not_held_off",
        NO_NAMESPACE => "no_namespace",
        _ => "run_failed",
    }
}

/// lamp-tasks' answer to one make `line`: with the switch `on`, the line
/// checked, the run's namespace checked ([`Runner::ready`]), its folder
/// and fresh home made in `work`, `started` told, the run made and waited
/// for, its answer read and the folder removed; else the refusal.
pub fn answer(
    line: &[u8],
    on: bool,
    work: &Path,
    runner: &dyn Runner,
    started: &mut dyn FnMut(),
) -> Value {
    answer_with(line, on, work, runner, None, started)
}

/// [`answer`], a line with `"one_path":true` ([`MakeLine::one_path`]) going
/// to the lamp's default agent as `gateway` picks it by its data
/// ([`MakeGateway::pick`]): a gateway agent's make through `gateway`'s one
/// turn ([`MakeTurn::make_turn`]), the lamp's Hermes's through `hermes` as
/// before, any other refused in words. Without the field, or with no
/// `gateway`, `hermes` as [`answer`] has it.
pub fn answer_with(
    line: &[u8],
    on: bool,
    work: &Path,
    hermes: &dyn Runner,
    gateway: Option<&MakeGateway>,
    started: &mut dyn FnMut(),
) -> Value {
    if !on {
        return refusal(
            "off",
            "a skill run's namespace is off on this lamp (LAMP_SKILL_TASKS)",
        );
    }
    let make = match MakeLine::parse(line) {
        Ok(make) => make,
        Err((code, why)) => return refusal(code, &why),
    };
    let runner = match gateway {
        Some(gateway) if make.one_path => match (gateway.pick)() {
            // The lamp's agent isn't Hermes, but its Harness target isn't
            // known yet (right after boot): said at once, never a Hermes make.
            Ok(MakeAgent::Hermes) if make.engine.as_deref().is_some_and(|e| e != "Hermes") => {
                return refusal(NOT_READY, NOT_READY_LINE);
            }
            Ok(MakeAgent::Hermes) => hermes,
            Ok(MakeAgent::Gateway(engine)) => {
                // Hermes's direct make keeps the check its own make has.
                let checked = gateway.checked || engine == crate::harness::EngineKind::Hermes;
                return answer_on_gateway(&make, work, gateway, checked, started);
            }
            Err(why) => return refusal("run_failed", &why),
        },
        _ => hermes,
    };
    if let Err(why) = runner.ready() {
        return refusal(not_started(&why), &why);
    }
    let folder = match put_with(work, &make.rules, make.guide) {
        Ok(folder) => folder,
        Err(error) => {
            return refusal(
                "run_failed",
                &format!("the make's folder was not made: {error}"),
            );
        }
    };
    started();
    // A make has no token to end.
    let switches = make.switches.clone().unwrap_or_default();
    let ran = runner.run(
        &make_run_with(&folder.path, make.within(), make.guide, &switches),
        &|| {},
    );
    match ran {
        Err(why) => refusal(not_started(&why), &why),
        Ok(_) => match answer_read(&folder.fd) {
            Some(text) => json!({"ok": true, "answer": text}),
            None => refusal("no_answer", NO_ANSWER),
        },
    }
}

/// #7245: a make on a gateway agent ([`MakeGateway`]): its folder with the
/// rules made in `work`, `started` told, the one make turn sent and its
/// answer taken ([`MakeTurn::make_turn`], `answer.md` read before the
/// session and folder go), the folder removed here too however it ends.
fn answer_on_gateway(
    make: &MakeLine,
    work: &Path,
    gateway: &MakeGateway,
    checked: bool,
    started: &mut dyn FnMut(),
) -> Value {
    if let Err(why) = gateway.ready() {
        return refusal(not_started(&why), &why);
    }
    let folder = match put_for_gateway(work, &make.rules, make.guide, checked) {
        Ok(folder) => folder,
        Err(error) => {
            return refusal(
                "run_failed",
                &format!("the make's folder was not made: {error}"),
            );
        }
    };
    started();
    let switches = make.switches.clone().unwrap_or_default();
    let run = gateway_run_with(&folder.path, make.within(), make.guide, checked, &switches);
    let ran = gateway.run(&run, &|| {});
    // A make cut at its deadline (or its turn ended badly) that had written
    // its whole answer.md is still its answer (First's P3 on #7737), read
    // before the folder goes; the voice still checks it whole.
    let late = match &ran {
        Err(why) if !not_ready(why) => answer_in(&folder.path),
        _ => None,
    };
    drop(folder);
    if let Some(text) = late.filter(|text| !text.trim().is_empty()) {
        eprintln!("[lamp-tasks] skill make: its turn ended badly, its written answer read");
        return json!({"ok": true, "answer": text});
    }
    match ran {
        // Harness or the gateway not ready yet: said to the person at
        // once in the lamp's short words, never a silent failure; the
        // reason is journaled.
        Err(why) if not_ready(&why) => {
            eprintln!("[lamp-tasks] skill make: not ready: {why}");
            refusal(NOT_READY, NOT_READY_LINE)
        }
        Err(why) => refusal(not_started(&why), &why),
        Ok(_) => match run
            .answer
            .get()
            .filter(|text| !text.trim().is_empty() && text.len() as u64 <= MOST_ANSWER_BYTES)
        {
            Some(text) => json!({"ok": true, "answer": text}),
            None => refusal("no_answer", NO_GATEWAY_ANSWER),
        },
    }
}

/// The refusal's code of a gateway make that couldn't start because
/// Harness or the gateway isn't ready yet (#7245): lamp-voice says
/// [`NOT_READY_LINE`] for it.
pub const NOT_READY: &str = "not_ready";
/// What the person is told, at once, for [`NOT_READY`].
pub const NOT_READY_LINE: &str = "I'm still getting ready. Give me a minute and ask me again.";

/// Whether a gateway make's run was refused because Harness or the gateway
/// isn't ready yet: Harness not set up or with no lamp target, or the
/// gateway not answering ([`crate::gateway::NOT_READY`]).
pub(crate) fn not_ready(why: &str) -> bool {
    why.contains(crate::gateway::NOT_READY)
        || why.starts_with("Harness isn't set up")
        || why.starts_with("the lamp has no Harness target")
}

/// Said when a gateway agent's make ended with no answer to read (#7245).
pub const NO_GATEWAY_ANSWER: &str = "the lamp's default agent wrote no answer";

/// Which agent a make with `"one_path":true` goes to (#7245), by the lamp's
/// default agent's data ([`crate::skill_task::HarnessRunner::make_agent`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MakeAgent {
    /// The lamp's Hermes, in its own namespace ([`MakeHermes`]).
    Hermes,
    /// A gateway agent (OpenClaw), through [`MakeGateway`].
    Gateway(crate::harness::EngineKind),
}

/// THE SEAM (#7245): the one step "send this one make turn to the agent
/// and get its answer". A gateway make reaches its agent only through
/// [`MakeTurn::make_turn`]. Today's only implementation is a fresh
/// `harness new <engine>` per make in the make's folder, through the one
/// run path's runner ([`crate::skill_task::HarnessRunner`] with `makes`),
/// its session closed `--now` and its folder removed however it ends.
/// When #7258's one session (`LAMP_TASKS_ONE_SESSION`) is on, it takes
/// this step over by sending the make as a message into the shared pane
/// instead of a session per make: implement this trait there and hand it
/// to [`MakeGateway::turn`]; nothing else of the make changes.
pub trait MakeTurn {
    /// Sends `run` (its words, its folder [`SkillRun::start_in`] holding
    /// [`RULES_FILE`], its deadline) as one turn to the agent and waits
    /// for its end, at most `run.deadline`: the agent's `answer.md`, when
    /// it wrote one, in [`SkillRun::answer`]. Nothing is left of the turn
    /// once this returns.
    fn make_turn(&self, run: &SkillRun) -> Result<u64, String>;
    /// lamp-tasks' stop, as [`Runner::stop_on`]: a turn that outlives
    /// lamp-tasks ends when this is set.
    fn stop_on(&self, _stop: Arc<AtomicBool>) {}
}

impl MakeTurn for crate::skill_task::HarnessRunner {
    fn make_turn(&self, run: &SkillRun) -> Result<u64, String> {
        if !self.makes {
            return Err("this runner takes a skill's runs, not a make".into());
        }
        // A make has no token to end.
        Runner::run(self, run, &|| {})
    }

    fn stop_on(&self, stop: Arc<AtomicBool>) {
        Runner::stop_on(self, stop);
    }
}

/// A voice make on the lamp's default agent when it is a gateway agent
/// (OpenClaw, #7245): `pick` reads the agent's data at each make, and the
/// make is one turn through [`MakeTurn`], never a second runner. No token
/// and no skill-start claim: a make has no grants and no script.
pub struct MakeGateway {
    /// Which agent a make with `"one_path":true` goes to now.
    pub pick: Box<dyn Fn() -> Result<MakeAgent, String> + Send + Sync>,
    /// The make's one turn ([`MakeTurn`], the seam).
    pub turn: Arc<dyn MakeTurn + Send + Sync>,
    /// Whether the make gets the guide's `lamp-skills pack` check (#7245):
    /// with V1's tools on in OpenClaw's config
    /// (`crate::openclaw_config::V1_TOOLS_SWITCH`, read from lamp-tasks'
    /// environment), the gateway runs it as Hermes's make does.
    pub checked: bool,
}

impl MakeGateway {
    /// The lamp's one run path's runner ([`crate::skill_task::HarnessRunner::lamp`]
    /// with `makes`) on lamp-tasks' Harness `harness`, for [`Self::of`] and
    /// [`Self::of_direct`]: its default agent a gateway agent by
    /// [`crate::skill_task::HarnessRunner::make_agent`], the lamp's Hermes
    /// as its own, any other refused in words.
    pub fn lamp_runner(
        home: &Path,
        harness: crate::skill_task::SkillHarnessSlot,
    ) -> Arc<crate::skill_task::HarnessRunner> {
        let mut runner = crate::skill_task::HarnessRunner::lamp(home, harness);
        runner.makes = true;
        Arc::new(runner)
    }

    /// With `runner` (its `makes` on) as both the pick and the turn.
    pub fn of(runner: Arc<crate::skill_task::HarnessRunner>) -> Self {
        Self::picking(runner, false)
    }

    /// [`Self::of`], with the lamp's Hermes picked as a gateway agent while
    /// its direct session is on (`crate::direct::takes_runs`): a make goes
    /// into its kept session. A live ask keeps [`Self::of`]: it is not on
    /// the direct path.
    pub fn of_direct(runner: Arc<crate::skill_task::HarnessRunner>) -> Self {
        Self::picking(runner, true)
    }

    fn picking(runner: Arc<crate::skill_task::HarnessRunner>, hermes_direct: bool) -> Self {
        let looks = runner.clone();
        Self {
            pick: Box::new(move || match looks.make_agent()? {
                Some((engine, true)) => Ok(MakeAgent::Gateway(engine)),
                Some((engine @ crate::harness::EngineKind::Hermes, false))
                    if hermes_direct && crate::direct::takes_runs(engine, false) =>
                {
                    Ok(MakeAgent::Gateway(engine))
                }
                // Hermes lamps unchanged (#7245's done-when): the lamp's
                // Hermes, and with Harness not set up (no gateway agent
                // can be reached), as without the field, with no Harness
                // check.
                Some((crate::harness::EngineKind::Hermes, false)) | None => Ok(MakeAgent::Hermes),
                Some((engine, false)) => Err(format!(
                    "the lamp's default agent ({}) is neither a gateway agent nor the lamp's Hermes, so a make is not handed to it and nothing was started",
                    engine.argument()
                )),
            }),
            turn: runner,
            checked: crate::openclaw_config::v1_tools_on(
                std::env::var(crate::openclaw_config::V1_TOOLS_SWITCH)
                    .ok()
                    .as_deref(),
            ),
        }
    }
}

impl Runner for MakeGateway {
    fn run(&self, run: &SkillRun, _end_token: &dyn Fn()) -> Result<u64, String> {
        self.turn.make_turn(run)
    }

    fn stop_on(&self, stop: Arc<AtomicBool>) {
        self.turn.stop_on(stop);
    }
}

/// Whether `caller` is [`CALLER_UNIT`] run as root; an unknown caller is
/// refused.
pub fn caller_may(caller: Option<&lamp_ipc::caller::Caller>) -> bool {
    caller.is_some_and(|caller| caller.uid == 0 && caller.unit.as_deref() == Some(CALLER_UNIT))
}

/// The switch for a make's network: the first builder's 19:14 ruling on
/// #6643 keeps it (the owner's ask, as V1's agent had it), on the
/// condition that each make has a fresh home and work folder, dropped
/// after ([`put`]). Exactly [`NET_HELD`] holds the run off it
/// ([`crate::skill_task::NET_ENV`] `run`: only `lo` and the kernel's
/// `@NONE` tunnels, the model through `model.sock`); unset or anything
/// else, the run keeps the lamp's network. Set in no unit.
pub const NET_SWITCH: &str = "LAMP_MAKE_AGENT_NET";
/// [`NET_SWITCH`]'s one value that holds the run off the network.
pub const NET_HELD: &str = "held";

/// Whether `value` of [`NET_SWITCH`] holds a make's run off the network:
/// [`NET_HELD`] exactly.
pub fn held(value: Option<&str>) -> bool {
    value == Some(NET_HELD)
}

/// The lamp's Hermes for a make, and only Hermes: it alone has a namespace
/// runner today ([`crate::skill_task::HermesRunner`]); the voice hands a
/// make over only when the lamp's agent is Hermes. Before every make,
/// held or not, its wrapper and namespace script must carry the make-home
/// lines ([`WRAPPER_HOME_LINE`], [`NS_HOME_LINE`]: lamp-update's, so the
/// wrapper is its namespacing one, never Hermes's own program), or, with
/// lamp-update's one run path, the one run path's Hermes wrapper with its
/// make-home line ([`ONE_PATH_HOME_LINE`]) and the one agent namespace
/// with Hermes's make lines ([`ONE_PATH_NS_MAKE_LINES`]); and, held, the
/// network lines too (the one run path holds only a skill's run off the
/// network, so a held make is refused there).
pub struct MakeHermes {
    pub hermes: crate::skill_task::HermesRunner,
    /// The work folder the wrapper takes a make's home in ([`MAKE_WORK`]);
    /// the runner's own must be it.
    pub make_work: PathBuf,
    /// Hermes's wrapper ([`crate::skill_task::HERMES`]).
    pub wrapper: PathBuf,
    /// Its namespace script ([`NS`]).
    pub ns: PathBuf,
    /// The one agent namespace ([`crate::skill_task::AGENT_NS`]), which
    /// the wrapper runs instead with the one run path.
    pub agent_ns: PathBuf,
    /// A task's run granted `net` ([`net_runner`]): held as every run, and
    /// given its own egress socket ([`crate::egress::Grant::open`]) while
    /// it runs, named to the namespace script by
    /// [`crate::egress::EGRESS_ENV`]. None: no way out but the model's.
    pub egress: Option<crate::egress::Grant>,
}

impl Runner for MakeHermes {
    fn ready(&self) -> Result<(), String> {
        self.hermes.ready()?;
        let has = |path: &Path, line: &str| {
            std::fs::read_to_string(path).is_ok_and(|text| text.contains(line))
        };
        let own = has(&self.wrapper, WRAPPER_HOME_LINE) && has(&self.ns, NS_HOME_LINE);
        let one_path = std::fs::read_to_string(&self.wrapper).is_ok_and(|text| {
            crate::skill_task::wrapper_takes_runs(&text, crate::harness::EngineKind::Hermes)
                && text.contains(ONE_PATH_HOME_LINE)
        }) && has(&self.agent_ns, ONE_PATH_NS_MAKE_LINES);
        if self.hermes.folder != self.make_work || !(own || one_path) {
            return Err(NO_NAMESPACE.into());
        }
        if self.egress.is_some()
            && !(has(&self.ns, crate::egress::NS_LINE)
                && has(&self.ns, crate::egress::NS_OWN_SKILL_LINE))
        {
            return Err(crate::egress::NO_EGRESS.into());
        }
        Ok(())
    }

    /// A task's run that tells ([`SkillRun::tells`]) keeps what its agent
    /// wrote to its own folder's [`ANSWER_FILE`] ([`Self::tell_words`]), as
    /// a make's answer is read: Hermes's chat answer goes nowhere here.
    /// Never a make's: its answer is its own.
    fn answers(&self) -> bool {
        true
    }

    fn tell_words(&self, start_in: Option<&Path>) -> String {
        match start_in {
            Some(folder) => crate::skill_task::tell_file_line(&folder.join(ANSWER_FILE)),
            None => crate::skill_task::TELL_LINE.to_owned(),
        }
    }

    /// With [`MakeHermes::egress`], the run's egress socket is made for its
    /// own folder (`make-<run id>`) before Hermes starts and removed when
    /// it ends, whatever the end; nothing piped through it outlives the
    /// run's deadline. A run that tells and ended well has its agent's
    /// answer read from that folder, before the folder is removed.
    fn run(&self, run: &SkillRun, end_token: &dyn Fn()) -> Result<u64, String> {
        let ran = self.run_held(run, end_token);
        if ran.is_ok() {
            keep_told_answer(run);
        }
        ran
    }
}

/// A task's run that tells ([`SkillRun::tells`]) and ended well, through a
/// make's runner: what its agent wrote to its own folder's [`ANSWER_FILE`]
/// kept as its answer ([`SkillRun::answer`]), read as a make's answer is
/// ([`answer_in`]: a regular file, never through a link, not empty, at most
/// [`MOST_ANSWER_BYTES`]). Nothing for any other run.
pub(crate) fn keep_told_answer(run: &SkillRun) {
    let Some(folder) = run.start_in.as_deref().filter(|_| run.tells) else {
        return;
    };
    let answer = answer_in(folder);
    // Its length only, never its words (#7180).
    eprintln!(
        "[lamp-tasks] skill task {}: its agent's answer: {}",
        run.run,
        answer.as_ref().map_or_else(
            || format!("none in its {ANSWER_FILE}"),
            |answer| format!("{} bytes kept", answer.len())
        )
    );
    if let Some(answer) = answer {
        let _ = run.answer.set(answer);
    }
}

impl MakeHermes {
    fn run_held(&self, run: &SkillRun, end_token: &dyn Fn()) -> Result<u64, String> {
        self.ready()?;
        let Some(grant) = &self.egress else {
            return self.hermes.run(run, end_token);
        };
        let name = run
            .start_in
            .as_deref()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .filter(|name| crate::egress::plain_name(name))
            .ok_or_else(|| "a run granted net has no folder of its own".to_owned())?;
        let until = std::time::Instant::now() + run.deadline;
        let served = grant
            .open_for(name, until, &run.folder)
            .map_err(|error| format!("the run's way out was not made: {}", error.kind()))?;
        let mut hermes = self.hermes.clone();
        hermes
            .env
            .push((crate::egress::EGRESS_ENV.into(), name.to_owned()));
        let ran = hermes.run(run, end_token);
        drop(served);
        ran
    }
}

/// The lamp's Hermes for a make ([`crate::skill_task::HermesRunner::lamp`]),
/// started through its namespace wrapper either way (only its own folder,
/// its lasting home hidden from a make but for the checkout, its work
/// folder, all but the make's own read-only, and the skills, `control.sock`
/// alone of lamp-control's and never `admin.sock`), with the make's own
/// home. `held`: with [`crate::skill_task::NET_ENV`] `run`, refused when
/// the wrapper can't hold it off. Not held: without it, so the wrapper
/// keeps the lamp's network, as Harness's own panes do (the model then
/// reached at lamp-cloud's forwarder, the one `model.sock` pipes to).
pub fn lamp_runner(home: &Path, held: bool) -> MakeHermes {
    lamp_runner_with(home, held, None)
}

/// The runner a task's run granted `net` goes through (the design note on
/// #7200): [`lamp_runner`] held, so `--net`, only `lo` and the model
/// relay as every run, with [`crate::egress::Grant::lamp`]: its own
/// egress socket, the public web on ports 80 and 443 alone.
pub fn net_runner(home: &Path) -> MakeHermes {
    lamp_runner_with(home, true, Some(crate::egress::Grant::lamp()))
}

/// The runner a task's run granted `net` goes through with
/// [`NET_SWITCH`](crate::skill_task::NET_SWITCH) on (#7200). With
/// lamp-update's one run path (`one_path`'s
/// [`crate::skill_task::HarnessRunner::default_agent`]), the lamp's own
/// Harness runner, as every skill's run: `harness new` (or the wrapper's
/// task entry with no Harness) in the run's `skill-<run id>` folder, its
/// egress socket `skill-<run id>.sock` made first
/// ([`crate::skill_task::HarnessRunner::egress`]), which the one run path's
/// wrapper finds by the run's id with its fresh home `make-<run id>/home`,
/// as Harness passes it no variable of lamp-tasks'; refused in words for an
/// agent that can't take it. Without the one run path, `make`
/// ([`net_runner`]): Hermes's own wrapper, started by lamp-tasks with
/// `LAMP_NS_EGRESS` in its environment.
pub struct NetRunner {
    pub make: MakeHermes,
    pub one_path: Option<Arc<crate::skill_task::HarnessRunner>>,
}

impl NetRunner {
    fn by_one_path(&self) -> Option<&crate::skill_task::HarnessRunner> {
        self.one_path
            .as_deref()
            .filter(|harness| harness.default_agent)
    }
}

impl Runner for NetRunner {
    /// By the one run path, what the wrapper and namespace script carry is
    /// checked at the run's start, with the default agent then known
    /// ([`crate::skill_task::takes_net_runs`]); else the make's net runner's
    /// checks.
    fn ready(&self) -> Result<(), String> {
        match self.by_one_path() {
            Some(_) => Ok(()),
            None => self.make.ready(),
        }
    }

    /// As the runner it goes through keeps its agent's last answer, so a
    /// `net` run tells what it found ([`crate::skill_task::tells_answer`]):
    /// by the one run path the lamp's Harness runner, from its chat; else
    /// the make's net runner, from the run's own `answer.md`.
    fn answers(&self) -> bool {
        match self.by_one_path() {
            Some(harness) => harness.answers(),
            None => self.make.answers(),
        }
    }

    fn tell_words(&self, start_in: Option<&Path>) -> String {
        match self.by_one_path() {
            Some(harness) => harness.tell_words(start_in),
            None => self.make.tell_words(start_in),
        }
    }

    fn run(&self, run: &SkillRun, end_token: &dyn Fn()) -> Result<u64, String> {
        match self.by_one_path() {
            Some(harness) => harness.run(run, end_token),
            None => self.make.run(run, end_token),
        }
    }
}

/// The runner a live ask held off the network goes through
/// (`crate::voice_ask`, an ask with a linked service's read). With
/// lamp-update's one run path (`one_path`'s
/// [`crate::skill_task::HarnessRunner::default_agent`]), the lamp's Hermes
/// by its wrapper's task entry, as a skill's run with no Harness
/// ([`crate::skill_task::HarnessRunner::run_task_entry`]): in the run's own
/// `skill-<run id>` folder, which the wrapper holds off the network (`--net`,
/// its one-run token read from its file), with the ask's folder
/// `make-<run id>/home` as its fresh home, named to the wrapper as a make's
/// is (`LAMP_NS_HOME`), and no egress socket: the one run path holds only a
/// run started in its own folder, so `make` would be refused
/// [`NOT_HELD_OFF_ONE_PATH`] there. Never through Harness, which passes the
/// wrapper none of lamp-tasks' variables. Without the one run path, `make`
/// ([`lamp_runner`] held), as before.
pub struct AskRunner {
    pub make: MakeHermes,
    pub one_path: Option<Arc<crate::skill_task::HarnessRunner>>,
    /// lamp-tasks' `HOME`, the wrapper's `HOME` as it is started.
    pub home: PathBuf,
}

impl AskRunner {
    /// The lamp's: [`lamp_runner`] held, and the one run path's runner
    /// ([`crate::skill_task::HarnessRunner::lamp`]) on `harness`.
    pub fn lamp(home: &Path, harness: crate::skill_task::SkillHarnessSlot) -> Self {
        Self {
            make: lamp_runner(home, true),
            one_path: Some(Arc::new(crate::skill_task::HarnessRunner::lamp(
                home, harness,
            ))),
            home: home.to_path_buf(),
        }
    }

    fn by_one_path(&self) -> Option<&crate::skill_task::HarnessRunner> {
        self.one_path
            .as_deref()
            .filter(|harness| harness.default_agent)
    }
}

impl Runner for AskRunner {
    /// By the one run path, the lamp's Hermes wrapper takes a run by its
    /// task entry in a fresh home of its own
    /// ([`crate::skill_task::HarnessRunner::takes_fresh_tasks`]); else the
    /// held make's checks.
    fn ready(&self) -> Result<(), String> {
        match self.by_one_path() {
            Some(harness) => harness.takes_fresh_tasks(crate::harness::EngineKind::Hermes),
            None => self.make.ready(),
        }
    }

    fn stop_on(&self, stop: Arc<AtomicBool>) {
        if let Some(harness) = &self.one_path {
            Runner::stop_on(harness.as_ref(), stop);
        }
    }

    fn run(&self, run: &SkillRun, end_token: &dyn Fn()) -> Result<u64, String> {
        match self.by_one_path() {
            Some(harness) => harness.run_task_entry(
                crate::harness::EngineKind::Hermes,
                &self.home,
                run,
                end_token,
            ),
            None => self.make.run(run, end_token),
        }
    }
}

fn lamp_runner_with(home: &Path, held: bool, egress: Option<crate::egress::Grant>) -> MakeHermes {
    let mut hermes = crate::skill_task::HermesRunner::lamp(home);
    if !held {
        hermes
            .env
            .retain(|(name, _)| name != crate::skill_task::NET_ENV);
        hermes.held_by = None;
    }
    MakeHermes {
        hermes,
        make_work: PathBuf::from(MAKE_WORK),
        wrapper: PathBuf::from(crate::skill_task::HERMES),
        ns: PathBuf::from(NS),
        agent_ns: PathBuf::from(crate::skill_task::AGENT_NS),
        egress,
    }
}

/// The voice's makes served: one line a connection, answered with
/// [`answer`], one make at a time (a second is refused `busy`).
pub struct MakeHandOvers {
    pub socket: PathBuf,
    /// The lamp's work folder, where each make's folder is made.
    pub work: PathBuf,
    pub runner: Arc<dyn Runner + Send + Sync>,
    /// #7245: where a line with `"one_path":true` goes ([`answer_with`]);
    /// `None`, every make to [`Self::runner`].
    pub one_path: Option<Arc<MakeGateway>>,
    pub caller: fn(Option<&lamp_ipc::caller::Caller>) -> bool,
    /// A live spoken ask's line (`"op":"voice_ask"`, `crate::voice_ask`),
    /// taken only with one of its switches on; `None`, refused `off` as
    /// any line that is not a make's.
    pub asks: Option<Arc<crate::voice_ask::Asks>>,
    /// A made skill's tried run before the voice keeps it (#8157,
    /// `"op":"make_try"`, `crate::make_try`); `None`, refused `off`.
    pub tries: Option<Arc<crate::make_try::Tries>>,
}

impl MakeHandOvers {
    /// The lamp's: [`MAKE_SOCKET`], the lamp's Hermes in its namespace
    /// ([`lamp_runner`], the network as [`NET_SWITCH`] says) and its work
    /// folder, lamp-voice alone as caller.
    pub fn lamp(
        home: &Path,
        harness: crate::skill_task::SkillHarnessSlot,
        default_engine: crate::voice_ask::DefaultEngineSlot,
    ) -> Self {
        let held = held(std::env::var(NET_SWITCH).ok().as_deref());
        eprintln!(
            "[lamp-tasks] skill make: a make's run on the lamp's Hermes (its namespace path only; a gateway agent's make on the one run path is its gateway's own turn) {}",
            if held {
                "is held off the network (LAMP_MAKE_AGENT_NET=held)"
            } else {
                "keeps the network (LAMP_MAKE_AGENT_NET is not held)"
            }
        );
        let runner: Arc<MakeHermes> = Arc::new(lamp_runner(home, held));
        // One runner for a gateway agent's makes and asks; only a make goes
        // to the lamp's Hermes's direct session (`LAMP_TASKS_HERMES_DIRECT`).
        let make_runner = MakeGateway::lamp_runner(home, harness.clone());
        let gateway = Arc::new(MakeGateway::of_direct(make_runner.clone()));
        let ask_gateway = Arc::new(MakeGateway::of(make_runner));
        let switches = crate::voice_ask::Switches::of_env();
        // A live ask on the lamp's default agent, picked as a make on the
        // one run path picks it: the lamp's Hermes on the same runner as a
        // make, one with a linked service's grants always held off the
        // network; a gateway agent through the same one turn.
        let asks = switches.any().then(|| {
            eprintln!(
                "[lamp-tasks] skill make: live asks for the lamp's agent are taken (hard asks {}, linked services {})",
                switches.hard, switches.services
            );
            Arc::new(crate::voice_ask::Asks {
                switches,
                open: runner.clone(),
                held: Arc::new(AskRunner::lamp(home, harness.clone())),
                agent: Some(ask_gateway),
                default_engine: Box::new(move || {
                    default_engine.get().and_then(|engine| engine())
                }),
                tokens: Arc::new(crate::skill_task::ControlTokens::default()),
                asking: AtomicBool::new(false),
                news: Default::default(),
            })
        });
        let work = runner.hermes.folder.clone();
        Self {
            socket: PathBuf::from(MAKE_SOCKET),
            work: work.clone(),
            runner,
            one_path: Some(gateway),
            caller: caller_may,
            asks,
            tries: Some(Arc::new(crate::make_try::Tries {
                work,
                launch: Box::new(crate::make_try::FenceLaunch::lamp()),
                busy: AtomicBool::new(false),
            })),
        }
    }

    /// With the switch `on`, the socket bound and served on a thread of its
    /// own until `stop`, the socket removed then; off, nothing is bound.
    pub fn start(
        self,
        on: bool,
        stop: Arc<AtomicBool>,
    ) -> std::io::Result<Option<std::thread::JoinHandle<()>>> {
        if !on {
            return Ok(None);
        }
        // What a lamp-tasks stopped mid-make left, before any make of its own.
        sweep(&self.work);
        // A gateway make's session lives in Harness's daemon: ended as
        // lamp-tasks stops (#7245).
        if let Some(gateway) = &self.one_path {
            gateway.stop_on(stop.clone());
        }
        // A held ask's run on the one run path ends as lamp-tasks stops.
        if let Some(asks) = &self.asks {
            asks.held.stop_on(stop.clone());
        }
        let (listener, binding) = crate::runtime::bind(&self.socket)?;
        let served = std::thread::Builder::new()
            .name("tasks-skill-make".into())
            .spawn(move || {
                let _binding = binding;
                self.accept(&listener, &stop);
            })?;
        Ok(Some(served))
    }

    fn accept(self, listener: &std::os::unix::net::UnixListener, stop: &AtomicBool) {
        let this = Arc::new(self);
        let running = Arc::new(AtomicBool::new(false));
        let open = Arc::new(AtomicUsize::new(0));
        while !stop.load(Ordering::Acquire) {
            let stream = match listener.accept() {
                Ok((stream, _)) => stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(ACCEPT_TICK);
                    continue;
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    eprintln!("[lamp-tasks] skill make: accept: {error}");
                    std::thread::sleep(ACCEPT_TICK);
                    continue;
                }
            };
            let caller = lamp_ipc::caller::of_stream(&stream);
            if !(this.caller)(caller.as_ref()) {
                eprintln!(
                    "[lamp-tasks] skill make: refused a caller that is not {CALLER_UNIT} (pid {})",
                    caller.map_or_else(|| "unknown".into(), |caller| caller.pid.to_string())
                );
                continue;
            }
            if stream.set_nonblocking(false).is_err()
                || stream.set_read_timeout(Some(LINE_WAIT)).is_err()
                || stream.set_write_timeout(Some(LINE_WAIT)).is_err()
            {
                continue;
            }
            if open.fetch_add(1, Ordering::AcqRel) >= MOST_CONNECTIONS {
                open.fetch_sub(1, Ordering::AcqRel);
                write_answer(&stream, &refusal("busy", BUSY));
                continue;
            }
            let held = Counted(open.clone());
            let (served, running) = (this.clone(), running.clone());
            let _ = std::thread::Builder::new()
                .name("tasks-skill-make-run".into())
                .spawn(move || {
                    let _held = held;
                    served.one(&stream, &running);
                });
        }
    }

    fn one(&self, stream: &UnixStream, running: &AtomicBool) {
        let line = match read_line_within(stream, MOST_LINE_BYTES) {
            Ok(line) => line,
            Err(refused) => {
                write_answer(stream, &refused);
                return;
            }
        };
        // A made skill's tried run (#8157): asked after its make's run has
        // ended, with its own one place.
        if crate::make_try::is_try(&line) {
            let answered = match &self.tries {
                Some(tries) => tries.answer(&line),
                None => refusal("off", "a made skill's tried run is off on this lamp"),
            };
            eprintln!(
                "{}",
                journal_line(&answered).replace("skill make:", "make try:")
            );
            write_answer(stream, &answered);
            return;
        }
        // A live ask has its own one place, beside a make's.
        if crate::voice_ask::is_ask(&line) {
            let answered = match &self.asks {
                Some(asks) => crate::voice_ask::answer_beside_make(
                    &line,
                    asks,
                    running.load(Ordering::Acquire),
                    &self.work,
                    &mut || {
                        write_answer(stream, &json!({"ok": true, "started": true}));
                    },
                ),
                None => refusal(
                    "off",
                    "an ask for the lamp's agent is off on this lamp (LAMP_HARD_ASKS_TO_AGENT, LAMP_SERVICES_TO_AGENT)",
                ),
            };
            eprintln!(
                "{}",
                journal_line(&answered).replace("skill make:", "live ask:")
            );
            write_answer(stream, &answered);
            return;
        }
        if running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            eprintln!("[lamp-tasks] skill make: refused busy, one is being made");
            write_answer(stream, &refusal("busy", BUSY));
            return;
        }
        let place = Place(running);
        let answered = answer_with(
            &line,
            true,
            &self.work,
            &*self.runner,
            self.one_path.as_deref(),
            &mut || {
                write_answer(stream, &json!({"ok": true, "started": true}));
            },
        );
        drop(place);
        eprintln!("{}", journal_line(&answered));
        write_answer(stream, &answered);
    }
}

/// The journal's line for a make's outcome: its code and, refused, the
/// refusal's reason in one line. Every reason is a fixed form (a path or
/// an exit status at most), never the rules or the answer.
fn journal_line(answered: &Value) -> String {
    if answered["ok"].as_bool() == Some(true) {
        return "[lamp-tasks] skill make: the run ended with an answer".into();
    }
    let why: String = answered["message"]
        .as_str()
        .unwrap_or("?")
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    format!(
        "[lamp-tasks] skill make: refused {}: {why}",
        answered["error"].as_str().unwrap_or("?")
    )
}

/// One of [`MOST_CONNECTIONS`] held while its thread lives.
struct Counted(Arc<AtomicUsize>);

impl Drop for Counted {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// The one make's place, let go however it ends.
struct Place<'a>(&'a AtomicBool);

impl Drop for Place<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// One run as a test's runner saw it: its rules, folder, own
    /// environment, deadline and what its home held at its start.
    type SeenRun = (
        String,
        PathBuf,
        Vec<(&'static str, String)>,
        Duration,
        Vec<String>,
    );

    /// What a test's runner saw of a make's runs, kept by it.
    #[derive(Default)]
    struct Seen(Mutex<Vec<SeenRun>>);
    use crate::skill_task::{ADMIN_SOCKET, HERMES, NET_ENV, NET_RUN, TOKEN_ENV, WRAPPER_NET_LINES};
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::fs::PermissionsExt;

    /// A `net` run's runner keeps its agent's answer either way, so the run
    /// tells it: by the one run path the lamp's Harness runner, asked for a
    /// last chat answer fit to say; else the make's net runner, asked to
    /// write it to the run's own `answer.md`.
    #[test]
    fn a_net_runner_keeps_an_answer_by_either_path() {
        let home = Path::new("/var/lib/lampos-tasks");
        let folder = Path::new("/w/make-r1");
        let mut harness = crate::skill_task::HarnessRunner::lamp(home, Default::default());
        harness.default_agent = true;
        let by_path = NetRunner {
            make: net_runner(home),
            one_path: Some(Arc::new(harness)),
        };
        assert!(by_path.answers());
        assert_eq!(
            by_path.tell_words(Some(folder)),
            crate::skill_task::TELL_LINE
        );
        let mut off = crate::skill_task::HarnessRunner::lamp(home, Default::default());
        off.default_agent = false;
        for runner in [
            NetRunner {
                make: net_runner(home),
                one_path: Some(Arc::new(off)),
            },
            NetRunner {
                make: net_runner(home),
                one_path: None,
            },
        ] {
            assert!(runner.answers());
            assert_eq!(
                runner.tell_words(Some(folder)),
                crate::skill_task::tell_file_line(&folder.join(ANSWER_FILE))
            );
        }
        assert!(lamp_runner(home, true).answers() && lamp_runner(home, false).answers());
    }

    /// A make's runner (an agent skill's, a keyed run's, a `net` run's off
    /// the one run path) keeps what a task's run that tells wrote to its own
    /// folder's `answer.md` once it ended well; a run that doesn't tell (a
    /// make's own) keeps nothing, nor one that failed.
    #[test]
    fn a_makes_runner_keeps_a_telling_runs_answer_from_its_folder() {
        let dir = tempfile::tempdir().unwrap();
        let work = dir.path().join("work");
        let folder = work.join("make-r1");
        std::fs::create_dir_all(&folder).unwrap();
        let net = format!("{}{}", WRAPPER_NET_LINES[0], WRAPPER_NET_LINES[1]);
        let marked =
            format!("#!/bin/sh\n{WRAPPER_HOME_LINE}unset T\n{net}--pid hermes-ns \"$@\"\n");
        let runner = |end: &str| {
            let (w, n) = namespace(dir.path(), &marked, NS_HOME_LINE);
            let mut runner = lamp_runner(dir.path(), false);
            runner.hermes.program = PathBuf::from("/bin/sh");
            runner.hermes.args = vec![
                "-c".into(),
                format!("printf 'Bitcoin is at 91,204 dollars.' > {ANSWER_FILE}; {end}"),
                "sh".into(),
            ];
            runner.hermes.folder = work.clone();
            runner.make_work = work.clone();
            runner.hermes.tick = Duration::from_millis(10);
            (runner.wrapper, runner.ns) = (w, n);
            runner
        };
        let ran = |tells: bool, end: &str| {
            let _ = std::fs::remove_file(folder.join(ANSWER_FILE));
            let mut run = make_run(&folder, Duration::from_secs(5));
            run.tells = tells;
            let ended = runner(end).run(&run, &|| {});
            (ended, run.answer.get().cloned())
        };
        assert_eq!(
            ran(true, "exit 0"),
            (Ok(0), Some("Bitcoin is at 91,204 dollars.".to_owned()))
        );
        assert_eq!(ran(false, "exit 0"), (Ok(0), None));
        assert!(matches!(ran(true, "exit 3"), (Err(_), None)));
        // Left empty (the skill already said it aloud): nothing to tell.
        assert_eq!(ran(true, &format!(": > {ANSWER_FILE}")), (Ok(0), None));
        // 3,000 bytes are kept whole (lamp-tasks carries at most
        // `MOST_TOLD_BYTES`, lamp-skills says about 400 characters of it);
        // past the 20 KiB read, none, as a make's answer.
        let long = ran(
            true,
            &format!("yes 'Bitcoin is at 91,204 dollars.' | head -c 3000 > {ANSWER_FILE}"),
        );
        assert_eq!(long.1.map(|answer| answer.len()), Some(3_000));
        let over = ran(
            true,
            &format!(
                "head -c {} /dev/zero | tr '\\0' a > {ANSWER_FILE}",
                MOST_ANSWER_BYTES + 1
            ),
        );
        assert_eq!(over, (Ok(0), None));
        // Through a link: never followed.
        let elsewhere = dir.path().join("elsewhere.md");
        std::fs::write(&elsewhere, "Not the run's.").unwrap();
        let linked = ran(
            true,
            &format!(
                "rm -f {ANSWER_FILE}; ln -s {} {ANSWER_FILE}",
                elsewhere.display()
            ),
        );
        assert_eq!(linked, (Ok(0), None));
    }

    fn line(rules: &str, within_ms: u64) -> Vec<u8> {
        json!({"op":"skill_make","rules":rules,"within_ms":within_ms})
            .to_string()
            .into_bytes()
    }

    /// Writes `answer` (none: nothing) as the run would, and keeps what it saw.
    struct Writing {
        answer: Option<&'static str>,
        seen: Seen,
        ready: Result<(), String>,
        /// What the run ends with, after it wrote in its home.
        result: Result<u64, String>,
    }

    impl Runner for Writing {
        fn ready(&self) -> Result<(), String> {
            self.ready.clone()
        }

        fn run(&self, run: &SkillRun, _: &dyn Fn()) -> Result<u64, String> {
            let folder = run.start_in.clone().unwrap();
            let home = folder.join(HOME_FOLDER);
            self.seen.0.lock().unwrap().push((
                std::fs::read_to_string(folder.join(RULES_FILE)).unwrap(),
                folder.clone(),
                run.env.clone(),
                run.deadline,
                listing(&home),
            ));
            // What a run leaves in its home: never seen by the next make.
            std::fs::write(home.join("memory.md"), "LEFT BEHIND").unwrap();
            std::fs::write(home.join(HERMES_FOLDER).join("state.db"), "S").unwrap();
            if let Some(answer) = self.answer {
                std::fs::write(folder.join(ANSWER_FILE), answer).unwrap();
            }
            self.result.clone()
        }
    }

    fn writing(answer: Option<&'static str>) -> Writing {
        Writing {
            answer,
            seen: Seen::default(),
            ready: Ok(()),
            result: Ok(0),
        }
    }

    /// What a make's own home held when its run began: every path in it.
    fn listing(home: &Path) -> Vec<String> {
        let mut found = Vec::new();
        let mut folders = vec![home.to_path_buf()];
        while let Some(folder) = folders.pop() {
            for entry in std::fs::read_dir(&folder).unwrap().flatten() {
                let path = entry.path();
                found.push(path.strip_prefix(home).unwrap().display().to_string());
                if entry.file_type().unwrap().is_dir() {
                    folders.push(path);
                }
            }
        }
        found.sort();
        found
    }

    #[test]
    fn a_make_runs_in_its_own_folder_and_its_answer_is_read_back() {
        let work = tempfile::tempdir().unwrap();
        let runner = writing(Some("```skill.toml\nx\n```"));
        let mut started = 0;
        let answer = answer(
            &line("SYSTEM\n\nBRIEF", 3_000),
            true,
            work.path(),
            &runner,
            &mut || started += 1,
        );
        assert_eq!(
            answer,
            json!({"ok": true, "answer": "```skill.toml\nx\n```"})
        );
        assert_eq!(started, 1);
        let seen = runner.seen.0.lock().unwrap();
        let (rules, folder, env, deadline, _) = &seen[0];
        assert_eq!(rules, "SYSTEM\n\nBRIEF");
        assert!(folder.starts_with(work.path()), "{folder:?}");
        // Its own home and nothing else: no token, no grant, no socket.
        let home = folder.join(HOME_FOLDER).display().to_string();
        assert_eq!(
            env,
            &vec![
                ("HOME", home.clone()),
                ("HERMES_HOME", format!("{home}/.hermes")),
                (HOME_ENV, home),
            ]
        );
        assert_eq!(*deadline, Duration::from_secs(3));
        // The folder is gone once the answer is in.
        assert!(!folder.exists());
        assert_eq!(std::fs::read_dir(work.path()).unwrap().count(), 0);
    }

    /// The first builder's 19:14 ruling on #6643: each make starts with a
    /// fresh, empty home but for the provider config (a variable's name for
    /// its key, never a key), never sees what an earlier make left in its
    /// home, and its home is gone after however it ended: answered, failed
    /// or past its deadline.
    #[test]
    fn each_make_has_a_fresh_home_dropped_after() {
        let work = tempfile::tempdir().unwrap();
        for result in [
            Ok(0),
            Err("the lamp's Hermes ended with status 1".to_owned()),
            Err("the run was ended at its deadline, 2 s".to_owned()),
            Ok(0),
        ] {
            let mut runner = writing(Some("A"));
            runner.result = result.clone();
            let answered = answer(&line("R", 2_000), true, work.path(), &runner, &mut || {});
            assert_eq!(answered["ok"], result.is_ok(), "{answered}");
            let seen = runner.seen.0.lock().unwrap();
            let (_, folder, _, _, at_start) = &seen[0];
            assert_eq!(
                at_start,
                &vec![
                    ".hermes".to_owned(),
                    ".hermes/config.yaml".to_owned(),
                    ".hermes/installs".to_owned(),
                    ".hermes/tools".to_owned(),
                ],
                "only the provider config and the runtime links, never what an earlier make left"
            );
            assert!(!folder.exists(), "dropped after {result:?}");
            assert_eq!(std::fs::read_dir(work.path()).unwrap().count(), 0);
        }
        // The provider config names its key's variable, never a key.
        let made = put(work.path(), "R").unwrap();
        let config = std::fs::read_to_string(
            made.path
                .join(HOME_FOLDER)
                .join(HERMES_FOLDER)
                .join(CONFIG_FILE),
        )
        .unwrap();
        assert!(
            config.contains("key_env: \"LAMP_HERMES_TOKEN\""),
            "{config}"
        );
        assert!(config.contains("127.0.0.1:7741"), "{config}");
        // Hermes's prepared runtime (#6643, c0a4): its dependency record
        // and its store, each a link to the lasting home's own, which the
        // namespace shows read-only; never its memory, sessions or state.
        let hermes = made.path.join(HOME_FOLDER).join(HERMES_FOLDER);
        for name in ["installs", "tools"] {
            assert_eq!(
                std::fs::read_link(hermes.join(name)).unwrap(),
                Path::new("/var/lib/lampos-hermes/.hermes").join(name),
                "{name}"
            );
        }
        // Each top-level key once: a second `terminal:` is a duplicate key
        // and Hermes refuses the whole file (exit 2, c0a4, 14 Nov).
        let mut tops: Vec<&str> = config
            .lines()
            .filter(|line| !line.starts_with([' ', '#', '-']) && line.contains(':'))
            .map(|line| line.split(':').next().unwrap())
            .collect();
        let all = tops.len();
        tops.sort_unstable();
        tops.dedup();
        assert_eq!(tops.len(), all, "{config}");
        assert!(
            config.contains("\nterminal:\n") || config.starts_with("terminal:\n"),
            "{config}"
        );
        // lamp-update's four settings, the same as its own.
        for (key, value) in MAKE_CONFIG {
            let (block, name) = key.split_once('.').unwrap();
            assert!(
                config.contains(&format!("\n{block}:\n  {name}: {value}\n")),
                "{config}"
            );
        }
        let source = include_str!("../../update/src/hermes_install.rs");
        let theirs = source
            .split("pub const CONFIG: [(&str, &str); 4] = [\n")
            .nth(1)
            .and_then(|rest| rest.split("];").next())
            .unwrap();
        let ours: String = MAKE_CONFIG
            .iter()
            .map(|(key, value)| format!("    ({key:?}, {value:?}),\n"))
            .collect();
        assert_eq!(theirs, ours);
        assert!(!config.contains("tok_"), "{config}");
        use std::os::unix::fs::MetadataExt;
        for (path, mode) in [
            (made.path.clone(), 0o700),
            (made.path.join(HOME_FOLDER), 0o700),
            (made.path.join(HOME_FOLDER).join(HERMES_FOLDER), 0o700),
            (made.path.join(RULES_FILE), 0o600),
        ] {
            assert_eq!(
                std::fs::metadata(&path).unwrap().mode() & 0o777,
                mode,
                "{path:?}"
            );
        }
        let path = made.path.clone();
        drop(made);
        assert!(!path.exists());
        // A panic while the make runs drops it too.
        let panicking = std::panic::catch_unwind(|| {
            let made = put(work.path(), "R").unwrap();
            let _keep = &made;
            panic!("in the run");
        });
        assert!(panicking.is_err());
        assert_eq!(std::fs::read_dir(work.path()).unwrap().count(), 0);
    }

    /// A make's folder as its run saw it at its start: every path in it,
    /// the run's words and the guide's text, if any.
    type Looked = (Vec<String>, String, Option<String>);

    /// Every make's folder and words, as each run saw them.
    struct Looking {
        seen: Mutex<Vec<Looked>>,
    }

    impl Runner for Looking {
        fn ready(&self) -> Result<(), String> {
            Ok(())
        }

        fn run(&self, run: &SkillRun, _: &dyn Fn()) -> Result<u64, String> {
            let folder = run.start_in.clone().unwrap();
            use std::os::unix::fs::MetadataExt;
            let guide = folder.join(GUIDE_FILE);
            if guide.exists() {
                assert_eq!(std::fs::metadata(&guide).unwrap().mode() & 0o777, 0o600);
            }
            self.seen.lock().unwrap().push((
                listing(&folder),
                run.prompt.clone(),
                std::fs::read_to_string(guide).ok(),
            ));
            std::fs::write(folder.join(ANSWER_FILE), "A").unwrap();
            Ok(0)
        }
    }

    /// #8610: the shipped recipe teaches that a made skill a time, an event
    /// or a repeat starts speaks only through the told answer: its rule
    /// paragraph, a `run` step and code that says its line only for a
    /// skill the person's words start, and an example a time starts whose
    /// task's script only prints, with no `speak` grant.
    #[test]
    fn the_guide_says_a_made_tell_me_skill_speaks_only_through_the_told_answer() {
        let flat = GUIDE.split_whitespace().collect::<Vec<_>>().join(" ");
        for told in [
            "**A skill a time, an event or a repeat starts (any `[trigger]`) speaks only \
             through the lamp's told answer.**",
            "Its scripts print what they found and never call `lamp.say`, `lamp.sound`, the \
             speaker or `control.sock`, and its grants never name `speak` or `speaker`.",
            "only when the owner is alone in view, no one has spoken for a minute, and it \
             isn't quiet hours",
            "only in a skill the person's own words start (`asks`, no trigger)",
            "Started by a time, an event or a repeat: `grants = [\"net\"]`, a `task` step, \
             and scripts/main.py fetches it and prints it; examples/morning-headline.",
            "only in a skill the person's words start, a script says it with `lamp.say`",
        ] {
            assert!(flat.contains(told), "{told}");
        }
        let file = |name: &str| {
            MAKE_EXAMPLES
                .iter()
                .find(|(path, _)| *path == format!("morning-headline/{name}"))
                .map(|(_, text)| *text)
                .unwrap_or_else(|| panic!("morning-headline/{name}"))
        };
        let toml: toml::Table = toml::from_str(file("skill.toml")).unwrap();
        assert!(toml.contains_key("trigger"));
        assert_eq!(toml["grants"].as_array().unwrap().len(), 1);
        assert_eq!(toml["grants"][0].as_str(), Some("net"));
        assert!(toml["do"][0]["task"]["run"].as_str() == Some("scripts/main.py"));
        let main = file("scripts/main.py");
        assert!(main.contains("print("), "{main}");
        for never in ["lamp.say", "lamp.sound", "speaker", "control.sock", "Lamp"] {
            assert!(!main.contains(never), "{never}: {main}");
        }
        // The examples that speak from their code are all started by words.
        for (path, text) in MAKE_EXAMPLES {
            if path.ends_with("skill.toml") && text.contains("[trigger]") {
                assert!(!text.contains("run = \"scripts/main.py\"\n"), "{path}");
                assert!(!text.contains("\"speak\""), "{path}");
            }
        }
    }

    /// `LAMP_MAKE_AGENT_GUIDE` (the owner's ask, 13 Nov: make skills as
    /// V1's skill-creator did): the guide is given only when the line asks
    /// for it, beside the rules, 0600, the run told to draft, check with
    /// the lamp's own check and fix before it answers. Without it, or with
    /// `false`, the run's words and its folder are today's, byte for byte.
    #[test]
    fn the_guide_is_given_only_when_the_line_asks_for_it() {
        let work = tempfile::tempdir().unwrap();
        let looking = Looking {
            seen: Mutex::default(),
        };
        let guided = |guide: Value| {
            json!({"op":"skill_make","rules":"R","within_ms":2_000,"guide":guide})
                .to_string()
                .into_bytes()
        };
        for asked in [line("R", 2_000), guided(json!(false)), guided(json!(true))] {
            let answered = answer(&asked, true, work.path(), &looking, &mut || {});
            assert_eq!(answered, json!({"ok": true, "answer": "A"}));
        }
        let seen = looking.seen.lock().unwrap();
        let today = [
            "home",
            "home/.hermes",
            "home/.hermes/config.yaml",
            "home/.hermes/installs",
            "home/.hermes/tools",
            "rules.md",
        ];
        for (files, words, guide) in &seen[..2] {
            assert_eq!(files, &today);
            assert_eq!(guide, &None);
            let folder = words
                .split("in the file rules.md in this folder, ")
                .nth(1)
                .and_then(|rest| rest.split(". Read it").next())
                .unwrap();
            assert_eq!(
                words,
                &format!(
                    "Everything you need is in the file rules.md in this folder, {folder}. Read it \
                     and answer exactly as it says: the fenced blocks, or its one line when the \
                     skill can't be made. Write that answer, and nothing else, to the file \
                     answer.md in this folder. Write no other file anywhere, install nothing and \
                     run nothing else."
                )
            );
        }
        let (files, words, guide) = &seen[2];
        assert_eq!(
            files,
            &[
                "guide.md",
                "home",
                "home/.hermes",
                "home/.hermes/config.yaml",
                "home/.hermes/installs",
                "home/.hermes/tools",
                "rules.md",
            ]
        );
        assert_eq!(guide.as_deref(), Some(GUIDE));
        for told in [
            "guide.md",
            "rules.md",
            "/opt/lampOS/skills/current/lamp-skills pack --out /tmp/lamp-check draft/NAME",
            "at most twice",
            "answer.md",
            "run no other command",
        ] {
            assert!(words.contains(told), "{told}: {words}");
        }
        // The guide names its origin and license first, and the same check.
        assert!(GUIDE.starts_with(
            "<!-- Adapted from V1's skills/skill-creator/SKILL.md (autonomous-ai/lampOS, main), \
             Apache License 2.0"
        ));
        assert!(GUIDE.contains(&format!(
            "{CHECK_PROGRAM} pack --out {CHECK_OUT} draft/NAME"
        )));
        // #7874: how a made skill runs, and the `task` words' rule.
        let flat = GUIDE.split_whitespace().collect::<Vec<_>>().join(" ");
        for told in [
            "## How a made skill runs",
            "`Run <skill-start> <run id>`",
            "The agent sees only what the script prints,",
            "a lone dash (-) is said as nothing",
            "one plain sentence a person understands, at most 120 characters",
            "`await lamp.service_read(service, args)`",
            "own code granted `net`",
            "`HTTPS_PROXY`",
            "examples/agenda-count",
            "examples/exchange-rate, sun-times",
            // #8157: two step kinds, scripts written at the make, a made
            // SKILL.md's three sections, its tried run, its own folder.
            "written exactly `run = \"scripts/main.py\"`",
            "calls `lamp.say` for its success line and for its failure line",
            "when each start needs the lamp's agent to sum up or reason",
            "Scripts are written at the make, never at a run",
            "`## Uses`",
            "`## Result`",
            "`## Answer`",
            "**Tried before it is kept**",
            // #8180: a try is a real run, with every declared grant.
            "The try is a real run: it has every grant the skill declares",
            "never edit, patch or write the lamp's own skills",
        ] {
            assert!(flat.contains(told), "{told}");
        }
        // #8157: no linked-service stub, and no engine named: the guide is
        // the same for whichever agent the lamp runs.
        for never in [
            "print(\"The lamp's agent does this task with its lamp-services tools.\")",
            "Said aloud through the lamp.",
            "OpenClaw",
            "Hermes",
        ] {
            assert!(!flat.contains(never), "{never}");
        }
        // #8157: a script waits for each line it says, so it never exits
        // before its say is said (a say dropped fails the run); each
        // example's `lamp.say` does.
        assert!(flat.contains("Always pass `wait=True`"), "{flat}");
        for (path, text) in MAKE_EXAMPLES {
            for line in text.lines().filter(|line| line.contains("lamp.say(")) {
                assert!(line.contains("wait=True"), "{path}: {line}");
            }
        }
        // Anything but a boolean is no make's line.
        let refused = answer(
            &guided(json!("yes")),
            true,
            work.path(),
            &looking,
            &mut || {},
        );
        assert_eq!(refused["error"], "bad_request");
        assert_eq!(seen.len(), 3);
        assert_eq!(std::fs::read_dir(work.path()).unwrap().count(), 0);
    }

    /// Heron's P2 on #6768: with the guide, the line's check switches
    /// ([`CHECK_SWITCHES`]) that are `on` are in the run's environment, so
    /// the guide's `lamp-skills pack` loads the draft as the voice's check
    /// does (lamp-skills' `tests/it/make_check.rs` runs both); one `off`
    /// or not given is not there. Only those names, only `"on"` or
    /// `"off"`, each once and only beside `"guide":true`: anything else is
    /// `bad_request`, and starts nothing.
    #[test]
    fn the_checks_switches_reach_the_run_only_as_allowed() {
        let work = tempfile::tempdir().unwrap();
        let runner = writing(Some("A"));
        let make = |extra: &str| {
            format!(r#"{{"op":"skill_make","rules":"R","within_ms":2000,"guide":true{extra}}}"#)
                .into_bytes()
        };
        let base =
            |run: &SeenRun| -> Vec<&'static str> { run.2.iter().map(|(name, _)| *name).collect() };
        for (extra, on) in [
            ("", &[][..]),
            (r#","switches":{}"#, &[]),
            (
                r#","switches":{"LAMP_AGENT_SERVICES":"off","LAMP_SKILL_WRITES":"off"}"#,
                &[],
            ),
            (
                r#","switches":{"LAMP_SKILL_WRITES":"on"}"#,
                &["LAMP_SKILL_WRITES"],
            ),
            (
                r#","switches":{"LAMP_AGENT_SERVICES":"on","LAMP_SKILL_WRITES":"on"}"#,
                &["LAMP_AGENT_SERVICES", "LAMP_SKILL_WRITES"],
            ),
        ] {
            let answered = answer(&make(extra), true, work.path(), &runner, &mut || {});
            assert_eq!(answered, json!({"ok": true, "answer": "A"}), "{extra}");
            let seen = runner.seen.0.lock().unwrap();
            let run = seen.last().unwrap();
            let names = base(run);
            assert_eq!(&names[..3], ["HOME", crate::hold::HERMES_HOME, HOME_ENV]);
            assert_eq!(&names[3..], on, "{extra}");
            assert!(run.2[3..].iter().all(|(_, value)| value == "on"));
        }
        let started = runner.seen.0.lock().unwrap().len();
        for bad in [
            // A name not on the list, however lamp-like.
            r#","switches":{"LAMP_SKILL_TASKS":"on"}"#,
            r#","switches":{"PATH":"/tmp"}"#,
            // A value but "on" or "off".
            r#","switches":{"LAMP_SKILL_WRITES":"yes"}"#,
            r#","switches":{"LAMP_SKILL_WRITES":true}"#,
            r#","switches":{"LAMP_SKILL_WRITES":null}"#,
            r#","switches":{"LAMP_SKILL_WRITES":"ON"}"#,
            // A name twice.
            r#","switches":{"LAMP_SKILL_WRITES":"off","LAMP_SKILL_WRITES":"on"}"#,
            r#","switches":["LAMP_SKILL_WRITES"]"#,
            r#","switches":null"#,
        ] {
            let refused = answer(&make(bad), true, work.path(), &runner, &mut || {});
            assert_eq!(refused["error"], "bad_request", "{bad}");
        }
        // Without the guide no switch is taken: the run runs nothing.
        for unguided in [
            r#"{"op":"skill_make","rules":"R","within_ms":2000,"switches":{"LAMP_SKILL_WRITES":"on"}}"#,
            r#"{"op":"skill_make","rules":"R","within_ms":2000,"guide":false,"switches":{}}"#,
        ] {
            let refused = answer(unguided.as_bytes(), true, work.path(), &runner, &mut || {});
            assert_eq!(refused["error"], "bad_request", "{unguided}");
        }
        assert_eq!(runner.seen.0.lock().unwrap().len(), started);
        assert_eq!(std::fs::read_dir(work.path()).unwrap().count(), 0);
        // The serde names are the list's.
        let all: CheckSwitches = serde_json::from_value(json!({
            CHECK_SWITCHES[0]: "on", CHECK_SWITCHES[1]: "on"
        }))
        .unwrap();
        let named: Vec<&str> = all.env().into_iter().map(|(name, _)| name).collect();
        assert_eq!(named, CHECK_SWITCHES);
    }

    /// A run that can't be held off the network, or whose namespace has no
    /// make-home line, starts nothing and makes no folder; one with no
    /// answer, or an answer that is a link, is `no_answer`; the switch off
    /// and a bad line refuse before anything.
    #[test]
    fn no_namespace_no_make_and_no_answer_is_refused() {
        let work = tempfile::tempdir().unwrap();
        for (why, code) in [
            (NOT_HELD_OFF, "not_held_off"),
            (NO_NAMESPACE, "no_namespace"),
        ] {
            let mut refusing = writing(Some("x"));
            refusing.ready = Err(why.into());
            let mut started = false;
            let refused = answer(&line("R", 2_000), true, work.path(), &refusing, &mut || {
                started = true
            });
            assert_eq!(refused["error"], code);
            assert!(!started);
            assert!(refusing.seen.0.lock().unwrap().is_empty());
            assert_eq!(std::fs::read_dir(work.path()).unwrap().count(), 0);
        }
        let silent = writing(None);
        let refused = answer(&line("R", 2_000), true, work.path(), &silent, &mut || {});
        assert_eq!(refused["error"], "no_answer");
        let off = answer(&line("R", 2_000), false, work.path(), &silent, &mut || {});
        assert_eq!(off["error"], "off");
        for bad in [
            line("", 2_000),
            line("R", 10),
            line("R", 600_001),
            line(&"x".repeat(MOST_RULES_BYTES + 1), 2_000),
            br#"{"op":"skill_make","rules":"R","within_ms":2000,"net":"host"}"#.to_vec(),
            br#"{"op":"skill_task","rules":"R","within_ms":2000}"#.to_vec(),
        ] {
            let refused = answer(&bad, true, work.path(), &silent, &mut || {});
            assert_eq!(refused["ok"], false, "{refused}");
        }
        assert_eq!(silent.seen.0.lock().unwrap().len(), 1);
        // The answer is read from the folder as made, never through a link.
        let made = put(work.path(), "R").unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        std::fs::write(elsewhere.path().join("secret"), "SECRET").unwrap();
        assert_eq!(answer_read(&made.fd), None);
        std::os::unix::fs::symlink(elsewhere.path().join("secret"), made.path.join(ANSWER_FILE))
            .unwrap();
        assert_eq!(answer_read(&made.fd), None, "never through a link");
        std::fs::remove_file(made.path.join(ANSWER_FILE)).unwrap();
        std::fs::write(made.path.join(ANSWER_FILE), "MINE").unwrap();
        // The folder swapped for a link after it was made: still its own.
        let moved = work.path().join("moved");
        std::fs::rename(&made.path, &moved).unwrap();
        std::fs::write(elsewhere.path().join(ANSWER_FILE), "THEIRS").unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), &made.path).unwrap();
        assert_eq!(answer_read(&made.fd).as_deref(), Some("MINE"));
        drop(made);
        std::fs::remove_dir_all(&moved).unwrap();
    }

    /// A whole skill the agent wrote in chat, as answer.md would hold it
    /// (lamp-c0a4, 17 Nov 09:35, OpenClaw deepseek-flash, 731 bytes).
    const CHAT_SKILL: &str = "Here is the skill.\n\n\
        ```toml skill.toml\nname = \"head-pat-next-event\"\n```\n\n\
        ```markdown SKILL.md\n---\nname: head-pat-next-event\n---\nSays the next event.\n```\n\n\
        ```python scripts/main.py\nprint(\"next\")\n```\n\nDone!";

    /// The answer's text when it was taken from the agent's chat.
    fn from_chat(folder: &Path, chat: &str) -> Option<String> {
        match make_answer(folder, Some((chat, false))) {
            Some((text, AnswerFrom::Chat)) => Some(text),
            Some((_, AnswerFrom::File)) => panic!("taken from the file"),
            None => None,
        }
    }

    /// A gateway make turn that writes `answer` to its folder, then ends
    /// with `ended`.
    struct CutTurn {
        answer: &'static str,
        ended: Result<u64, String>,
    }

    impl MakeTurn for CutTurn {
        fn make_turn(&self, run: &SkillRun) -> Result<u64, String> {
            let folder = run.start_in.clone().unwrap();
            std::fs::write(folder.join(ANSWER_FILE), self.answer).unwrap();
            self.ended.clone()
        }
    }

    /// #7245 (First's P3 on #7737): a make cut at its deadline that had
    /// written answer.md gives that answer, not a failure; one not ready
    /// still says so; the folder goes either way.
    #[test]
    fn a_gateway_make_cut_at_its_deadline_still_gives_its_written_answer() {
        let work = tempfile::tempdir().unwrap();
        let line = br#"{"op":"skill_make","rules":"R","within_ms":2000}"#;
        let make = MakeLine::parse(line).unwrap();
        let gateway = |ended: Result<u64, String>| MakeGateway {
            pick: Box::new(|| Ok(MakeAgent::Gateway(crate::harness::EngineKind::Openclaw))),
            turn: Arc::new(CutTurn {
                answer: "```skill.toml\nkind = \"installable\"\n```",
                ended,
            }),
            checked: false,
        };
        let cut = answer_on_gateway(
            &make,
            work.path(),
            &gateway(Err("deadline".into())),
            false,
            &mut || {},
        );
        assert_eq!(cut["ok"], true, "{cut}");
        assert!(cut["answer"].as_str().unwrap().contains("skill.toml"));
        let not_ready = answer_on_gateway(
            &make,
            work.path(),
            &gateway(Err(crate::gateway::NOT_READY.into())),
            false,
            &mut || {},
        );
        assert_eq!(not_ready["error"], NOT_READY, "{not_ready}");
        assert_eq!(std::fs::read_dir(work.path()).unwrap().count(), 0);
    }

    /// lamp-c0a4, 13584, OpenClaw: a make the agent can't do, said
    /// `cannot:` in chat with no `answer.md`, is that line, so the person
    /// hears why, not "try again".
    #[test]
    fn a_gateway_make_with_no_answer_file_takes_its_chat_cannot_line() {
        let work = tempfile::tempdir().unwrap();
        let made = put(work.path(), "R").unwrap();
        let chat = "cannot: I can't do that yet: a skill can't read your phone's battery.\n\nAnything else?";
        assert_eq!(
            from_chat(&made.path, chat).as_deref(),
            Some("cannot: I can't do that yet: a skill can't read your phone's battery.")
        );
        assert_eq!(
            from_chat(&made.path, "  \nCANNOT: no light sensor.").as_deref(),
            Some("CANNOT: no light sensor.")
        );
        // Held as the file is: a reason, no fenced block, within the bound.
        let long = format!("cannot: {}", "x".repeat(MOST_ANSWER_BYTES as usize));
        for chat in [
            "cannot:",
            "cannot:   ",
            "cannot: no\n```toml\n```",
            long.as_str(),
        ] {
            assert_eq!(
                make_answer(&made.path, Some((chat, false))),
                None,
                "{chat:?}"
            );
        }
    }

    /// lamp-c0a4, 17 Nov 09:35: a whole skill the agent wrote in chat,
    /// with no `answer.md`, is the make's answer, words around it and all
    /// (the voice reads its blocks), not `no_answer`.
    #[test]
    fn a_gateway_make_with_no_answer_file_takes_a_skill_said_in_chat() {
        let work = tempfile::tempdir().unwrap();
        let made = put(work.path(), "R").unwrap();
        assert_eq!(
            from_chat(&made.path, CHAT_SKILL).as_deref(),
            Some(CHAT_SKILL)
        );
        // Fences by language, or bare (front matter is `SKILL.md`).
        for chat in [
            "```toml\nname = \"x\"\n```\n```md\n---\nname: x\n---\n```",
            "```\nname = \"x\"\n```\n\n```\n---\nname: x\n---\nIt says.\n```",
            "```skill.toml\nname = \"x\"\n```SKILL.md\n---\nname: x\n---\n```",
        ] {
            assert_eq!(
                from_chat(&made.path, chat).as_deref(),
                Some(chat),
                "{chat:?}"
            );
        }
        // An empty `answer.md` is none: the chat is taken.
        std::fs::write(made.path.join(ANSWER_FILE), " \n").unwrap();
        assert_eq!(
            from_chat(&made.path, CHAT_SKILL).as_deref(),
            Some(CHAT_SKILL)
        );
    }

    /// Any other chat answer is still dropped: a make with no `answer.md`
    /// stays `no_answer`.
    #[test]
    fn a_gateway_make_with_no_answer_file_drops_any_other_chat_answer() {
        let work = tempfile::tempdir().unwrap();
        let made = put(work.path(), "R").unwrap();
        let big = format!("{CHAT_SKILL}\n{}", "x".repeat(MOST_ANSWER_BYTES as usize));
        for chat in [
            "I made it! ```toml\nname = \"x\"\n```",
            "Done, the skill is ready.",
            "Sorry, cannot: battery.",
            "",
            // One of a skill's two files only.
            "```toml skill.toml\nname = \"x\"\n```",
            "```python scripts/main.py\nprint(1)\n```",
            // One of them twice.
            "```toml\na = 1\n```\n```toml\nb = 2\n```\n```md\n---\n```",
            // Cut off: SKILL.md never closed.
            "```toml\nname = \"x\"\n```\n```md\n---\nname: x\n",
            // Past the bound.
            big.as_str(),
        ] {
            assert_eq!(
                make_answer(&made.path, Some((chat, false))),
                None,
                "{chat:?}"
            );
        }
        assert_eq!(make_answer(&made.path, None), None);
    }

    /// Heron on #7441: a chat answer Harness says it cut short is never
    /// taken as a skill, though its two files' blocks are whole (a
    /// `scripts/…` block cut off after them): `no_answer`. A `cannot:` line
    /// that ended before the cut is still taken.
    #[test]
    fn a_gateway_make_never_takes_a_chat_skill_harness_cut_short() {
        let work = tempfile::tempdir().unwrap();
        let made = put(work.path(), "R").unwrap();
        let cut = "```toml skill.toml\nname = \"x\"\n```\n\
            ```markdown SKILL.md\n---\nname: x\n---\nSays.\n```\n\
            ```python scripts/main.py\nprint(\"ne";
        assert_eq!(make_answer(&made.path, Some((cut, true))), None);
        assert_eq!(make_answer(&made.path, Some((CHAT_SKILL, true))), None);
        assert_eq!(
            make_answer(
                &made.path,
                Some(("cannot: no battery here.\nMore wo", true))
            ),
            Some(("cannot: no battery here.".to_owned(), AnswerFrom::Chat))
        );
    }

    /// The adversarial read on #7441: a `cannot:` line Harness cut short
    /// (no line break after it in what it showed) is never taken: its
    /// reason may end mid-word. Whole, it still is.
    #[test]
    fn a_gateway_make_never_takes_a_cannot_line_harness_cut_short() {
        let work = tempfile::tempdir().unwrap();
        let made = put(work.path(), "R").unwrap();
        for chat in ["cannot: a skill can't read your pho", "  cannot: no batt  "] {
            assert_eq!(
                make_answer(&made.path, Some((chat, true))),
                None,
                "{chat:?}"
            );
            assert!(
                make_answer(&made.path, Some((chat, false))).is_some(),
                "{chat:?}"
            );
        }
    }

    /// The adversarial read on #7441: the rules' own example skill given
    /// back in chat (`example-only`, or its `skill.toml` as shown) is never
    /// taken as the make's answer.
    #[test]
    fn a_gateway_make_never_takes_the_rules_example_given_back_in_chat() {
        let work = tempfile::tempdir().unwrap();
        let made = put(work.path(), "R").unwrap();
        let card = "```SKILL.md\n---\nname: glow-back\ndescription: x\n---\n```";
        let example = format!("```skill.toml\n{EXAMPLE_TOML}```\n\n{card}");
        let renamed = CHAT_SKILL.replace("name: head-pat-next-event", "name: example-only");
        assert_ne!(renamed, CHAT_SKILL);
        for chat in [example.as_str(), renamed.as_str()] {
            assert_eq!(
                make_answer(&made.path, Some((chat, false))),
                None,
                "{chat:?}"
            );
        }
        // The same form with its own `skill.toml` and name is taken.
        let own = example.replace("cyan", "amber");
        assert!(make_answer(&made.path, Some((own.as_str(), false))).is_some());
    }

    /// The adversarial read on #7441: a bare fence of another mark, or a
    /// shorter one, inside a block is a line of its body, never its end,
    /// as the voice reads it.
    #[test]
    fn a_bare_fence_closes_only_a_block_of_its_mark_and_length() {
        let work = tempfile::tempdir().unwrap();
        let made = put(work.path(), "R").unwrap();
        // Closed by its own `~~~~`: the ``` and ~~~ inside are its body.
        let whole = "~~~~toml\na = 1\n```\n~~~\nb = 2\n~~~~\n```md\n---\nname: x\n---\n```";
        assert_eq!(skill_blocks(whole).len(), 2);
        assert!(make_answer(&made.path, Some((whole, false))).is_some());
        // A bare ``` inside a `~~~~` block is its body, and the bare `~~~~`
        // ends it: the `md` block after it is the skill's `SKILL.md`.
        let mixed = "~~~~toml\na = 1\n```\n~~~~\n```md\n---\nname: x\n---\n```";
        assert!(make_answer(&made.path, Some((mixed, false))).is_some());
        // Never closed: a ``` is not its end, so all that follows is its
        // body and no `SKILL.md` was written.
        let open = "~~~~toml\na = 1\n```\n```\n---\nname: x\n---\n```";
        assert_eq!(make_answer(&made.path, Some((open, false))), None);
    }

    /// `answer.md` wins over whatever the agent said in chat.
    #[test]
    fn a_gateway_make_with_an_answer_file_takes_the_file_over_its_chat() {
        let work = tempfile::tempdir().unwrap();
        let made = put(work.path(), "R").unwrap();
        std::fs::write(made.path.join(ANSWER_FILE), "FILE").unwrap();
        for chat in [
            Some(("cannot: chat says no", false)),
            Some((CHAT_SKILL, false)),
            Some((CHAT_SKILL, true)),
            None,
        ] {
            assert_eq!(
                make_answer(&made.path, chat),
                Some(("FILE".to_owned(), AnswerFrom::File)),
                "{chat:?}"
            );
        }
    }

    /// Stand-ins for lamp-update's wrapper and namespace script, with the
    /// lines given.
    fn namespace(dir: &Path, wrapper: &str, ns: &str) -> (PathBuf, PathBuf) {
        let (w, n) = (dir.join("hermes"), dir.join("hermes-ns"));
        std::fs::write(&w, wrapper).unwrap();
        std::fs::write(&n, ns).unwrap();
        (w, n)
    }

    /// The make's run is the lamp's Hermes exactly as a skill's run is
    /// started (row-6443's path), through its wrapper: held, with
    /// `LAMP_NS_NET=run`; never a token or the admin socket; started in its
    /// make's folder with its own home. Held or not, the wrapper and the
    /// namespace script must carry the make-home lines, lamp-update's own
    /// (so never Hermes's own program); held, the network lines too.
    #[test]
    fn a_make_is_started_through_the_marked_namespace_held_or_not() {
        let dir = tempfile::tempdir().unwrap();
        let lamp = MakeHandOvers::lamp(
            Path::new("/var/lib/lampos-tasks"),
            Default::default(),
            Default::default(),
        );
        assert_eq!(
            lamp.work,
            Path::new("/var/lib/lampos-tasks/harnesses/lamp-tasks/lamp")
        );
        let work = dir.path().join("work");
        std::fs::create_dir(&work).unwrap();
        let script = format!(
            "pwd > {seen}/cwd; env > {seen}/env; printf '%s' \"$1\" > {seen}/words; \
             ls -A \"$HOME\" > {seen}/home; cp {RULES_FILE} {seen}/rules; \
             printf 'ANSWER' > {ANSWER_FILE}",
            seen = dir.path().display()
        );
        let make = |held: bool, wrapper: &str, ns: &str| {
            let (w, n) = namespace(dir.path(), wrapper, ns);
            let mut runner = lamp_runner(dir.path(), held);
            runner.hermes.program = PathBuf::from("/bin/sh");
            runner.hermes.args = vec!["-c".into(), script.clone(), "sh".into()];
            runner.hermes.folder = work.clone();
            runner.make_work = work.clone();
            runner.hermes.tick = Duration::from_millis(10);
            if held {
                runner.hermes.held_by = Some(w.clone());
            }
            runner.wrapper = w;
            runner.ns = n;
            let _ = std::fs::remove_file(dir.path().join("env"));
            answer(&line("RULES", 5_000), true, &work, &runner, &mut || {})
        };
        let net = format!("{}{}", WRAPPER_NET_LINES[0], WRAPPER_NET_LINES[1]);
        let marked =
            format!("#!/bin/sh\n{WRAPPER_HOME_LINE}unset T\n{net}--pid hermes-ns \"$@\"\n");
        // Not held: Hermes's own program, or a wrapper or script without
        // the make-home line, starts nothing.
        for (wrapper, ns) in [
            ("#!/usr/bin/env python3\n# hermes\n", NS_HOME_LINE),
            (net.as_str(), NS_HOME_LINE),
            (marked.as_str(), "#!/bin/sh\n"),
        ] {
            let refused = make(false, wrapper, ns);
            assert_eq!(refused["error"], "no_namespace", "{refused}");
            assert!(!dir.path().join("env").exists(), "nothing started");
        }
        // A work folder other than the one the wrapper takes a make's
        // home in: the make would run in Hermes's lasting home.
        let (w, n) = namespace(dir.path(), &marked, NS_HOME_LINE);
        let mut elsewhere = lamp_runner(dir.path(), false);
        elsewhere.hermes.folder = work.clone();
        (elsewhere.wrapper, elsewhere.ns) = (w, n);
        assert_eq!(elsewhere.ready(), Err(NO_NAMESPACE.to_owned()));
        assert_eq!(
            lamp_runner(Path::new("/var/lib/lampos-tasks"), false)
                .hermes
                .folder,
            Path::new(MAKE_WORK)
        );
        assert!(WRAPPER_HOME_LINE.contains(&format!(";; {MAKE_WORK}/make-*/home)")));
        // Held: the network lines too.
        let unheld = format!("#!/bin/sh\n{WRAPPER_HOME_LINE}unset T\n");
        let refused = make(true, &unheld, NS_HOME_LINE);
        assert_eq!(refused["error"], "not_held_off", "{refused}");
        assert!(!dir.path().join("env").exists());
        for held in [false, true] {
            let answered = make(held, &marked, NS_HOME_LINE);
            assert_eq!(answered, json!({"ok": true, "answer": "ANSWER"}), "{held}");
            let env = std::fs::read_to_string(dir.path().join("env")).unwrap();
            assert_eq!(env.contains("LAMP_NS_NET=run"), held, "{env}");
            assert!(!env.contains(TOKEN_ENV), "{env}");
            assert!(!env.contains(ADMIN_SOCKET), "{env}");
            assert!(!env.contains("CARGO"), "{env}");
            let cwd = std::fs::read_to_string(dir.path().join("cwd")).unwrap();
            let cwd = Path::new(cwd.trim());
            assert!(
                cwd.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(FOLDER_HEAD),
                "{cwd:?}"
            );
            // pwd names the folder through /private on macOS; the home is
            // the same folder's.
            let home = work
                .join(cwd.file_name().unwrap())
                .join(HOME_FOLDER)
                .display()
                .to_string();
            let lines: Vec<&str> = env.lines().collect();
            assert!(lines.contains(&format!("HOME={home}").as_str()), "{env}");
            assert!(lines.contains(&format!("HERMES_HOME={home}/.hermes").as_str()));
            assert!(lines.contains(&format!("LAMP_NS_HOME={home}").as_str()));
            assert_eq!(
                std::fs::read_to_string(dir.path().join("home")).unwrap(),
                ".hermes\n"
            );
            let words = std::fs::read_to_string(dir.path().join("words")).unwrap();
            assert!(words.contains(ANSWER_FILE) && words.contains(RULES_FILE));
            assert_eq!(
                std::fs::read_to_string(dir.path().join("rules")).unwrap(),
                "RULES"
            );
            assert_eq!(std::fs::read_dir(&work).unwrap().count(), 0, "removed");
        }
        // The lines are lamp-update's own, word for word.
        let source = include_str!("../../update/src/hermes_install.rs");
        for (name, line) in [
            ("WRAPPER_HOME_LINE", WRAPPER_HOME_LINE),
            ("NS_HOME_LINE", NS_HOME_LINE),
        ] {
            assert!(
                source.contains(&format!("pub const {name}: &str = {line:?};")),
                "{name}"
            );
        }
        assert!(source.contains(&format!("pub const NS: &str = {NS:?};")));
        assert!(source.contains(&format!("pub const HOME_ENV: &str = {HOME_ENV:?};")));
        assert!(
            WRAPPER_HOME_LINE
                .contains("/var/lib/lampos-tasks/harnesses/lamp-tasks/lamp/make-*/home")
        );
        // The namespace shows control.sock alone and never names admin.sock.
        let tasks_edits = source
            .split("const TASKS_EDITS")
            .nth(1)
            .and_then(|rest| rest.split("pub fn ns_text").next())
            .unwrap();
        let script: String = tasks_edits
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect();
        assert!(script.contains("L=/run/lampos-control/control.sock"));
        assert!(!script.contains("admin.sock"));
        assert!(script.contains("@NONE"));
    }

    /// Hermes 0.22.2 writes under `installs` at every start
    /// (`hermes_cli/runtime_state.py`): `runtime_lock` opens
    /// `<key>/.install.lock` `O_CREAT|O_RDWR`, and `lease_directory`
    /// creates and flocks a lease in `<key>/environments/<gen>/.leases`;
    /// its PM runtime (`pm/runtime.py` `prepare_runtime`) opens
    /// `<key>/pm-runtime/.prepare.lock` `a+b` and leases
    /// `<key>/pm-runtime/generations/<gen>/.leases`.
    /// The make's namespace binds exactly those back read-write, from the
    /// lasting home before it is hidden, onto the shown `installs` before
    /// its read-only remount; no lock at all ends it before Hermes runs.
    #[test]
    fn a_make_may_write_hermess_install_lock_and_leases_only() {
        let shown = NS_HOME_LINE.find("mount -n --move \"$X/i\"").unwrap();
        let hidden = NS_HOME_LINE.find("none \"$H/.hermes\"").unwrap();
        for bind in [
            "mount -n -c --bind /proc/self/fd/9 \"$X/i/${p#\"$H/.hermes/installs/\"}\"",
            "mount -n -c --bind /proc/self/fd/9 \"$X/i/${p#\"$H/.hermes/installs/\"}/.prepare.lock\"",
            "mount -n -c --bind /proc/self/fd/9 \"$X/i/${g#\"$H/.hermes/installs/\"}/.leases\"",
        ] {
            let at = NS_HOME_LINE.find(bind).expect(bind);
            assert!(at < hidden && at < shown, "{bind}");
        }
        assert!(NS_HOME_LINE.contains("\"$H/.hermes/installs\"/*/.install.lock"));
        assert!(NS_HOME_LINE.contains(
            "\"$H/.hermes/installs\"/*/environments/*/.lease-managed \"$H/.hermes/installs\"/*/pm-runtime/generations/*/.lease-managed"
        ));
        assert!(NS_HOME_LINE.contains("\"$H/.hermes/installs\"/*/pm-runtime; do"));
        assert!(NS_HOME_LINE.contains(
            "[ -n \"$k\" ] || fail \"no Hermes install lock: the lasting Hermes has not run\""
        ));
        // Each of those three is opened first and bound through that
        // descriptor (#7051), so a link swapped in after the checks cannot
        // redirect the bind: the path checked again, before the bind, to
        // be no link and the very file the descriptor holds; the target
        // checked after it to be no link with the source's device and
        // inode (#7011 P3), one that differs unmounted before the make
        // ends; the descriptor closed on every way out. Both of lamp-update's
        // make lines alike: the namespace script's, and the one run path's.
        for (lines, s) in [(NS_HOME_LINE, "X"), (ONE_PATH_NS_MAKE_LINES, "Y")] {
            for (source, target, refusal) in [
                (
                    "\"$p\"",
                    format!("\"${s}/i/${{p#\"$H/.hermes/installs/\"}}\""),
                    "Hermes install lock not shown",
                ),
                (
                    "\"$p/.prepare.lock\"",
                    format!("\"${s}/i/${{p#\"$H/.hermes/installs/\"}}/.prepare.lock\""),
                    "Hermes runtime lock not shown",
                ),
                (
                    "\"$g/.leases\"",
                    format!("\"${s}/i/${{g#\"$H/.hermes/installs/\"}}/.leases\""),
                    "Hermes leases not shown",
                ),
            ] {
                let open = format!("command exec 9<{source} || fail \"{refusal}\"\n");
                let bind = format!(
                    "    s=$(stat -L -c %d:%i /proc/self/fd/9) && [ ! -L {source} ] && [ \"$(stat -c %d:%i {source})\" = \"$s\" ] && mount -n -c --bind /proc/self/fd/9 {target} || {{ exec 9<&-; fail \"{refusal}\"; }}\n"
                );
                let again = format!(
                    "    [ ! -L {target} ] && [ \"$(stat -c %d:%i {target})\" = \"$s\" ] || {{ exec 9<&-; umount -n {target}; fail \"{refusal}\"; }}\n    exec 9<&-\n"
                );
                let steps = format!("{open}{bind}{again}");
                assert_eq!(lines.matches(&steps).count(), 1, "{steps}");
                assert!(!lines.contains(&format!("--bind {source}")), "{source}");
            }
            assert_eq!(lines.matches("command exec 9<").count(), 3);
            assert_eq!(lines.matches("exec 9<&-").count(), 3 * 3);
            assert_eq!(lines.matches("s=$(stat ").count(), 3);
        }
        // Nothing else of `installs`, nor `tools`, is bound back: by path
        // only the four staging binds, through a descriptor only those
        // three (#7051).
        assert_eq!(NS_HOME_LINE.matches("mount -n --bind").count(), 4);
        assert_eq!(NS_HOME_LINE.matches("mount -n -c --bind").count(), 3);
        assert_eq!(NS_HOME_LINE.matches("--bind").count(), 7);
        for kept in [
            "facts.json",
            "publication.json",
            "selected.json",
            "venv",
            "tools\"/",
        ] {
            assert!(!NS_HOME_LINE.contains(kept), "{kept}");
        }
        assert!(NS_HOME_LINE.contains("ro \"$H/.hermes/installs\""));
        assert!(NS_HOME_LINE.contains("ro \"$H/.hermes/tools\""));
    }

    /// The make's lines run after `/run` is read-only, where util-linux
    /// cannot write its utab: a `--move` without `-n` failed there on a
    /// lamp ("Hermes checkout not shown"). Every mount the lines make is
    /// `mount -n` (the `ro` helper's remounts passed there as they are).
    #[test]
    fn every_mount_a_makes_lines_make_skips_the_utab() {
        assert_eq!(NS_HOME_LINE.matches("mount -n --move").count(), 4);
        assert_eq!(
            NS_HOME_LINE.matches("mount ").count(),
            NS_HOME_LINE.matches("mount -n ").count()
        );
    }

    /// A refused make is journalled with its code and its reason, in one
    /// line; an answered one with no words of it.
    #[test]
    fn a_refused_make_is_journalled_with_its_reason() {
        assert_eq!(
            journal_line(&refusal(
                "run_failed",
                "the lamp's Hermes ended with status 1"
            )),
            "[lamp-tasks] skill make: refused run_failed: the lamp's Hermes ended with status 1"
        );
        assert_eq!(
            journal_line(&refusal("no_answer", "a\nb")),
            "[lamp-tasks] skill make: refused no_answer: a b"
        );
        assert_eq!(
            journal_line(&json!({"ok": true, "answer": "THE ANSWER"})),
            "[lamp-tasks] skill make: the run ended with an answer"
        );
    }

    /// The first builder's 19:14 ruling: by default a make's run keeps the
    /// network, still in Hermes's namespace, so `control.sock` alone and
    /// never `admin.sock`; `LAMP_MAKE_AGENT_NET=held` exactly gives the
    /// strict held-off run, `LAMP_NS_NET=run`. Both check the wrapper's
    /// mark.
    #[test]
    fn the_default_keeps_the_network_and_held_holds_it_off() {
        for value in [None, Some(""), Some("HELD"), Some("on"), Some("run")] {
            assert!(!held(value), "{value:?}");
        }
        assert!(held(Some("held")));
        let home = Path::new("/var/lib/lampos-tasks");
        let kept = lamp_runner(home, false);
        assert_eq!(
            kept.hermes.program,
            Path::new(HERMES),
            "through its namespace"
        );
        assert_eq!(kept.wrapper, Path::new(HERMES));
        assert_eq!(kept.ns, Path::new(NS));
        assert!(
            kept.hermes.env.iter().all(|(name, _)| name != NET_ENV),
            "{:?}",
            kept.hermes.env
        );
        assert!(
            kept.hermes
                .env
                .iter()
                .all(|(name, value)| !value.contains("admin.sock") && name != TOKEN_ENV)
        );
        let strict = lamp_runner(home, true);
        assert_eq!(strict.hermes.program, Path::new(HERMES));
        assert!(
            strict
                .hermes
                .env
                .contains(&(NET_ENV.to_owned(), NET_RUN.to_owned()))
        );
        assert_eq!(strict.hermes.held_by.as_deref(), Some(Path::new(HERMES)));
        // Either way the namespace shows control.sock alone, never the
        // admin socket: its bind is the file, not the folder, and not
        // only with LAMP_NS_NET=run.
        let source = include_str!("../../update/src/hermes_install.rs");
        assert!(source.contains(
            "if [ -S \\\"$L\\\" ]; then touch \\\"$S/l\\\" && mount --bind \\\"$L\\\" \\\"$S/l\\\""
        ));
        // The kept run reaches the model at lamp-cloud's forwarder, the one
        // model.sock pipes to.
        assert!(source.contains("pub const RELAY_LISTEN: &str = \"127.0.0.1:7741\";"));
    }

    /// Make folders an earlier lamp-tasks left are swept as the socket is
    /// started; nothing else in the work folder is.
    #[test]
    fn old_make_folders_are_swept_at_the_start() {
        let dir = tempfile::Builder::new()
            .prefix("tms-")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let work = dir.path().join("work");
        std::fs::create_dir_all(work.join("make-task-1/home/.hermes")).unwrap();
        std::fs::write(work.join("make-task-1/rules.md"), "R").unwrap();
        std::fs::create_dir(work.join("other")).unwrap();
        std::fs::write(work.join("make-notes.md"), "kept").unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let handle = MakeHandOvers {
            socket: dir.path().join("skill-make.sock"),
            work: work.clone(),
            runner: Arc::new(writing(None)),
            one_path: None,
            caller: |_| true,
            asks: None,
            tries: None,
        }
        .start(true, stop.clone())
        .unwrap()
        .unwrap();
        assert!(!work.join("make-task-1").exists());
        assert!(work.join("other").exists() && work.join("make-notes.md").exists());
        stop.store(true, Ordering::Release);
        handle.join().unwrap();
    }

    #[test]
    fn only_lamp_voice_as_root_may_hand_a_make_over() {
        use lamp_ipc::caller::Caller;
        let caller = |uid, unit: Option<&str>| Caller {
            pid: 10,
            uid,
            unit: unit.map(str::to_owned),
        };
        assert!(caller_may(Some(&caller(0, Some(CALLER_UNIT)))));
        assert!(!caller_may(Some(&caller(994, Some(CALLER_UNIT)))));
        assert!(!caller_may(Some(&caller(0, Some("lamp-skills.service")))));
        assert!(!caller_may(Some(&caller(0, None))));
        assert!(!caller_may(None));
    }

    #[test]
    fn the_socket_says_started_then_the_answer_and_one_make_at_a_time() {
        let dir = tempfile::Builder::new()
            .prefix("tmk-")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let socket = dir.path().join("skill-make.sock");
        let work = dir.path().join("work");
        std::fs::create_dir(&work).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let handle = MakeHandOvers {
            socket: socket.clone(),
            work,
            runner: Arc::new(writing(Some("A"))),
            one_path: None,
            caller: |_| true,
            asks: None,
            tries: None,
        }
        .start(true, stop.clone())
        .unwrap()
        .expect("on binds");
        let mut stream = UnixStream::connect(&socket).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        stream.write_all(&line("R", 2_000)).unwrap();
        stream.write_all(b"\n").unwrap();
        let mut lines = BufReader::new(stream).lines();
        let first: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
        assert_eq!(first, json!({"ok": true, "started": true}));
        let second: Value = serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap();
        assert_eq!(second, json!({"ok": true, "answer": "A"}));
        stop.store(true, Ordering::Release);
        handle.join().unwrap();
        assert!(!socket.exists());
        let off = MakeHandOvers {
            socket: socket.clone(),
            work: dir.path().into(),
            runner: Arc::new(writing(None)),
            one_path: None,
            caller: |_| true,
            asks: None,
            tries: None,
        }
        .start(false, Arc::default())
        .unwrap();
        assert!(off.is_none() && !socket.exists());
    }

    /// #6643 with #6685's one run path (`LAMP_SKILL_DEFAULT_AGENT` on in
    /// lamp-update): Hermes's wrapper is the one run path's (lamp-update's
    /// pinned bytes, with Hermes's make-home line in its data) and runs the
    /// one agent namespace, which carries Hermes's make lines: a make is
    /// ready, Hermes's own `hermes-ns` gone. Without either line, or with
    /// another agent's wrapper, it is refused [`NO_NAMESPACE`]; held, it
    /// is refused [`NOT_HELD_OFF_ONE_PATH`], as the one run path holds only
    /// a run started in its own folder; so is the make's net runner, with
    /// no egress socket made (a task's run granted `net` goes by the one
    /// run path, [`NetRunner`]). The lines are lamp-update's own.
    #[test]
    fn with_the_one_run_path_a_make_is_ready() {
        let dir = tempfile::tempdir().unwrap();
        let hermes = include_str!("../../update/src/agent_ns/wrapper/one-path-hermes.sh");
        let claude = include_str!("../../update/src/agent_ns/wrapper/one-path-claude.sh");
        assert!(hermes.contains(ONE_PATH_HOME_LINE));
        let with_make = format!("#!/bin/sh\nH=${{LAMP_NS_HOME-}} \n{ONE_PATH_NS_MAKE_LINES}");
        let runner = |held: bool, wrapper: &str, agent_ns: &str| {
            let (w, n) = (dir.path().join("hermes"), dir.path().join("agent-ns"));
            std::fs::write(&w, wrapper).unwrap();
            std::fs::write(&n, agent_ns).unwrap();
            let mut runner = lamp_runner(Path::new("/var/lib/lampos-tasks"), held);
            if held {
                runner.hermes.held_by = Some(w.clone());
            }
            runner.wrapper = w;
            runner.ns = dir.path().join("hermes-ns-gone");
            runner.agent_ns = n;
            runner
        };
        assert_eq!(runner(false, hermes, &with_make).ready(), Ok(()));
        let without = hermes.replacen(ONE_PATH_HOME_LINE, "", 1);
        for (wrapper, agent_ns) in [
            (without.as_str(), with_make.as_str()),
            (hermes, "#!/bin/sh\n"),
            (claude, with_make.as_str()),
        ] {
            assert_eq!(
                runner(false, wrapper, agent_ns).ready(),
                Err(NO_NAMESPACE.to_owned())
            );
        }
        // Said as the one run path's, never blaming a switch or a round.
        assert_eq!(
            runner(true, hermes, &with_make).ready(),
            Err(NOT_HELD_OFF_ONE_PATH.to_owned())
        );
        // The make's net runner is held too, so it is refused the same way
        // there, before its egress socket is made: the one run path's
        // wrapper gives `--net` only to a run started in its own
        // `skill-<run-id>` folder. A task's run granted `net` goes by the
        // one run path instead (`NetRunner`, #7200).
        let mut net = runner(true, hermes, &with_make);
        let egress = dir.path().join("egress");
        net.egress = Some(crate::egress::Grant {
            dir: egress.clone(),
            ..crate::egress::Grant::lamp()
        });
        let folder = dir.path().join("make-a");
        std::fs::create_dir(&folder).unwrap();
        let run = make_run_with(
            &folder,
            Duration::from_secs(1),
            false,
            &CheckSwitches::default(),
        );
        assert_eq!(net.run(&run, &|| {}), Err(NOT_HELD_OFF_ONE_PATH.to_owned()));
        assert!(!egress.exists());
        assert_eq!(
            lamp_runner(Path::new("/var/lib/lampos-tasks"), false).agent_ns,
            Path::new(crate::skill_task::AGENT_NS)
        );
        let source = include_str!("../../update/src/hermes_install.rs");
        for (name, line) in [
            ("ONE_PATH_HOME_LINE", ONE_PATH_HOME_LINE),
            ("ONE_PATH_NS_MAKE_LINES", ONE_PATH_NS_MAKE_LINES),
        ] {
            assert!(
                source.contains(&format!("pub const {name}: &str = {line:?};")),
                "{name}"
            );
        }
    }

    /// #7245, V1's tools on (`MakeGateway::checked`): a gateway make's
    /// folder holds the rules and the guide whole, the check in both, as
    /// Hermes's make has them; its words run that check, from its folder
    /// and with the line's switches, and nothing else; off, as before.
    #[test]
    fn a_gateway_make_with_v1s_tools_gets_the_check() {
        let rules = format!("a\n  {CHECK_PROGRAM} pack --out x draft/A\nb\n");
        let work = tempfile::tempdir().unwrap();
        let made = put_for_gateway(work.path(), &rules, true, true).unwrap();
        let read = |name: &str| std::fs::read_to_string(made.path.join(name)).unwrap();
        assert_eq!(read(RULES_FILE), rules);
        assert_eq!(read(GUIDE_FILE), GUIDE);
        assert!(read(GUIDE_FILE).contains("lamp-skills pack --out /tmp/lamp-check draft/NAME"));
        let other = tempfile::tempdir().unwrap();
        let plain = put_for_gateway(other.path(), &rules, true, false).unwrap();
        let read = |name: &str| std::fs::read_to_string(plain.path.join(name)).unwrap();
        assert_eq!(read(RULES_FILE), "a\nb\n");
        assert_eq!(read(GUIDE_FILE), gateway_guide());

        let folder = Path::new("/w/make-1");
        let none = CheckSwitches::default();
        let words = gateway_checked_prompt(folder, &none);
        assert!(
            words.contains(&format!(
                "run from this folder as cd /w/make-1 && {CHECK_PROGRAM} pack --out {CHECK_OUT} draft/NAME,"
            )),
            "{words}"
        );
        assert!(words.contains("run no other command"), "{words}");
        assert!(!words.contains("run no command"), "{words}");
        assert_eq!(
            words
                .replace("cd /w/make-1 && ", "")
                .replace(&format!("{GATEWAY_CAN}\n\n"), "")
                .replace(&format!("\n\n{}", gateway_form()), ""),
            guided_prompt(folder),
            "Hermes's words, the check run from the folder"
        );
        let both = CheckSwitches {
            agent_services: Switch::On,
            skill_writes: Switch::On,
        };
        assert!(gateway_checked_prompt(folder, &both).contains(&format!(
            "cd /w/make-1 && LAMP_AGENT_SERVICES=on LAMP_SKILL_WRITES=on {CHECK_PROGRAM} pack"
        )));
        let run = gateway_run_with(folder, Duration::from_secs(5), true, true, &both);
        assert_eq!(run.prompt, gateway_checked_prompt(folder, &both));
        assert!(run.env.is_empty() && run.script.as_os_str().is_empty());
        // Off, or with no guide: today's words.
        let off = gateway_run_with(folder, Duration::from_secs(5), true, false, &both);
        assert_eq!(off.prompt, gateway_prompt(folder, true));
        let unguided = gateway_run_with(folder, Duration::from_secs(5), false, true, &none);
        assert_eq!(unguided.prompt, gateway_prompt(folder, false));
    }

    /// #7245: a gateway agent's make never gets the guide's check, which
    /// its exec allow-list can't run: the guide without its section 3, the
    /// rules without a line naming the check, and words that run nothing;
    /// the line's `one_path` is a bool, false when not given.
    #[test]
    fn a_gateway_make_is_given_no_check_and_one_path_is_read() {
        let guide = gateway_guide();
        assert!(!guide.contains(CHECK_PROGRAM) && !guide.contains("lamp-skills"));
        assert!(!guide.contains("## 4. ") && guide.contains("## 3. Answer"));
        assert!(guide.starts_with(&GUIDE[..GUIDE.find("## 3. ").unwrap()]));
        assert!(GUIDE.contains(CHECK_PROGRAM), "the guide itself keeps it");
        let rules = format!("a\n  {CHECK_PROGRAM} pack --out x draft/A\nb\n");
        assert_eq!(without_check(&rules), "a\nb\n");
        assert_eq!(without_check("a\nb"), "a\nb");
        let folder = Path::new("/w/make-1");
        for guided in [false, true] {
            let words = gateway_prompt(folder, guided);
            assert!(
                !words.contains(CHECK_PROGRAM) && !words.contains(" pack "),
                "{words}"
            );
            let run = gateway_run(folder, Duration::from_secs(5), guided);
            assert_eq!(run.prompt, words);
            assert!(run.env.is_empty() && run.script.as_os_str().is_empty());
            assert_eq!(run.start_in.as_deref(), Some(folder));
            assert_eq!(run.run, "1");
        }
        assert_eq!(
            gateway_prompt(folder, false),
            format!("{GATEWAY_CAN}\n\n{}\n\n{}", prompt(folder), gateway_form())
        );
        // lamp-c0a4, 18 Nov: the words say first that the skill can be
        // made, and that cannot is only for a limit rules.md names.
        for guided in [false, true] {
            let words = gateway_prompt(folder, guided);
            assert!(words.starts_with("You are making a skill for this lamp, and it can be made."));
            assert!(words.contains("if unsure, make it"), "{words}");
            assert!(words.contains("no lookup"), "{words}");
        }
        let parsed = |extra: &str| {
            MakeLine::parse(
                format!(r#"{{"op":"skill_make","rules":"R","within_ms":2000{extra}}}"#).as_bytes(),
            )
            .map(|make| make.one_path)
        };
        assert_eq!(parsed(""), Ok(false));
        assert_eq!(parsed(r#","one_path":false"#), Ok(false));
        assert_eq!(parsed(r#","one_path":true"#), Ok(true));
        assert!(parsed(r#","one_path":"yes""#).is_err());
    }

    /// #7245: a gateway make's words carry the skill.toml form inline,
    /// guided or not: the order, the one rule on asks and the minimal
    /// skill.toml, after the words that still point to the rules for the
    /// rest and the request; within Harness's task bound for the lamp's
    /// make folder. Without the form inline (the words alone, as before),
    /// this fails.
    #[test]
    fn a_gateway_makes_words_carry_the_skill_toml_form_inline() {
        let folder = Path::new(MAKE_WORK).join(FOLDER_HEAD);
        let form = gateway_form();
        for guided in [false, true] {
            let words = gateway_prompt(&folder, guided);
            assert!(words.ends_with(&format!("\n\n{form}")), "{words}");
            for part in [FORM_ORDER, FORM_RULE, FORM_TOML] {
                assert!(words.contains(part), "{guided}: {part}");
            }
            assert!(words.contains("```skill.toml\nasks = [\"desk glow on\"]\n"));
            assert!(words.contains("then [permissions]"), "{words}");
            assert!(words.contains("Never write asks under [permissions]"));
            assert!(crate::harness::prompt_fits(&words), "{}", words.len());
            assert_eq!(
                gateway_run(&folder, Duration::from_secs(5), guided).prompt,
                words
            );
        }
    }

    /// #7245: the inline form is a short block, at most
    /// [`MOST_FORM_BYTES`], its skill.toml 12 lines at most with `asks` on
    /// its first line and before any table.
    #[test]
    fn the_inline_form_stays_within_its_cap() {
        let form = gateway_form();
        assert!(form.len() <= MOST_FORM_BYTES, "{}", form.len());
        assert!(FORM_TOML.lines().count() <= 12);
        assert!(FORM_TOML.starts_with("asks = ["));
        let first_table = FORM_TOML.find("\n[").unwrap();
        assert!(
            ["kind = ", "version = "]
                .iter()
                .all(|key| FORM_TOML.find(key).is_some_and(|at| at < first_table))
        );
        assert!(FORM_TOML.find("[permissions]") < FORM_TOML.find("[[do]]"));
    }

    /// #7245: the gateway words still name the rules, for the rest of the
    /// form and the person's request, and the guide when guided; the
    /// request itself is never inlined.
    #[test]
    fn a_gateway_makes_words_still_name_the_rules() {
        let folder = Path::new("/w/make-1");
        for guided in [false, true] {
            let words = gateway_prompt(folder, guided);
            // After the first words, which name every file the folder may hold.
            let words = words.strip_prefix(GATEWAY_CAN).unwrap();
            let before = &words[..words.find(FORM_ORDER).unwrap()];
            assert!(before.contains(RULES_FILE), "{before}");
            assert_eq!(before.contains(GUIDE_FILE), guided, "{before}");
            assert!(gateway_form().contains("rules.md has the rest"));
        }
    }

    /// #7245: Hermes's make's words (the harness make's, [`make_run_with`])
    /// are as before: no inline form, guided or not.
    #[test]
    fn a_hermes_makes_words_are_unchanged() {
        let folder = Path::new("/w/make-1");
        for guided in [false, true] {
            let words = make_run_with(
                folder,
                Duration::from_secs(5),
                guided,
                &CheckSwitches::default(),
            )
            .prompt;
            let before = if guided {
                guided_prompt(folder)
            } else {
                prompt(folder)
            };
            assert_eq!(words, before);
            assert!(!words.contains(FORM_TOML) && !words.contains(FORM_RULE));
        }
        assert!(prompt(folder).ends_with("install nothing and run nothing else."));
        assert!(guided_prompt(folder).ends_with("install nothing and run no other command."));
    }

    /// #7245: the inline skill.toml given back in chat is refused as the
    /// rules' example is; with another ask it is taken.
    #[test]
    fn a_gateway_make_never_takes_the_inline_form_given_back_in_chat() {
        let work = tempfile::tempdir().unwrap();
        let made = put(work.path(), "R").unwrap();
        let card = "```SKILL.md\n---\nname: desk-glow\ndescription: x\n---\n```";
        let given = format!("```skill.toml\n{FORM_TOML}```\n\n{card}");
        assert_eq!(make_answer(&made.path, Some((given.as_str(), false))), None);
        let own = given.replace("desk glow on", "reading glow please");
        assert!(make_answer(&made.path, Some((own.as_str(), false))).is_some());
    }

    /// #7245: a gateway agent's make folder, guided or not, holds the
    /// format's whole reference and the example skills beside the rules,
    /// each file 0600 and each folder 0700; its words name them within
    /// Harness's task bound. Hermes's make folder holds neither.
    /// #7245: the checked words stay within Harness's 2,000-byte task cap
    /// with a real make folder's path and every switch on.
    #[test]
    fn the_checked_prompt_fits_harness_cap() {
        let folder = Path::new("/var/lib/lampos-tasks/harnesses/lamp-tasks/lamp/make-4294967295");
        let switches = CheckSwitches {
            agent_services: Switch::On,
            skill_writes: Switch::On,
        };
        let words = gateway_checked_prompt(folder, &switches);
        // With lamp-tasks' LAMP_MADE_SKILL_ASKS on (the product's unit),
        // its prefix too (Twelve's P3 on #7737).
        let asks = format!("{ASKS_ENVIRONMENT}=on ");
        assert!(words.len() + asks.len() <= 2_000, "{} bytes", words.len());
        assert!(words.contains("Use only this make's folder below"));
        assert!(words.contains("run nothing but the check it names"));
    }

    #[test]
    fn a_gateway_make_folder_holds_the_format_and_the_examples() {
        use std::os::unix::fs::PermissionsExt;
        let work = tempfile::tempdir().unwrap();
        for guide in [false, true] {
            let made = put_for_gateway(work.path(), "R", guide, false).unwrap();
            let format = made.path.join(FORMAT_FILE);
            assert_eq!(std::fs::read_to_string(&format).unwrap(), FORMAT);
            assert!(FORMAT.contains("show = \"NAME\""));
            assert_eq!(
                std::fs::read_to_string(made.path.join(SDK_FILE)).unwrap(),
                SDK
            );
            let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&format), 0o600);
            for (path, text) in MAKE_EXAMPLES {
                let file = made.path.join(EXAMPLES_FOLDER).join(path);
                assert_eq!(std::fs::read_to_string(&file).unwrap(), text, "{path}");
                assert_eq!(mode(&file), 0o600, "{path}");
                assert_eq!(mode(file.parent().unwrap()), 0o700, "{path}");
            }
            assert_eq!(made.path.join(GUIDE_FILE).exists(), guide);
            let words = gateway_prompt(&made.path, guide);
            for named in [FORMAT_FILE, "examples/", GUIDE_FILE, RULES_FILE] {
                assert!(words.contains(named), "{named}: {words}");
            }
        }
        let hermes = put_with(work.path(), "R", true).unwrap();
        assert!(!hermes.path.join(FORMAT_FILE).exists());
        assert!(!hermes.path.join(EXAMPLES_FOLDER).exists());
    }

    /// #7245: every gateway make's words, unguided and guided, for the
    /// longest make folder a run has (its whole run id), fit Harness's task
    /// bound with room to spare: within 2,000 bytes, the bound before it was
    /// raised to 3,000, so the margin stays.
    #[test]
    fn every_gateway_makes_words_fit_the_task_bound_with_room() {
        let folder = Path::new(MAKE_WORK).join(format!("{FOLDER_HEAD}{}", "f".repeat(64)));
        for guide in [false, true] {
            let words = gateway_prompt(&folder, guide);
            assert!(
                crate::harness::prompt_fits(&words),
                "{guide}: {}",
                words.len()
            );
            assert!(words.len() <= crate::harness::MAX_PROMPT_BYTES);
            assert!(words.len() <= 2000, "{guide}: {}", words.len());
            assert_eq!(
                gateway_run(&folder, Duration::from_secs(5), guide).prompt,
                words
            );
        }
        assert!(GATEWAY_CAN.len() <= 540, "{}", GATEWAY_CAN.len());
    }

    /// #6811 (the first builder, 18 Nov 17:23): a make marked "may run on"
    /// may be given up to ten minutes, as every other make may since #7245
    /// (a gateway agent's make); the mark is only `true` or `false`, and
    /// past ten minutes either is refused.
    #[test]
    fn a_make_marked_may_run_on_is_given_up_to_ten_minutes() {
        let parsed = |within_ms: u64, extra: &str| {
            MakeLine::parse(
                format!(r#"{{"op":"skill_make","rules":"R","within_ms":{within_ms}{extra}}}"#)
                    .as_bytes(),
            )
            .map(|make| (make.may_run_on, make.within()))
        };
        let past = 300_000;
        assert_eq!(parsed(past, ""), Ok((false, Duration::from_millis(past))));
        assert_eq!(
            parsed(past, r#","may_run_on":false"#),
            Ok((false, Duration::from_millis(past)))
        );
        assert!(
            parsed(600_001, "").is_err(),
            "not marked: MOST_WITHIN at most"
        );
        assert_eq!(
            parsed(past, r#","may_run_on":true"#),
            Ok((true, Duration::from_millis(past)))
        );
        assert_eq!(
            parsed(600_000, r#","may_run_on":true"#),
            Ok((true, RUN_ON_MOST_WITHIN))
        );
        assert!(parsed(600_001, r#","may_run_on":true"#).is_err());
        assert_eq!(parsed(600_000, ""), Ok((false, MOST_WITHIN)));
        assert!(parsed(2_000, r#","may_run_on":"yes""#).is_err());
        assert_eq!(RUN_ON_MOST_WITHIN, Duration::from_secs(600));
        assert_eq!(MOST_WITHIN, Duration::from_secs(600));
    }
}
