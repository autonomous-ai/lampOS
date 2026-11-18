//! A core skill's line through the lamp's agent (`LAMP_CORE_VIA_AGENT`, a
//! pilot: `on` exactly, off unless set, in no unit; the first builder's
//! ruling of 19 Nov 05:42 on its scope and fallback). Only
//! [`PILOTS`]: Sip of water and Stretch break, the two that say a line at
//! each fire. Off, nothing here is read and both load and run byte for byte
//! as before.
//!
//! - **The middle form, for a core skill.** lamp-skills keeps the skill's
//!   trigger, `[[while]]` guards, `most_a_day`, `[active]`, `[quiet]`,
//!   `cooldown`, `rearm`, `wait`, budget, rest and stop exactly: they are
//!   `skill.toml`'s own and never copied. Only what each fire says becomes
//!   a `do.task` for the lamp's agent, the skill's own plain intent from its
//!   `SKILL.md` ([`Pilot::task`]), with a run-time brief in its
//!   `scripts/agent.py` ([`script`], the form of a voice make's
//!   `lamp_voice::make_no_cannot` brief: one plain Python string).
//! - **The variant** ([`OVERLAY`], `agent.toml` beside `skill.toml`): it
//!   stands in for `grants`, `[[do]]` and `[[present.do]]` alone
//!   ([`overlaid`]), and only while every one of its steps but the task is
//!   `skill.toml`'s own, word for word, its grants none, its task the
//!   pilot's words and its script [`script`]'s byte for byte. Anything else
//!   and the skill loads as it is, with one journal line why. A core skill
//!   carries a `do.task` only so (`crate::task::origin_refuses` is not
//!   asked: its brief is compiled into this program, not a script from
//!   elsewhere). The owner's yes, lamp-control's one-run token, the day's
//!   bounds and one run at a time hold as for every `do.task`.
//! - **The fallback, never late** ([`FALLBACK_WITHIN`], [`awaited`]): the
//!   run's say waits for the agent's line at most 10 s from the fire, on
//!   the run's own thread (never the trigger's); the line comes, it is said
//!   as exact words in the say's place ([`with_line`]), with the look, the
//!   light and the move as the say brings them. Not within 10 s, or the
//!   agent down, not reached, refused, waiting for the yes, or answering
//!   `-`, nothing, or more than one short line ([`heard`]), the skill's own
//!   say is said at once, as with the switch off. A line that comes after
//!   is dropped, never said late, and a failed run says nothing of its own.
use crate::{Call, SAY_EXPIRES_MS, SAY_PRIORITY, Words};
use std::{fs, path::Path, sync::mpsc, time::Duration};

/// The switch, in lamp-skills and its manage helper: `on` exactly.
pub const ENVIRONMENT: &str = "LAMP_CORE_VIA_AGENT";

/// The variant beside a pilot's `skill.toml`.
pub const OVERLAY: &str = "agent.toml";

/// Its task's script: the run-time brief, as a voice make's
/// (`lamp_voice::make_no_cannot::RUN`).
pub const RUN: &str = lamp_voice::make_no_cannot::RUN;

/// The longest a fire's say waits for the agent's line before the skill's
/// own is said: the first builder's 10 s (05:42).
pub const FALLBACK_WITHIN: Duration = Duration::from_secs(10);

/// Where a pilot's task is said to come from in the owner's yes.
pub const FROM: &str = "the lamp itself";

/// The most characters of the agent's line: one short line.
pub const MOST_LINE_CHARS: usize = 200;

/// The most sentences of the agent's line.
pub const MOST_LINE_SENTENCES: usize = 2;

/// A core skill whose line may go through the lamp's agent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pilot {
    /// Its folder's name.
    pub name: &'static str,
    /// What fires it, in plain words, for the brief.
    pub fire: &'static str,
    /// Its `do.task`: the skill's own plain intent, from its `SKILL.md`,
    /// never a guess at a person's words.
    pub task: &'static str,
}

/// The pilots: only these two (the first builder, 05:42). A quick
/// reaction, or a skill whose fire is only light, a fixed sound or a move,
/// never goes through the agent.
pub const PILOTS: [Pilot; 2] = [
    Pilot {
        name: "sip-of-water",
        fire: "someone has sat at the desk a while, half an hour or more",
        task: "One short, friendly line suggesting a sip of water; no lecture, nothing about health.",
    },
    Pilot {
        name: "stretch-break",
        fire: "someone has been at the desk about fifty minutes without getting up, and the lamp \
               stretches",
        task: "One short, playful line, as an invitation to take a break, as the lamp stretches \
               too; never a lecture.",
    },
];

/// The pilot named `name`, if it is one.
pub fn pilot(name: &str) -> Option<&'static Pilot> {
    PILOTS.iter().find(|pilot| pilot.name == name)
}

/// `pilot`'s `scripts/agent.py`: the brief the agent reads at each run,
/// naming what fired it and what the lamp says now, asking for that one
/// line alone and `-` when nothing fits. One plain Python string.
pub fn script(pilot: &Pilot) -> String {
    let task = pilot.task.trim_end_matches('.');
    let task = format!("{}{}", task[..1].to_lowercase(), &task[1..]);
    format!(
        "print(\"A skill that comes with the lamp, {}, has fired ({}). What the lamp says now: \
         {task}. Answer with that one line alone, the very words the lamp will say, in the \
         language the owner speaks with the lamp: nothing else, no tool, never making, changing \
         or removing a skill. If nothing fitting comes to mind, answer -.\")\n",
        pilot.name, pilot.fire
    )
}

#[cfg(test)]
thread_local! {
    /// The switch as a test of this thread sets it.
    static ON_HERE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Whether the switch is on in this process.
pub fn on() -> bool {
    #[cfg(test)]
    if let Some(on) = ON_HERE.with(std::cell::Cell::get) {
        return on;
    }
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// The switch for this test's thread alone.
#[cfg(test)]
pub fn set_on_here(on: Option<bool>) {
    ON_HERE.with(|here| here.set(on));
}

/// The pilot `name`'s `skill.toml` as `table` with its variant in `dir`
/// ([`OVERLAY`]) standing in for `grants`, `do` and `present.do`: none with
/// the switch off, for a skill not a pilot, or with no variant; why not,
/// for a variant that is not exactly the pilot's (the skill then loads as
/// it is).
pub fn overlaid(
    name: &str,
    dir: &Path,
    table: &toml::Table,
) -> Result<Option<toml::Table>, String> {
    let Some(pilot) = pilot(name).filter(|_| on()) else {
        return Ok(None);
    };
    let text = match fs::read_to_string(dir.join(OVERLAY)) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let mut overlay: toml::Table = toml::from_str(&text).map_err(|error| error.to_string())?;
    if table.get("kind").and_then(toml::Value::as_str) != Some("core") {
        return Err("only a core skill's line goes through the agent so".into());
    }
    if let Some(key) = overlay
        .keys()
        .find(|key| !matches!(key.as_str(), "grants" | "do" | "present"))
    {
        return Err(format!(
            "it gives only grants, [[do]] and [[present.do]], not {key:?}"
        ));
    }
    if overlay.get("grants") != Some(&toml::Value::Array(Vec::new())) {
        return Err("grants = [] exactly: the agent's run reaches none of the lamp's parts".into());
    }
    let script_path = dir.join(RUN);
    if !fs::symlink_metadata(&script_path).is_ok_and(|meta| meta.is_file())
        || fs::read_to_string(&script_path).ok() != Some(script(pilot))
    {
        return Err(format!(
            "{RUN} is not the lamp's own brief for {name}, byte for byte"
        ));
    }
    let mut with = table.clone();
    with.insert("grants".into(), toml::Value::Array(Vec::new()));
    let steps = overlay.remove("do").ok_or("it gives no [[do]]")?;
    with.insert("do".into(), task_beside(pilot, &steps, table.get("do"))?);
    let present_do = table
        .get("present")
        .and_then(toml::Value::as_table)
        .and_then(|present| present.get("do"));
    match (overlay.remove("present"), present_do) {
        (None, None) => {}
        (Some(toml::Value::Table(mut present)), Some(own)) => {
            let steps = present
                .remove("do")
                .ok_or("its [present] gives only [[present.do]]")?;
            if !present.is_empty() {
                return Err("its [present] gives only [[present.do]]".into());
            }
            let steps = task_beside(pilot, &steps, Some(own))?;
            if let Some(toml::Value::Table(present)) = with.get_mut("present") {
                present.insert("do".into(), steps);
            }
        }
        _ => {
            return Err(
                "[[present.do]] stands in only for skill.toml's own, and for it always".into(),
            );
        }
    }
    Ok(Some(with))
}

/// `steps` checked as the variant's: one task, the pilot's words running
/// [`RUN`], alone in its table, and every other step `own`'s, in order,
/// word for word (the fallback is the skill's own say).
fn task_beside(
    pilot: &Pilot,
    steps: &toml::Value,
    own: Option<&toml::Value>,
) -> Result<toml::Value, String> {
    let toml::Value::Array(list) = steps else {
        return Err("[[do]] is a list of steps".into());
    };
    let task = |step: &toml::Value| {
        step.as_table()
            .is_some_and(|step| step.contains_key("task"))
    };
    let tasks: Vec<&toml::Value> = list.iter().filter(|step| task(step)).collect();
    let mut wanted = toml::Table::new();
    wanted.insert("task".into(), toml::Value::String(pilot.task.into()));
    wanted.insert("run".into(), toml::Value::String(RUN.into()));
    let mut alone = toml::Table::new();
    alone.insert("task".into(), toml::Value::Table(wanted));
    if tasks != [&toml::Value::Table(alone)] {
        return Err(format!(
            "its one task is {{ task = {:?}, run = {RUN:?} }}, alone in its [[do]]",
            pilot.task
        ));
    }
    let rest: Vec<toml::Value> = list.iter().filter(|step| !task(step)).cloned().collect();
    if Some(&toml::Value::Array(rest)) != own {
        return Err("every step but the task is skill.toml's own, word for word".into());
    }
    Ok(steps.clone())
}

/// What became of a fire's run, as its say waits for it ([`awaited`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Heard {
    /// The agent's one short line, to say in the say's place.
    Line(String),
    /// Nothing to say of the agent's: why, for the journal.
    Nothing(&'static str),
}

/// Why nothing of the agent's is said, for the journal.
pub const NOTHING_TO_SAY: &str = "its agent had nothing to say (- or an empty answer)";
/// The agent's line longer than one short line.
pub const TOO_LONG: &str = "its agent's answer was more than one short line";
/// The run not handed over, refused or failed.
pub const NOT_RUN: &str = "its agent's run didn't run or didn't end well";

/// The agent's line in its told sentences (`crate::task::told` of
/// lamp-tasks' answer): one short line of at most
/// [`MOST_LINE_SENTENCES`] sentences and [`MOST_LINE_CHARS`] characters;
/// none (a lone `-`, an empty answer, a run that failed) is
/// [`NOTHING_TO_SAY`], a longer one [`TOO_LONG`]: never something wrong.
pub fn heard(told: Option<Vec<String>>) -> Heard {
    let Some(lines) = told else {
        return Heard::Nothing(NOTHING_TO_SAY);
    };
    let line = lines.join(" ");
    if lines.len() > MOST_LINE_SENTENCES
        || line.chars().count() > MOST_LINE_CHARS
        || lines.iter().any(|said| said == crate::task::MORE_LINE)
    {
        return Heard::Nothing(TOO_LONG);
    }
    if line.trim().is_empty() {
        return Heard::Nothing(NOTHING_TO_SAY);
    }
    Heard::Line(line)
}

/// What a fire says, once [`awaited`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Said {
    /// The agent's line, in time.
    Agent(String),
    /// The skill's own say: why.
    Fallback(String),
}

impl Said {
    /// The journal's words, never the agent's.
    pub fn journal(&self) -> String {
        match self {
            Self::Agent(line) => format!(
                "its line through the lamp's agent: said its agent's line ({} characters)",
                line.chars().count()
            ),
            Self::Fallback(why) => {
                format!("its line through the lamp's agent: said its own line at once: {why}")
            }
        }
    }
}

/// What the fire says: the agent's line if it comes on `from` within
/// `within` of the fire (the product's [`FALLBACK_WITHIN`]), the skill's own
/// at once otherwise: past `within`, the run gone without a word (its
/// sender dropped: down, not reached, refused), or [`Heard::Nothing`]. The
/// receiver is dropped with it, so a line after it is never said.
pub fn awaited(from: mpsc::Receiver<Heard>, within: Duration) -> Said {
    match from.recv_timeout(within) {
        Ok(Heard::Line(line)) => Said::Agent(line),
        Ok(Heard::Nothing(why)) => Said::Fallback(why.into()),
        Err(mpsc::RecvTimeoutError::Timeout) => Said::Fallback(format!(
            "its agent's line didn't come within {} s",
            within.as_secs_f32()
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => Said::Fallback(NOT_RUN.into()),
    }
}

/// Whether `call` is a fire's say, which waits for the agent's line.
pub fn is_line(call: &Call) -> bool {
    matches!(call, Call::Say(_) | Call::Words(_))
}

/// `calls` as `said` has them: with the agent's line, the say is it, as
/// exact words at the say's own priority (the look, the light and the move
/// still made as the voice begins it); with the fallback, as they are.
pub fn with_line(calls: Vec<Call>, said: &Said) -> Vec<Call> {
    let Said::Agent(line) = said else {
        return calls;
    };
    calls
        .into_iter()
        .map(|call| match call {
            Call::Say(_) => Call::Words(Words {
                words: line.clone(),
                exact: true,
                priority: SAY_PRIORITY,
                expires_ms: SAY_EXPIRES_MS,
                question: false,
                may_end_unsaid: false,
                count: None,
            }),
            Call::Words(words) => Call::Words(Words {
                words: line.clone(),
                exact: true,
                question: false,
                count: None,
                ..words
            }),
            call => call,
        })
        .collect()
}

#[cfg(test)]
mod tests;
