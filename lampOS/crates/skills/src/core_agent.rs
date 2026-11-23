//! A core skill's line through the lamp's agent (`LAMP_CORE_VIA_AGENT`:
//! `on` for every group, or a list of [`Group`]s, `pilot,nudges`; off unless
//! set, in no unit; the first builder's ruling of 19 Nov 05:42 on the
//! pilot's scope and fallback, and 06:05: quick reactions stay DeepSeek's,
//! and every other model-worded core line goes to the agent, group by
//! group). Only [`PILOTS`]; Task news, the `news` group, is lamp-voice's
//! (`lamp_voice::news_agent`). Off, nothing here is read and they load and
//! run byte for byte as before.
//!
//! - **The middle form, for a core skill.** lamp-skills keeps the skill's
//!   trigger, `[[while]]` guards, `most_a_day`, `[active]`, `[quiet]`,
//!   `cooldown`, `rearm`, `wait`, budget, rest and stop exactly: they are
//!   `skill.toml`'s own and never copied. Only what each fire says becomes
//!   a `do.task` for the lamp's agent, the skill's own plain intent from its
//!   `SKILL.md` ([`Task::task`]), with a run-time brief in its
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
//!   elsewhere). Its task is pre-approved ([`pre_approved`]): no yes is
//!   ever asked, and lamp-control mints its one-run token with no grant.
//!   The day's bounds and one run at a time hold as for every `do.task`.
//! - **The fallback, never late** ([`FALLBACK_WITHIN`], [`awaited`]): the
//!   run's say waits for the agent's line at most 10 s from the fire, on
//!   the run's own thread (never the trigger's); the line comes, it is said
//!   as exact words in the say's place ([`with_line`]), with the look, the
//!   light and the move as the say brings them. Not within 10 s, or the
//!   agent down, not reached, refused, or answering
//!   `-`, nothing, or more than one short line ([`heard`]), the skill's own
//!   say is said at once, as with the switch off. A line that comes after
//!   is dropped, never said late, and a failed run says nothing of its own.
use crate::{Call, SAY_EXPIRES_MS, SAY_PRIORITY, Words};
use std::{fs, path::Path, sync::mpsc, time::Duration};

/// The switch, in lamp-skills and its manage helper: `on` for every
/// [`Group`], or the groups' names with commas between (`pilot,nudges`).
pub const ENVIRONMENT: &str = lamp_voice::news_agent::ENVIRONMENT;

/// The groups the skills go on by, each by its own switch-on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    /// Sip of water and Stretch break.
    Pilot,
    /// The other wellbeing and room nudges.
    Nudges,
}

impl Group {
    /// Its name in [`ENVIRONMENT`].
    pub fn name(self) -> &'static str {
        match self {
            Self::Pilot => "pilot",
            Self::Nudges => "nudges",
        }
    }
}

/// Whether [`ENVIRONMENT`]'s `value` turns `group` on: `on`, or a list
/// naming it. Anything else, a typo or an unknown name, turns on nothing
/// more.
pub fn turns_on(value: Option<&str>, group: Group) -> bool {
    // One rule with lamp-voice's `news` group.
    lamp_voice::news_agent::turns_on(value, group.name())
}

/// One `do.task` of a pilot: for the line of its trigger `on` (a skill whose
/// steps name their triggers), or of every fire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Task {
    pub on: Option<&'static str>,
    /// The skill's own plain intent for that line, from its `SKILL.md` and
    /// its say's note, never a guess at a person's words.
    pub task: &'static str,
}

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
    /// The switch-on it goes on by.
    pub group: Group,
    /// What fires it, in plain words, for the brief.
    pub fire: &'static str,
    /// What the lamp says now, in plain words, for the brief.
    pub says: &'static str,
    /// Its `do.task`s, in the order of its steps.
    pub tasks: &'static [Task],
}

/// What a brief says the lamp says now, for a skill of more than one task.
const THIS_TASKS_LINE: &str = "the line this run's task names, for this moment";

/// The skills whose line goes through the agent. A quick reaction, a skill
/// whose fire is only light, a fixed sound or a move, a fixed line, a
/// `do.read`, or a skill a conversation calls (its line is the
/// conversation's reply) never does.
pub const PILOTS: [Pilot; 11] = [
    Pilot {
        name: "sip-of-water",
        group: Group::Pilot,
        fire: "someone has sat at the desk a while, half an hour or more",
        says: "one short, friendly line suggesting a sip of water; no lecture, nothing about \
               health",
        tasks: &[Task {
            on: None,
            task: "One short, friendly line suggesting a sip of water; no lecture, nothing about \
                   health.",
        }],
    },
    Pilot {
        name: "stretch-break",
        group: Group::Pilot,
        fire: "someone has been at the desk about fifty minutes without getting up, and the lamp \
               stretches",
        says: "one short, playful line, as an invitation to take a break, as the lamp stretches \
               too; never a lecture",
        tasks: &[Task {
            on: None,
            task: "One short, playful line, as an invitation to take a break, as the lamp \
                   stretches too; never a lecture.",
        }],
    },
    Pilot {
        name: "stand-up",
        group: Group::Nudges,
        fire: "someone has sat at the desk over an hour and a half without getting up, and the \
               lamp tilts toward them",
        says: "one short, friendly line inviting them to stand up and move about for a minute; \
               never a word about posture",
        tasks: &[Task {
            on: None,
            task: "One short, friendly line inviting them to stand up and move about for a \
                   minute; never a word about posture.",
        }],
    },
    Pilot {
        name: "rest-your-eyes",
        group: Group::Nudges,
        fire: "someone has looked at the screen about three quarters of an hour, and the lamp \
               tilts and softens its light",
        says: "one short, light line suggesting a rest for the eyes, perhaps looking at \
               something far away",
        tasks: &[Task {
            on: None,
            task: "One short, light line suggesting a rest for the eyes, perhaps looking at \
                   something far away.",
        }],
    },
    Pilot {
        name: "sit-back",
        group: Group::Nudges,
        fire: "someone has leaned in toward the desk a while, or sat back after the lamp's word",
        says: THIS_TASKS_LINE,
        tasks: &[
            Task {
                on: Some("leaned_in"),
                task: "One short, kind line, the way a friend would, that it might be a good \
                       moment to sit back for a bit; nothing about their body, back, neck or \
                       looks, and nothing about health.",
            },
            Task {
                on: Some("sat_well"),
                task: "They sat back after the lamp's word: a warm thanks or that's better, in \
                       two or three words; nothing about their body or looks.",
            },
        ],
    },
    Pilot {
        name: "posture",
        group: Group::Nudges,
        fire: "the posture reading has read most of the last ten minutes at the desk as bad",
        says: THIS_TASKS_LINE,
        tasks: &[
            Task {
                on: Some("neck"),
                task: "One short, kind line, the way a friend would, suggesting they gently bring the \
                       head back over the shoulders; say only a while, never a number, name no \
                       condition or illness, and never mention a camera.",
            },
            Task {
                on: Some("trunk"),
                task: "One short, kind line, the way a friend would, suggesting they sit back and let \
                       the chair support the back; say only a while, never a number, name no \
                       condition or illness, and never mention a camera.",
            },
            Task {
                on: Some("upper_arm"),
                task: "One short, kind line, the way a friend would, suggesting they gently let the \
                       shoulders relax; say only a while, never a number, name no condition or \
                       illness, and never mention a camera.",
            },
            Task {
                on: Some("lower_arm"),
                task: "One short, kind line, the way a friend would, suggesting they adjust the \
                       keyboard so the elbows rest comfortably; say only a while, never a number, \
                       name no condition or illness, and never mention a camera.",
            },
            Task {
                on: Some("mixed"),
                task: "One short, kind line, the way a friend would, suggesting they stand up for a \
                       short stretch; say only a while, never a number, name no condition or \
                       illness, and never mention a camera.",
            },
        ],
    },
    Pilot {
        name: "wind-down",
        group: Group::Nudges,
        fire: "it is the hour before bedtime and someone has been at the desk twenty minutes, \
               and the lamp droops sleepily",
        says: "one short, gentle line suggesting they start to wind down for the night",
        tasks: &[Task {
            on: None,
            task: "One short, gentle line suggesting they start to wind down for the night.",
        }],
    },
    Pilot {
        name: "comfort-break",
        group: Group::Nudges,
        fire: "someone has sat at the desk a long while, an hour and a half or more, with nobody \
               else in view",
        says: "one short, light line, the way a friend would, that it is a good moment for a \
               short break; name no place and no reason, and nothing about health",
        tasks: &[Task {
            on: None,
            task: "One short, light line, the way a friend would, that it is a good moment for a \
                   short break; name no place and no reason, and nothing about health.",
        }],
    },
    Pilot {
        name: "meal-check",
        group: Group::Nudges,
        fire: "someone has been at the desk through most of lunchtime or dinnertime, with nobody \
               else in view",
        says: THIS_TASKS_LINE,
        tasks: &[
            Task {
                on: Some("lunch"),
                task: "Ask once, warmly and in a few words, the way a friend would, whether they \
                       have had lunch yet; one open question, suggest no food, no menu and no \
                       place, and nothing about health.",
            },
            Task {
                on: Some("dinner"),
                task: "Ask once, warmly and in a few words, the way a friend would, whether they \
                       have had dinner yet; one open question, suggest no food, no menu and no \
                       place, and nothing about health.",
            },
        ],
    },
    Pilot {
        name: "quiet-focus",
        group: Group::Nudges,
        fire: "someone has just looked up after working quietly head down a good while",
        says: "one short, warm line of a few words, the way a friend would, that you noticed \
               their focus and it is nicely done; ask nothing, suggest nothing, and nothing about \
               their body or looks, the time, or health",
        tasks: &[Task {
            on: Some("looked_up"),
            task: "One short, warm line of a few words, the way a friend would, that you noticed \
                   their focus and it is nicely done; ask nothing, suggest nothing, and nothing \
                   about their body or looks, the time, or health.",
        }],
    },
    Pilot {
        name: "stuffy-air",
        group: Group::Nudges,
        fire: "the air at the desk has been stuffy for ten minutes, and the lamp droops and dims",
        says: "one short line suggesting some fresh air, such as opening a window or a door",
        tasks: &[Task {
            on: None,
            task: "One short line suggesting some fresh air, such as opening a window or a door.",
        }],
    },
];

/// The pilot named `name`, if it is one.
pub fn pilot(name: &str) -> Option<&'static Pilot> {
    PILOTS.iter().find(|pilot| pilot.name == name)
}

/// `pilot`'s `scripts/agent.py`: the brief the agent reads at each run,
/// naming what fired it and what the lamp says now, asking for that one
/// line alone and `-` when nothing fits. One plain Python string. Kept,
/// byte for byte, in this crate's `src/core_agent/briefs/<name>.py`,
/// never in a skill folder of the repository (the SDK's check refuses a
/// script there, #7862): install and the build's bundle write it beside
/// the pilot's `agent.toml` on a lamp (`lamp_update::install`'s
/// `core_brief!`).
pub fn script(pilot: &Pilot) -> String {
    format!(
        "print(\"A skill that comes with the lamp, {}, has fired ({}). What the lamp says now: \
         {}. Answer with that one line alone, the very words the lamp will say, in the \
         language the owner speaks with the lamp: nothing else, no tool, never making, changing \
         or removing a skill. If nothing fitting comes to mind, answer -.\")\n",
        pilot.name, pilot.fire, pilot.says
    )
}

#[cfg(test)]
thread_local! {
    /// The switch as a test of this thread sets it, for every group.
    static ON_HERE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Whether the switch turns `group` on in this process.
pub fn on(group: Group) -> bool {
    #[cfg(test)]
    if let Some(on) = ON_HERE.with(std::cell::Cell::get) {
        return on;
    }
    turns_on(std::env::var(ENVIRONMENT).ok().as_deref(), group)
}

/// The switch for this test's thread alone.
#[cfg(test)]
pub fn set_on_here(on: Option<bool>) {
    ON_HERE.with(|here| here.set(on));
}

/// Whether the folder `dir` is a pilot's, named as its folder, with its
/// variant exactly the lamp's own ([`variant`]), whatever the switch: its
/// task is then pre-approved (the first builder, 19 Nov 08:57: "They're
/// ours and have no grants"), so no yes is ever asked for it, aloud or of
/// lamp-control, which mints its one-run token with no grant. Anything
/// else, and the owner's yes holds as for every `do.task`.
pub fn pre_approved(dir: &Path) -> bool {
    let Some(pilot) = dir
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(pilot)
    else {
        return false;
    };
    let file = dir.join("skill.toml");
    if !fs::symlink_metadata(&file).is_ok_and(|meta| meta.is_file()) {
        return false;
    }
    fs::read_to_string(&file)
        .ok()
        .and_then(|text| toml::from_str::<toml::Table>(&text).ok())
        .is_some_and(|table| matches!(variant(pilot, dir, &table), Ok(Some(_))))
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
    match pilot(name).filter(|pilot| on(pilot.group)) {
        Some(pilot) => variant(pilot, dir, table),
        None => Ok(None),
    }
}

/// [`overlaid`] for `pilot`, whatever the switch.
fn variant(pilot: &Pilot, dir: &Path, table: &toml::Table) -> Result<Option<toml::Table>, String> {
    let name = pilot.name;
    let text = match fs::read_to_string(dir.join(OVERLAY)) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let mut overlay: toml::Table = toml::from_str(&text).map_err(|error| error.to_string())?;
    // Meal check and Quiet focus come from the Store: installable.
    if !matches!(
        table.get("kind").and_then(toml::Value::as_str),
        Some("core" | "installable")
    ) {
        return Err("only a core or installable skill's line goes through the agent so".into());
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

/// `steps` checked as the variant's: the pilot's tasks in order, each its
/// words running [`RUN`], alone in its table but for its `on`, and every
/// other step `own`'s, in order, word for word (the fallback is the
/// skill's own say).
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
    let tasks: Vec<toml::Value> = list.iter().filter(|step| task(step)).cloned().collect();
    let wanted: Vec<toml::Value> = pilot
        .tasks
        .iter()
        .map(|task| {
            let mut words = toml::Table::new();
            words.insert("task".into(), toml::Value::String(task.task.into()));
            words.insert("run".into(), toml::Value::String(RUN.into()));
            let mut alone = toml::Table::new();
            alone.insert("task".into(), toml::Value::Table(words));
            if let Some(on) = task.on {
                alone.insert("on".into(), toml::Value::String(on.into()));
            }
            toml::Value::Table(alone)
        })
        .collect();
    if tasks != wanted {
        return Err(format!(
            "its tasks are the lamp's own for {}, each {{ task, run = {RUN:?} }} alone in its \
             [[do]] but for its on",
            pilot.name
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
