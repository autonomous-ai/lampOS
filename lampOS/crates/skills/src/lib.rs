//! Physical skills (`docs/skills.md`). A skill is a folder; this crate loads
//! one, decides from what the bus carries when it runs, and says which calls
//! its steps make. Sending them is the program's (`main.rs`).
//!
//! Time here is microseconds of one monotonic clock, given by the caller:
//! the program's own clock when a message arrives, a message's `at_us` when
//! a recording is replayed.
use lamp_bus::Header;
use lamp_msgs::{Faces, Light, SkillEvent, SkillStage, Speech};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
};

pub mod clock;
pub mod glow;

/// Whether the body plays `name` on its mood socket: one of the clips in its
/// own table (`lamp_body::clips::ALL`), but `idle`, which is the body's own.
pub fn gesture_known(name: &str) -> bool {
    name != "idle" && lamp_body::clips::named(name).is_some()
}

/// How long the body's clip `name` plays, if it is one.
pub fn clip_us(name: &str) -> Option<u64> {
    lamp_body::clips::named(name).map(lamp_body::clips::Clip::duration_us)
}

/// With `LAMP_BODY_REQUESTS=on`, each gesture and move a skill sends on the
/// body's mood socket says who asks (#940).
pub const BODY_REQUESTS_ENVIRONMENT: &str = "LAMP_BODY_REQUESTS";

/// A gesture's or a move's datagram for the body's mood socket. With
/// `skill`, it says who asks, `skill:<name>` at `skill` priority, as a body
/// with `--arbitration on` weighs it (#940); with its switch off, a body
/// with #1239 ignores the two fields, and only an older one refuses them.
pub fn body_ask(mut ask: Value, skill: Option<&str>) -> String {
    if let Some(skill) = skill {
        ask["source"] = json!(format!("skill:{skill}"));
        ask["priority"] = json!(lamp_msgs::Priority::Skill);
    }
    ask.to_string()
}

/// The body's step: `lamp_body::body::TICK`, which is built for Linux alone.
pub const BODY_STEP_US: u64 = 20_000;

/// A clip's length and this is when the body would have played it out had
/// it begun the clip as it was sent: the earliest the light is given back
/// after the last clip, and, with 3 s more, how long the next waits for
/// `body/gesture` to say `over`. The body reads a gesture once, at a step,
/// and lets a clip go at the first step that reaches its length, after that
/// step's read (`crates/body/src/body.rs`); 5 steps (100 ms) cover that and
/// steps run late on a loaded lamp. The next clip itself is sent on `over`
/// (#1199).
pub const CLIP_GAP_US: u64 = 5 * BODY_STEP_US;
// A few of the body's steps, and at least 60 ms (the reviewer on #1012).
const _: () = assert!(CLIP_GAP_US >= 3 * BODY_STEP_US && CLIP_GAP_US >= 60_000);

/// What the voice keeps of what a person said (`crates/voice/src/thread.rs`):
/// the most bytes, and how long a piece not finished waits for more.
const HEARD_BYTES: usize = 300;
const HEARD_WITHIN_US: u64 = 20_000_000;

/// Adds `text` to `to` as the voice's thread does: control characters and
/// whitespace as one space, and nothing past `most` bytes.
fn add_heard(to: &mut String, text: &str, most: usize) {
    for c in text.chars() {
        let c = if c.is_control() || c.is_whitespace() {
            ' '
        } else {
            c
        };
        if to.len() + c.len_utf8() > most || (c == ' ' && to.ends_with(' ')) {
            continue;
        }
        to.push(c);
    }
}

/// The whole words of `text`, lowercase: what is between characters that
/// are not letters or digits.
fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}
/// The most values a tool's choice may have.
const MOST_CHOICES: usize = 16;
/// The pose a skill may hold on the body's mood socket (#1011): `reading`
/// looks down at the desk and leans forward while it is said again, at
/// least every 1.5 s. The voice's own moods are not a skill's.
pub const MOODS: [&str; 1] = ["reading"];
/// How long a skill may hold a pose.
pub const MOOD_AT_MOST_US: u64 = 120_000_000;
/// The longest a held-back chance may wait for its conditions.
const MOST_WAIT_US: u64 = 3_600_000_000;
/// How long a face message counts when its header names no lifespan: the
/// eyes' two seconds. A value on another topic counts until it is replaced.
const FACE_LIFESPAN_MS: u64 = 2_000;
/// How long a held run's trigger topic may say nothing, when its newest
/// value names no lifespan, before the run ends (#1021: until the air part
/// says its readings expire).
pub const SILENT_ENDS_US: u64 = 15 * 60 * 1_000_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SkillFile {
    #[serde(default)]
    needs: Vec<String>,
    /// One `[trigger]` table, or `[[trigger]]` tables.
    trigger: Option<toml::Value>,
    #[serde(default, rename = "while")]
    conditions: Vec<TestFile>,
    ask: Option<AskFile>,
    /// A function the model may call, which runs the steps.
    tool: Option<String>,
    /// The tool's one argument the model picks from named values.
    choice: Option<ChoiceFile>,
    /// The tool's one argument that is yes or no.
    flag: Option<FlagFile>,
    /// When the tool runs again soon after it ran.
    again: Option<AgainFile>,
    /// What the model is told of a call while the tool's run, which holds
    /// no pose, is under way.
    still: Option<String>,
    /// Local hours when the skill never starts itself.
    quiet: Option<QuietFile>,
    #[serde(rename = "do")]
    steps: Vec<StepFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QuietFile {
    from: String,
    to: String,
}

/// A trigger's `rearm`: a word, or a line its field must stay past for a
/// while.
#[derive(Deserialize)]
#[serde(untagged)]
enum RearmFile {
    Word(String),
    Line(RearmLine),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RearmLine {
    above: Option<f64>,
    below: Option<f64>,
    #[serde(rename = "for")]
    lasting: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChoiceFile {
    name: String,
    about: String,
    one_of: Vec<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FlagFile {
    name: String,
    about: String,
}

/// `[again]`: within `within` of the tool's last run, a call runs only if
/// what was heard since asks for it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AgainFile {
    within: String,
    if_heard: Vec<String>,
    /// What the model is told when a call is refused.
    told: Option<String>,
}

/// `[again]` as loaded: each entry as its whole words, lowercase.
#[derive(Clone, Debug, PartialEq)]
struct Again {
    within_us: u64,
    if_heard: Vec<Vec<String>>,
    told: String,
}

/// What the model is told of a call while the run is under way, when the
/// skill does not say.
const STILL_TOLD: &str = "I'm still doing that.";

/// What the model is told of a call `[again]` refused, when the skill does
/// not say.
const AGAIN_TOLD: &str = "Not done: you just did this. Do it again only if they ask for it again.";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TriggerFile {
    topic: String,
    /// The trigger's name, which a step's `on` names.
    name: Option<String>,
    /// A field that is a list: each of its items is watched on its own.
    each: Option<String>,
    /// The field of an item that names it, with `each`.
    key: Option<String>,
    when: Option<String>,
    field: Option<String>,
    equals: Option<toml::Value>,
    above: Option<f64>,
    below: Option<f64>,
    /// On `humans/faces/tracked`: only a face at least this share of the
    /// picture's height counts.
    near: Option<f32>,
    absent_for: Option<String>,
    #[serde(rename = "for")]
    lasting: Option<String>,
    gap: Option<String>,
    rearm: Option<RearmFile>,
    cooldown: Option<String>,
    /// A chance held back by a `[[while]]` condition waits this long, while
    /// the trigger still holds, for the conditions to hold.
    wait: Option<String>,
}

/// A `[[while]]` condition.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TestFile {
    topic: String,
    field: Option<String>,
    equals: Option<toml::Value>,
    /// Holds while the field is anything else (#1140: the lamp not
    /// speaking, whatever else the room's talk does).
    not_equals: Option<toml::Value>,
    above: Option<f64>,
    below: Option<f64>,
    present: Option<bool>,
    /// It has held this long.
    #[serde(rename = "for")]
    lasting: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AskFile {
    policy: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StepFile {
    body: Option<BodyFile>,
    light: Option<LightStep>,
    /// A note in words, or `{ from = "schedule" | "tasks" }` for words the
    /// voice keeps itself (#939): read by hand ([`say_step`]), so that a
    /// wrong one is told in plain words.
    say: Option<toml::Value>,
    /// A file in `sounds/`, by its name without `.wav`.
    sound: Option<String>,
    look: Option<bool>,
    /// `"say"`: this step and every one after it wait for the say to end.
    after: Option<String>,
    /// The name of the trigger this step is made for; for every one without.
    on: Option<String>,
}

/// What a `say` asks of the voice: a note it phrases, or what it keeps.
fn say_step(say: toml::Value) -> Result<Call, String> {
    const WHAT: &str = "say is a note in words, or { from = \"schedule\" } or { from = \"tasks\" }";
    match say {
        toml::Value::String(note) if note.trim().is_empty() => Err(WHAT.into()),
        toml::Value::String(note) => Ok(Call::Say(note)),
        toml::Value::Table(table) => {
            let mut keys = table.keys();
            if keys.next().map(String::as_str) != Some("from") || keys.next().is_some() {
                return Err(format!("{WHAT}: a say table has from and nothing else"));
            }
            match table["from"].as_str() {
                Some("schedule") => Ok(Call::SayFrom(Source::Schedule, None)),
                Some("tasks") => Ok(Call::SayFrom(Source::Tasks, None)),
                Some(other) => Err(format!(
                    "say.from is \"schedule\" or \"tasks\", not {other:?}"
                )),
                None => Err(format!("{WHAT}: from is a word")),
            }
        }
        _ => Err(WHAT.into()),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BodyFile {
    look_at: Option<String>,
    gesture: Option<GestureFile>,
    /// The flag's name: the gesture is played twice when the call sets it.
    again_if: Option<String>,
    /// A file in `moves/`.
    #[serde(rename = "move")]
    clip: Option<String>,
    mood: Option<String>,
    #[serde(rename = "for")]
    lasting: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum GestureFile {
    One(String),
    /// For each value of the choice, the clips one is picked from.
    ByChoice(BTreeMap<String, Vec<String>>),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum LightStep {
    /// A file in `lights/`.
    Named(String),
    Inline(LightFile),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LightFile {
    colour: Option<String>,
    level: Option<u8>,
    #[serde(rename = "for")]
    lasting: Option<String>,
    dim: Option<u8>,
    until: Option<String>,
}

/// A skill as loaded: everything in it checked.
#[derive(Clone, Debug)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub needs: Vec<String>,
    pub triggers: Vec<Trigger>,
    conditions: Vec<Condition>,
    /// The Policy's question; empty for a skill started by its tool alone.
    pub policy: String,
    /// The function the model may call to run the steps, described to it
    /// with the skill's description.
    pub tool: Option<String>,
    choice: Option<ChoiceFile>,
    flag: Option<FlagFile>,
    again: Option<Again>,
    still: String,
    /// Local minutes of the day, from and to, when it never starts itself.
    quiet: Option<(u16, u16)>,
    /// Each step, and the name of the trigger it is made for, if only one.
    steps: Vec<(Option<String>, Step)>,
}

impl Skill {
    /// Whether a step holds something until its trigger stops holding.
    fn holds_until_ends(&self) -> bool {
        self.steps.iter().any(|(_, step)| {
            matches!(
                step,
                Step::Call(Call::Dim {
                    until_ends: true,
                    ..
                })
            )
        })
    }
}

/// The line a trigger's field must stay past for a while before the trigger
/// runs again.
#[derive(Clone, Debug)]
pub struct Rearm {
    test: Test,
    pub for_us: u64,
}

/// A step as loaded, made into calls when the skill runs.
#[derive(Clone, Debug, PartialEq)]
enum Step {
    Call(Call),
    /// One clip, picked from those listed for the call's choice, or from all
    /// of them when the call gives none; twice when `again` and the call sets
    /// the flag. A gesture written as one name is listed under "".
    Gesture {
        by: Vec<(String, Vec<String>)>,
        again: bool,
    },
}

/// What a message must show for a trigger or a condition to hold.
#[derive(Clone, Debug, PartialEq)]
enum Test {
    /// A face in the list of `humans/faces/tracked`, at least this share of
    /// the picture's height.
    Faces(f32),
    Equals(String, Value),
    NotEquals(String, Value),
    Above(String, f64),
    Below(String, f64),
}

impl Test {
    fn holds(&self, value: &Value) -> bool {
        match self {
            Self::Faces(near) => value["faces"].as_array().is_some_and(|faces| {
                faces.iter().any(|face| {
                    face["ymax"].as_f64().unwrap_or(0.0) - face["ymin"].as_f64().unwrap_or(0.0)
                        >= f64::from(*near)
                })
            }),
            Self::Equals(field, wanted) => equal(&value[field], wanted),
            Self::NotEquals(field, unwanted) => !equal(&value[field], unwanted),
            Self::Above(field, line) => value[field].as_f64().is_some_and(|got| got > *line),
            Self::Below(field, line) => value[field].as_f64().is_some_and(|got| got < *line),
        }
    }
}

/// A number is compared as a number, anything else exactly.
fn equal(got: &Value, wanted: &Value) -> bool {
    match (got.as_f64(), wanted.as_f64()) {
        (Some(got), Some(wanted)) => got == wanted,
        _ => got == wanted,
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Condition {
    topic: String,
    test: Test,
    for_us: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum When {
    /// Holds where it did not just before: once each time.
    Appears,
    /// Holds, and has for `for`.
    Present,
}

#[derive(Clone, Debug)]
pub struct Trigger {
    pub topic: String,
    /// What a step's `on` calls it.
    pub name: Option<String>,
    /// The list each of whose items is watched on its own, and the field
    /// that names an item. The test is then of an item.
    pub each: Option<(String, String)>,
    test: Test,
    pub when: When,
    pub absent_for_us: u64,
    pub for_us: u64,
    pub gap_us: u64,
    /// Once each time it holds; again only after it stopped.
    pub rearm: bool,
    /// And, with a line, only after its field stayed past it for a while.
    pub rearm_line: Option<Rearm>,
    pub cooldown_us: u64,
    /// How long a chance held back by a `[[while]]` condition is kept, while
    /// the trigger still holds, and taken the moment the conditions hold:
    /// someone talking when the moment comes, and the lamp acts once the
    /// talk ends. 0: a chance held back is spent.
    pub wait_us: u64,
}

/// One call a step makes, as the program sends it and a test expects it.
#[derive(Clone, Debug, PartialEq)]
pub enum Call {
    /// To the body's gaze socket, as the eyes send a face: its centre from
    /// the middle of the picture (-1 to 1) and its height as a share of it.
    /// Before a run it names no face.
    Gaze { face: Option<(f32, f32, f32)> },
    /// A gesture by name, to the body's mood socket.
    Gesture(String),
    /// A move of the skill's own (`moves/NAME.json`), checked as the body
    /// checks it, sent whole to the body's mood socket and played there as
    /// a gesture.
    Move(lamp_body::moves::Move),
    /// A pose held on the body's mood socket for `lasting_us`, said again
    /// every second; the body comes back when it is no longer said.
    Mood { mood: String, lasting_us: u64 },
    /// A colour, to the actions part's `light_colour`, then stepped to its
    /// level; held for its while in a run with no say.
    Light {
        colour: String,
        level: Option<u8>,
        lasting: Option<String>,
    },
    /// Steps dimmer, to the actions part's `light_dimmer`; with
    /// `until_ends`, held until the trigger stops holding, then given back.
    Dim { steps: u8, until_ends: bool },
    /// A note for the voice to say in its own words.
    Say(String),
    /// Words the voice keeps itself and says when asked (#939, (c)): the
    /// reminder due, or the news of work handed over. The voice keeps the
    /// words and their delivery (a reminder's acknowledgement, news told
    /// once); the skill says when, and moves with them.
    /// With the task an item's run is for (a trigger with `each` and a
    /// numeric key), sent as `"task"` so the voice gives that task's news
    /// and no other (#1237).
    SayFrom(Source, Option<u64>),
    /// The voice's full look (#836), which it takes when the lamp says it
    /// will look. Only in a skill its tool starts.
    Look,
    /// A short sound of the skill's own, `sounds/NAME.wav` (checked at load,
    /// [`check_sound`]), played by the voice through its say service.
    Sound { name: String, path: PathBuf },
    /// The calls after this wait for the say to end: the skill's own say, or
    /// in a skill its tool starts, the line the model says after the call.
    AfterSay,
    /// The trigger of a run that holds something until it ends stopped
    /// holding: what it held is given back. Alone in its run.
    Ends,
}

/// Where the words of a [`Call::SayFrom`] are kept, in the voice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The reminder Schedule has due (`schedule/due`).
    Schedule,
    /// The news of work handed to an agent (`tasks/status`).
    Tasks,
}

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Self::Schedule => "schedule",
            Self::Tasks => "tasks",
        }
    }

    /// The say service's priority: a reminder at its time, after a second
    /// of quiet (8, as the voice's own reminders); news in a pause, after
    /// 15 s of quiet (5, as the voice's own news).
    pub fn priority(self) -> u8 {
        match self {
            Self::Schedule => 8,
            Self::Tasks => 5,
        }
    }
}

impl Call {
    /// The call's name and its arguments, as a test names them.
    pub fn named(&self) -> (&'static str, Value) {
        match self {
            Self::Gaze { face } => {
                let mut args = json!({ "look_at": "face" });
                if let Some((x, y, size)) = face {
                    args["x"] = json!(x);
                    args["y"] = json!(y);
                    args["size"] = json!(size);
                }
                ("body.gaze", args)
            }
            Self::Gesture(name) => ("body.gesture", json!({ "gesture": name })),
            Self::Move(clip) => ("body.move", json!({ "move": clip.name })),
            Self::Mood { mood, lasting_us } => (
                "body.mood",
                json!({ "mood": mood, "for_ms": lasting_us / 1_000 }),
            ),
            Self::Light {
                colour,
                level,
                lasting,
            } => {
                let mut args = json!({ "colour": colour });
                if let Some(level) = level {
                    args["level"] = json!(level);
                }
                if let Some(lasting) = lasting {
                    args["for"] = json!(lasting);
                }
                ("light", args)
            }
            Self::Dim { steps, until_ends } => {
                let mut args = json!({ "dim": steps });
                if *until_ends {
                    args["until"] = json!("ends");
                }
                ("light", args)
            }
            Self::Say(note) => ("say", json!({ "note": note })),
            Self::SayFrom(source, task) => {
                let mut args = json!({ "from": source.name() });
                if let Some(task) = task {
                    args["task"] = json!(task);
                }
                ("say", args)
            }
            Self::Look => ("look", json!({})),
            Self::Sound { name, .. } => ("sound", json!({ "sound": name })),
            Self::AfterSay => ("after_say", json!({})),
            Self::Ends => ("ends", json!({})),
        }
    }
}

/// `"5m"` and the like: a whole number of `ms`, `s`, `m` or `h`.
pub fn duration_us(text: &str) -> Result<u64, String> {
    let digits = text.trim_end_matches(|c: char| c.is_ascii_alphabetic());
    let per_unit: u64 = match &text[digits.len()..] {
        "ms" => 1_000,
        "s" => 1_000_000,
        "m" => 60_000_000,
        "h" => 3_600_000_000,
        _ => {
            return Err(format!(
                "{text:?} is not a duration: a whole number of ms, s, m or h"
            ));
        }
    };
    digits
        .parse::<u64>()
        .ok()
        .and_then(|count| count.checked_mul(per_unit))
        .ok_or_else(|| format!("{text:?} is not a duration: a whole number of ms, s, m or h"))
}

fn plain_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
        })
}

/// `name` and `description` from `SKILL.md`'s front matter (Agent Skills).
fn front_matter(text: &str) -> Result<(String, String), String> {
    let body = text
        .strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---"))
        .ok_or("SKILL.md begins with front matter between two --- lines")?
        .0;
    let field = |key: &str| {
        body.lines()
            .find_map(|line| line.strip_prefix(key)?.strip_prefix(':'))
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .ok_or(format!("SKILL.md's front matter has no {key}"))
    };
    Ok((field("name")?, field("description")?))
}

/// A test from `field` and one of `equals`, `above`, `below`; or, on
/// `humans/faces/tracked` with none of them, a face in view.
fn test(
    topic: &str,
    field: Option<String>,
    equals: Option<toml::Value>,
    above: Option<f64>,
    below: Option<f64>,
) -> Result<Test, String> {
    let equals = equals
        .map(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
        .transpose()?;
    match (field, equals, above, below) {
        (None, None, None, None) if topic == Faces::TOPIC => Ok(Test::Faces(0.0)),
        (Some(field), Some(value), None, None) => Ok(Test::Equals(field, value)),
        (Some(field), None, Some(line), None) => Ok(Test::Above(field, line)),
        (Some(field), None, None, Some(line)) => Ok(Test::Below(field, line)),
        _ => Err(format!(
            "on {topic}: a field and one of equals, above or below \
             (only {} can do without, for a face in view)",
            Faces::TOPIC
        )),
    }
}

fn trigger(file: TriggerFile) -> Result<Trigger, String> {
    let optional = |text: &Option<String>| text.as_deref().map_or(Ok(0), duration_us);
    let when = match file.when.as_deref() {
        Some("appears") => When::Appears,
        None | Some("present") => When::Present,
        Some(other) => {
            return Err(format!(
                "when = {other:?}: only \"appears\" and \"present\" are known"
            ));
        }
    };
    let rearm_given = file.rearm.is_some();
    let (rearm, rearm_line) = match file.rearm {
        None => (false, None),
        Some(RearmFile::Word(word)) if word == "leaves" || word == "ends" => (true, None),
        Some(RearmFile::Word(other)) => {
            return Err(format!(
                "rearm = {other:?}: \"leaves\", \"ends\" or {{ below or above, for }}"
            ));
        }
        Some(RearmFile::Line(line)) => {
            let test = match (file.field.clone(), line.above, line.below) {
                (Some(field), Some(above), None) => Test::Above(field, above),
                (Some(field), None, Some(below)) => Test::Below(field, below),
                _ => {
                    return Err(
                        "rearm = { below or above, for }: one line, on a trigger with a field"
                            .into(),
                    );
                }
            };
            let for_us = line.lasting.as_deref().map_or(Ok(0), duration_us)?;
            (true, Some(Rearm { test, for_us }))
        }
    };
    let wait_us = optional(&file.wait)?;
    if wait_us > MOST_WAIT_US {
        return Err("wait is at most 1h".into());
    }
    if wait_us > 0 && !(rearm || when == When::Appears) {
        return Err(
            "wait goes with rearm or when = \"appears\": without them every message is a chance"
                .into(),
        );
    }
    if let Some(name) = &file.name
        && (!plain_name(name) || name.contains('-'))
    {
        return Err(format!(
            "trigger name {name:?}: 1 to 64 lowercase letters, digits and _"
        ));
    }
    let each = match (file.each, file.key) {
        (None, None) => None,
        (Some(list), Some(key)) if !list.is_empty() && !key.is_empty() => {
            // An item's edge is its own: what is timed or counted over the
            // whole topic means nothing for one item (chosen).
            if when != When::Appears
                || file.near.is_some()
                || file.absent_for.is_some()
                || file.lasting.is_some()
                || file.gap.is_some()
                || rearm_given
                || file.cooldown.is_some()
                || file.wait.is_some()
            {
                return Err(
                    "each goes with when = \"appears\", and with no near, absent_for, \
                            for, gap, rearm, cooldown or wait"
                        .into(),
                );
            }
            if file.field.is_none() {
                return Err(
                    "each tests a field of an item, with one of equals, above or below".into(),
                );
            }
            Some((list, key))
        }
        _ => {
            return Err(
                "each names a list field, and key the field that names one of its items".into(),
            );
        }
    };
    let mut test = test(&file.topic, file.field, file.equals, file.above, file.below)?;
    match (file.near, &mut test) {
        (None, _) => {}
        (Some(near), Test::Faces(least)) if (0.0..=1.0).contains(&near) => *least = near,
        _ => {
            return Err(format!(
                "near is a share of the picture from 0 to 1, on {} alone",
                Faces::TOPIC
            ));
        }
    }
    Ok(Trigger {
        test,
        topic: file.topic,
        name: file.name,
        each,
        when,
        absent_for_us: optional(&file.absent_for)?,
        for_us: optional(&file.lasting)?,
        gap_us: optional(&file.gap)?,
        rearm,
        rearm_line,
        cooldown_us: optional(&file.cooldown)?,
        wait_us,
    })
}

/// One skill folder, or why it cannot be loaded.
pub fn load(dir: &Path) -> Result<Skill, String> {
    let read =
        |file: &str| fs::read_to_string(dir.join(file)).map_err(|error| format!("{file}: {error}"));
    let (name, description) = front_matter(&read("SKILL.md")?)?;
    if dir.file_name().and_then(|folder| folder.to_str()) != Some(name.as_str()) {
        return Err(format!(
            "SKILL.md names {name:?}, which is not its folder's name"
        ));
    }
    if !plain_name(&name) || name.contains('_') {
        return Err(format!(
            "{name:?}: a name is 1 to 64 lowercase letters, digits and hyphens"
        ));
    }
    if dir.join("run.py").exists() {
        return Err("run.py is not run yet: a skill with one is not loaded".into());
    }
    let file: SkillFile =
        toml::from_str(&read("skill.toml")?).map_err(|error| format!("skill.toml: {error}"))?;
    let triggers = match file.trigger {
        None => Vec::new(),
        Some(toml::Value::Array(many)) => many,
        Some(one) => vec![one],
    };
    let triggers = triggers
        .into_iter()
        .map(|value| {
            trigger(
                value
                    .try_into()
                    .map_err(|error| format!("skill.toml: trigger: {error}"))?,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    if (triggers.is_empty() && file.tool.is_none()) || file.steps.is_empty() {
        return Err("skill.toml needs a trigger or a tool, and at least one [[do]]".into());
    }
    let policy = file.ask.map(|ask| ask.policy).unwrap_or_default();
    if !triggers.is_empty() && policy.is_empty() {
        return Err("[ask] names the Policy's question".into());
    }
    if let Some(tool) = &file.tool {
        // What the provider link takes (`lamp_gemini::Tool::validate`).
        let mut letters = tool.bytes();
        let first = letters
            .next()
            .is_some_and(|first| first.is_ascii_lowercase());
        if !first
            || tool.len() > 48
            || !letters
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        {
            return Err(format!(
                "tool {tool:?}: a lowercase letter, then at most 47 lowercase letters, digits and _"
            ));
        }
        if description.len() > 1_024 {
            return Err("a skill with a tool has a description of at most 1,024 bytes".into());
        }
    }
    let argument = |name: &str, about: &str| -> Result<(), String> {
        if file.tool.is_none() {
            return Err("a choice or a flag is an argument of the skill's tool".into());
        }
        let mut letters = name.bytes();
        if !letters
            .next()
            .is_some_and(|first| first.is_ascii_lowercase())
            || name.len() > 32
            || !letters
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        {
            return Err(format!(
                "argument {name:?}: a lowercase letter, then at most 31 lowercase letters, digits and _"
            ));
        }
        if about.trim().is_empty() || about.len() > 256 {
            return Err(format!(
                "argument {name:?}: its about is what it means, in at most 256 bytes"
            ));
        }
        Ok(())
    };
    if let Some(choice) = &file.choice {
        argument(&choice.name, &choice.about)?;
        let mut values = choice.one_of.clone();
        values.sort();
        values.dedup();
        if choice.one_of.is_empty()
            || choice.one_of.len() > MOST_CHOICES
            || values.len() != choice.one_of.len()
            || !values.iter().all(|value| plain_name(value))
        {
            return Err(format!(
                "choice {:?}: one_of is 1 to {MOST_CHOICES} different plain names",
                choice.name
            ));
        }
    }
    if let Some(flag) = &file.flag {
        argument(&flag.name, &flag.about)?;
        if file
            .choice
            .as_ref()
            .is_some_and(|choice| choice.name == flag.name)
        {
            return Err("the choice and the flag have different names".into());
        }
    }
    let again = match file.again {
        None => None,
        Some(_) if file.tool.is_none() => {
            return Err("[again] is about the skill's tool: a skill with one".into());
        }
        Some(again) => {
            let within_us = duration_us(&again.within).map_err(|why| format!("[again]: {why}"))?;
            let if_heard: Vec<Vec<String>> =
                again.if_heard.iter().map(|entry| words(entry)).collect();
            if if_heard.is_empty() || if_heard.iter().any(Vec::is_empty) {
                return Err("[again]: if_heard lists at least one word, none of them empty".into());
            }
            let told = again.told.unwrap_or_else(|| AGAIN_TOLD.into());
            if told.trim().is_empty() || told.len() > 256 {
                return Err("[again]: told is what the model is told, in at most 256 bytes".into());
            }
            Some(Again {
                within_us,
                if_heard,
                told,
            })
        }
    };
    let still = match file.still {
        None => STILL_TOLD.to_owned(),
        Some(_) if file.tool.is_none() => {
            return Err("still is about the skill's tool: a skill with one".into());
        }
        Some(still) if still.trim().is_empty() || still.len() > 256 => {
            return Err("still is what the model is told, in at most 256 bytes".into());
        }
        Some(still) => still,
    };
    let mut conditions = Vec::new();
    for condition in file.conditions {
        let test = match (condition.present, condition.not_equals) {
            (None, Some(unwanted)) => match (
                condition.field,
                condition.equals,
                condition.above,
                condition.below,
            ) {
                (Some(field), None, None, None) => Test::NotEquals(
                    field,
                    serde_json::to_value(unwanted).map_err(|error| error.to_string())?,
                ),
                _ => {
                    return Err(format!(
                        "on {}: not_equals takes a field and nothing else of equals, above or below",
                        condition.topic
                    ));
                }
            },
            (Some(_), Some(_)) => {
                return Err("not_equals and present are one or the other".into());
            }
            (None, None) => test(
                &condition.topic,
                condition.field,
                condition.equals,
                condition.above,
                condition.below,
            )?,
            (Some(true), None) if condition.topic == Faces::TOPIC && condition.field.is_none() => {
                Test::Faces(0.0)
            }
            (Some(_), None) => {
                return Err(format!(
                    "present = true is known only on {}, alone",
                    Faces::TOPIC
                ));
            }
        };
        conditions.push(Condition {
            topic: condition.topic,
            test,
            for_us: condition.lasting.as_deref().map_or(Ok(0), duration_us)?,
        });
    }
    let quiet = match file.quiet {
        None => None,
        Some(_) if triggers.is_empty() => {
            return Err("quiet hours are for a skill with a trigger".into());
        }
        Some(quiet) => {
            let (from, to) = (minute_of_day(&quiet.from)?, minute_of_day(&quiet.to)?);
            if from == to {
                return Err("quiet: from and to are different times".into());
            }
            Some((from, to))
        }
    };
    let names: Vec<&String> = triggers
        .iter()
        .filter_map(|trigger| trigger.name.as_ref())
        .collect();
    if (1..names.len()).any(|at| names[..at].contains(&names[at])) {
        return Err("two triggers have one name".into());
    }
    let any_on = file.steps.iter().any(|step| step.on.is_some());
    let mut steps = Vec::new();
    // The trigger each step is for, beside it: one for each step made.
    let mut ons: Vec<Option<String>> = Vec::new();
    let mut said = false;
    let mut after_say = false;
    for step in file.steps {
        if step.body.is_none()
            && step.light.is_none()
            && step.say.is_none()
            && step.sound.is_none()
            && step.look.is_none()
        {
            return Err("a [[do]] step names body, light, say, sound or look".into());
        }
        if let Some(on) = &step.on
            && !names.contains(&on)
        {
            return Err(format!("on = {on:?} names none of the skill's triggers"));
        }
        // Steps for one trigger and steps after the say are not put together
        // until a skill needs both (chosen).
        if any_on && step.after.is_some() {
            return Err("after = \"say\" in a skill whose steps name their trigger".into());
        }
        let on = step.on.clone();
        match step.after.as_deref() {
            None => {}
            Some("say") if after_say => {}
            // The skill's say in an earlier step, or the model's line.
            Some("say") if (said || file.tool.is_some()) && step.say.is_none() => {
                after_say = true;
                steps.push(Step::Call(Call::AfterSay));
            }
            Some("say") => {
                return Err(
                    "after = \"say\" follows a say: one in an earlier step, or the model's line \
                     in a skill with a tool"
                        .into(),
                );
            }
            Some(other) => return Err(format!("after = {other:?}: only \"say\"")),
        }
        if after_say && step.say.is_some() {
            return Err("a say after the say ends".into());
        }
        match step.look {
            None => {}
            Some(true) if file.tool.is_some() && triggers.is_empty() => {
                steps.push(Step::Call(Call::Look))
            }
            Some(_) => {
                return Err(
                    "look = true, in a skill started by its tool alone: the model takes the look"
                        .into(),
                );
            }
        }
        if let Some(body) = step.body {
            match body.look_at.as_deref() {
                None => {}
                Some("face") => steps.push(Step::Call(Call::Gaze { face: None })),
                Some(other) => return Err(format!("look_at = {other:?}: only \"face\"")),
            }
            let again = match (&body.again_if, &file.flag) {
                (None, _) => false,
                (Some(name), Some(flag)) if *name == flag.name && body.gesture.is_some() => true,
                (Some(name), _) => {
                    return Err(format!(
                        "again_if = {name:?} names the skill's flag, beside a gesture"
                    ));
                }
            };
            if let Some(gesture) = body.gesture {
                let by = match gesture {
                    GestureFile::One(name) => vec![(String::new(), vec![name])],
                    GestureFile::ByChoice(by) => {
                        let Some(choice) = &file.choice else {
                            return Err("a gesture by choice needs a [choice]".into());
                        };
                        let mut values = choice.one_of.clone();
                        values.sort();
                        if by.keys().cloned().collect::<Vec<_>>() != values
                            || by.values().any(Vec::is_empty)
                        {
                            return Err(format!(
                                "a gesture by choice lists clips for each of {:?}, and only those",
                                choice.one_of
                            ));
                        }
                        by.into_iter().collect()
                    }
                };
                for name in by.iter().flat_map(|(_, names)| names) {
                    if !gesture_known(name) {
                        return Err(format!(
                            "gesture {name:?} is not one of the body's clips (lamp_body::clips::ALL)"
                        ));
                    }
                }
                steps.push(Step::Gesture { by, again });
            }
            if let Some(clip) = body.clip {
                steps.push(Step::Call(Call::Move(move_file(dir, &clip)?)));
            }
            match (body.mood, body.lasting) {
                (None, None) => {}
                (Some(mood), Some(lasting)) => {
                    if !MOODS.contains(&mood.as_str()) {
                        return Err(format!("mood {mood:?} is not one of {MOODS:?}"));
                    }
                    let lasting_us = duration_us(&lasting)?;
                    if lasting_us == 0 || lasting_us > MOOD_AT_MOST_US {
                        return Err("a mood's for is more than 0 and at most 2m".into());
                    }
                    steps.push(Step::Call(Call::Mood { mood, lasting_us }));
                }
                _ => return Err("a body mood goes with its for, and a for with a mood".into()),
            }
        }
        if let Some(light) = step.light {
            let (file, place) = match light {
                LightStep::Named(light) => {
                    if !plain_name(&light) {
                        return Err(format!("light {light:?} is not a file name in lights/"));
                    }
                    let place = format!("lights/{light}.toml");
                    let file: LightFile = toml::from_str(&read(&place)?)
                        .map_err(|error| format!("{place}: {error}"))?;
                    (file, place)
                }
                LightStep::Inline(file) => (file, "a light step".into()),
            };
            steps.push(Step::Call(
                light_call(file).map_err(|why| format!("{place}: {why}"))?,
            ));
        }
        if let Some(say) = step.say {
            let call = say_step(say)?;
            // A tool's run tells the model what to say, and what the voice
            // keeps is not the model's to say (Fourth on #1033).
            if matches!(call, Call::SayFrom(..)) && file.tool.is_some() {
                return Err(
                    "say.from, in a skill with a tool: what the voice keeps is said when a trigger starts the skill, never from the model's call"
                        .into(),
                );
            }
            said = true;
            steps.push(Step::Call(call));
        }
        if let Some(name) = step.sound {
            // A tool is called while the voice answers: a sound would always
            // be refused as the lamp speaking.
            if file.tool.is_some() {
                return Err(
                    "sound, in a skill with a tool: the voice is answering the call, so the lamp is speaking"
                        .into(),
                );
            }
            if !plain_name(&name) {
                return Err(format!("sound {name:?} is not a file name in sounds/"));
            }
            let place = format!("sounds/{name}.wav");
            let path = std::path::absolute(dir.join(&place))
                .map_err(|error| format!("{place}: {error}"))?;
            check_sound(&path).map_err(|why| format!("{place}: {why}"))?;
            steps.push(Step::Call(Call::Sound { name, path }));
        }
        ons.resize(steps.len(), on);
    }
    let steps: Vec<(Option<String>, Step)> = ons.into_iter().zip(steps).collect();
    // One say or sound a run: for each trigger, its own steps and those for all.
    let says = |name: Option<&String>| {
        steps
            .iter()
            .filter(|(on, step)| {
                matches!(
                    step,
                    Step::Call(Call::Say(_) | Call::SayFrom(..) | Call::Sound { .. })
                ) && (on.is_none() || on.as_ref() == name)
            })
            .count()
    };
    if says(None) > 1 || names.iter().any(|name| says(Some(name)) > 1) {
        return Err(
            "one say or sound a skill (or one for each trigger its steps name): its other steps are made, \
             in their order, when the voice begins it"
                .into(),
        );
    }
    // A tool's run has no trigger to end it: what it held would be held for
    // good.
    if (triggers.is_empty() || file.tool.is_some())
        && steps.iter().any(|(_, step)| {
            matches!(
                step,
                Step::Call(Call::Dim {
                    until_ends: true,
                    ..
                })
            )
        })
    {
        return Err(
            "until = \"ends\" is the end of a trigger: in a skill with one and no tool".into(),
        );
    }
    // An item's run has no quiet hours and no end of its own: quiet would be
    // passed over, and a dim held until it ends would hold the skill busy
    // (the reviewer on #1042, chosen until a skill needs them).
    if triggers.iter().any(|trigger| trigger.each.is_some()) {
        if quiet.is_some() {
            return Err("[quiet] in a skill with an each trigger: an item's run has none".into());
        }
        if steps.iter().any(|(_, step)| {
            matches!(
                step,
                Step::Call(Call::Dim {
                    until_ends: true,
                    ..
                })
            )
        }) {
            return Err(
                "until = \"ends\" in a skill with an each trigger: an item's run has no end".into(),
            );
        }
    }
    Ok(Skill {
        name,
        description,
        needs: file.needs,
        triggers,
        conditions,
        policy,
        tool: file.tool,
        choice: file.choice,
        flag: file.flag,
        again,
        still,
        quiet,
        steps,
    })
}

/// Whether `path` is a sound the voice will play, by the voice's own check
/// (`lamp_voice::say::read_sound`: a regular file, not a link, of 16-bit
/// mono PCM at 24 kHz, at most 3 s, no 10 ms louder than -30 dBFS RMS and
/// no peak past -12 dBFS), so that the two never differ.
pub fn check_sound(path: &Path) -> Result<(), String> {
    lamp_voice::say::read_sound(path).map(drop)
}

/// `moves/NAME.json`, held to what the body takes (`lamp_body::moves::check`):
/// the body's own clips' bounds, length and rate. Whether it may play from
/// the pose the lamp is in is the body's to judge as it begins.
fn move_file(dir: &Path, name: &str) -> Result<lamp_body::moves::Move, String> {
    if !plain_name(name) {
        return Err(format!("move {name:?} is not a file name in moves/"));
    }
    let place = format!("moves/{name}.json");
    // The body's own reader: a regular file of at most 256 KiB, looked at
    // before it is read, so a pipe or a huge file never stalls the load.
    let clip =
        lamp_body::moves::read(&dir.join(&place)).map_err(|why| format!("{place}: {why}"))?;
    if clip.name != name {
        return Err(format!("{place} names {:?}, not {name:?}", clip.name));
    }
    lamp_body::moves::check(&clip).map_err(|why| format!("{place}: {why}"))?;
    Ok(clip)
}

/// `"22:00"`: minutes since midnight.
fn minute_of_day(text: &str) -> Result<u16, String> {
    let wrong = || format!("{text:?} is not a time of day: HH:MM, from 00:00 to 23:59");
    let (hours, minutes) = text.split_once(':').ok_or_else(wrong)?;
    if hours.len() != 2 || minutes.len() != 2 {
        return Err(wrong());
    }
    match (hours.parse::<u16>(), minutes.parse::<u16>()) {
        (Ok(hours), Ok(minutes)) if hours < 24 && minutes < 60 => Ok(hours * 60 + minutes),
        _ => Err(wrong()),
    }
}

/// Whether `minute` is from `from` up to, not including, `to`, past
/// midnight when `to` comes first.
fn within(minute: u16, (from, to): (u16, u16)) -> bool {
    if from < to {
        (from..to).contains(&minute)
    } else {
        minute >= from || minute < to
    }
}

/// How an item of a list is named by its key field: a text or a number.
fn item_name(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

/// Each item of the trigger's list in `value`, by name, and whether it holds.
fn items_of(trigger: &Trigger, list: &str, key: &str, value: &Value) -> BTreeMap<String, bool> {
    value[list]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| Some((item_name(&item[key])?, trigger.test.holds(item))))
        .collect()
}

/// The order items' runs go in: by number when the names are numbers (task
/// 9 before task 10, as the voice reads them), else by text, numbers first.
fn item_order(name: &str) -> (bool, u64, &str) {
    match name.parse::<u64>() {
        Ok(number) => (false, number, name),
        Err(_) => (true, 0, name),
    }
}

fn light_call(file: LightFile) -> Result<Call, String> {
    let until_ends = match file.until.as_deref() {
        None => false,
        Some("ends") => true,
        Some(other) => return Err(format!("until = {other:?}: only \"ends\"")),
    };
    if let Some(lasting) = &file.lasting
        && duration_us(lasting)? > 600_000_000
    {
        return Err("a light's for is at most 10m".into());
    }
    match (file.colour, file.dim) {
        (Some(colour), None) => {
            if !Light::COLOURS.contains(&colour.as_str()) {
                return Err(format!(
                    "colour {colour:?} is not one of {:?}",
                    Light::COLOURS
                ));
            }
            if file.level.is_some_and(|level| !(1..=5).contains(&level)) {
                return Err("level is 1 to 5".into());
            }
            if until_ends {
                return Err("until goes with dim".into());
            }
            Ok(Call::Light {
                colour,
                level: file.level,
                lasting: file.lasting,
            })
        }
        (None, Some(steps)) if (1..=4).contains(&steps) => {
            if file.level.is_some() || file.lasting.is_some() {
                return Err("dim takes no level and no for".into());
            }
            Ok(Call::Dim { steps, until_ends })
        }
        _ => Err("a light is a colour (with a level and a for) or dim = 1 to 4".into()),
    }
}

/// Every skill folder under `root` but those whose name begins with a dot,
/// and why each other one did not load.
pub fn load_all(root: &Path) -> (Vec<Skill>, Vec<String>) {
    let mut folders: Vec<PathBuf> = fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    folders.sort();
    let (mut skills, mut errors) = (Vec::new(), Vec::new());
    for folder in folders {
        match load(&folder) {
            Ok(skill) => skills.push(skill),
            Err(why) => errors.push(format!("{}: {why}", folder.display())),
        }
    }
    (skills, errors)
}

#[derive(Default)]
struct Watch {
    started_us: Option<u64>,
    last_held_us: Option<u64>,
    /// Since when it has held, through dropouts shorter than the gap.
    since_us: Option<u64>,
    /// How long it had not held before that.
    away_us: u64,
    /// This time it holds has had its chance.
    spent: bool,
    /// Since when this time's chance has waited for the trigger's
    /// `[[while]]` conditions (`wait`).
    waiting_us: Option<u64>,
    ran_us: Option<u64>,
    /// With a rearm line: a chance was had, and the field has not stayed
    /// past the line long enough since.
    disarmed: bool,
    /// Since when the field has been past the rearm line.
    past_line_us: Option<u64>,
    /// A run that holds something until its trigger stops holding is out.
    open: bool,
    /// With `each`: whether each item in its last message held; none before its first.
    items: Option<BTreeMap<String, bool>>,
}

/// A trigger that held: what was said of it, and the calls to make.
#[derive(Debug)]
pub struct Run {
    pub skill: String,
    pub events: Vec<SkillEvent>,
    pub calls: Vec<Call>,
    /// With the Policy asked (`LAMP_SKILLS_ASK=policy`): the question for
    /// `skills/ask` (#958's `SkillAsk` without its number), and the calls
    /// wait for its answer.
    pub ask: Option<Value>,
    /// The run is one item's, from a trigger with `each`: with the skill's
    /// last run under way it waits its turn, where any other is let go.
    pub queue: bool,
    /// The trigger that held, by its place in the skill, and with `each`
    /// the item's name: what [`Engine::still`] asks again of the Policy's
    /// answer. None for a tool's run and an end.
    pub from: Option<(usize, Option<String>)>,
}

pub struct Engine {
    skills: Vec<Skill>,
    /// By skill name and trigger, kept across reloads, so an edit does not
    /// forget a cooldown.
    watches: HashMap<(String, usize), Watch>,
    /// The newest value of each topic: when it came and how long it lasts.
    latest: HashMap<String, (Value, u64, Option<u64>)>,
    /// Since when each skill's condition has held, by skill and condition.
    holding: HashMap<(String, usize), u64>,
    /// When each skill's tool last ran, by skill.
    tool_ran: HashMap<String, u64>,
    /// The newest thing heard on `voice/heard`, its pieces joined as the
    /// voice joins them. An event topic: the bus keeps no latest of it.
    heard: Heard,
    skip_ask: bool,
    ask_policy: bool,
    /// The local time: minutes east of UTC, and one moment on both clocks.
    clock: Option<(i32, u64, u64)>,
}

fn event(skill: &str, stage: SkillStage, why: String, allowed: Option<bool>) -> SkillEvent {
    SkillEvent {
        skill: skill.into(),
        stage,
        why,
        allowed,
    }
}

impl Engine {
    /// `skip_ask`: act without asking the Policy (`LAMP_SKILLS_ASK=skip`).
    /// Without it, or `asking_the_policy`, nothing a skill starts is done.
    pub fn new(skills: Vec<Skill>, skip_ask: bool) -> Self {
        Self {
            skills,
            watches: HashMap::new(),
            latest: HashMap::new(),
            holding: HashMap::new(),
            tool_ran: HashMap::new(),
            heard: Heard::default(),
            skip_ask,
            ask_policy: false,
            clock: None,
        }
    }

    /// Ask the Policy's gate (#958, `lamp-policy --gate`) on `skills/ask`
    /// and act only on its `say_now` (`LAMP_SKILLS_ASK=policy`).
    pub fn asking_the_policy(mut self) -> Self {
        self.ask_policy = true;
        self
    }

    /// The local time is known: at `now_us` on the engine's clock it is
    /// `unix_ms`, `utc_offset_minutes` east of UTC. Told again as often as
    /// wanted, so a wall clock set late (by NTP) is followed. Until then, a
    /// skill with quiet hours never starts itself.
    pub fn clock(&mut self, utc_offset_minutes: i32, now_us: u64, unix_ms: u64) {
        self.clock = Some((utc_offset_minutes, now_us, unix_ms));
    }

    /// [`Engine::clock`] in `local`, with its offset at `unix_ms` (#1384): a
    /// zone's changes with summer time, so quiet hours stay at the same clock
    /// hours across one, which a fixed offset can't do.
    pub fn clock_in(&mut self, local: &lamp_local_time::LocalTime, now_us: u64, unix_ms: u64) {
        self.clock(local.offset_minutes_at(unix_ms), now_us, unix_ms);
    }

    /// Time passes with no message: a run held until its trigger ends ends
    /// when the trigger topic's newest value has expired (its header's
    /// `expires_ms`) or, naming no lifespan, when the topic has said nothing
    /// for [`SILENT_ENDS_US`]. Its spell is over with it.
    pub fn tick(&mut self, now_us: u64) -> Vec<Run> {
        let mut runs = Vec::new();
        for skill in &self.skills {
            for (index, trigger) in skill.triggers.iter().enumerate() {
                let Some(watch) = self.watches.get_mut(&(skill.name.clone(), index)) else {
                    continue;
                };
                let Some((_, at_us, lifespan_us)) = self.latest.get(&trigger.topic) else {
                    continue;
                };
                if watch.open
                    && now_us.saturating_sub(*at_us) >= lifespan_us.unwrap_or(SILENT_ENDS_US)
                {
                    watch.open = false;
                    watch.since_us = None;
                    runs.push(Run {
                        skill: skill.name.clone(),
                        events: Vec::new(),
                        calls: vec![Call::Ends],
                        ask: None,
                        queue: false,
                        from: None,
                    });
                }
            }
        }
        runs
    }

    /// Minutes since local midnight at `now_us`, if the local time is known.
    fn local_minute(&self, now_us: u64) -> Option<u16> {
        let (offset, at_us, unix_ms) = self.clock?;
        let ms = i128::from(unix_ms) + (i128::from(now_us) - i128::from(at_us)) / 1_000;
        let minutes = ms.div_euclid(60_000) + i128::from(offset);
        Some(minutes.rem_euclid(1_440) as u16)
    }

    /// The skills loaded again. A timed condition keeps when it began to
    /// hold only if it is the same condition as before. A run held until its
    /// trigger ends ends now, as at a normal end, if its skill is gone, no
    /// longer holds anything until its trigger ends, or no longer has that
    /// trigger (the same place in the skill on the same topic): nothing else
    /// would ever end it (the reviewer on #1066). Those ends are returned.
    pub fn replace(&mut self, skills: Vec<Skill>) -> Vec<Run> {
        let old = std::mem::replace(&mut self.skills, skills);
        let mut ends = Vec::new();
        for ((name, index), watch) in &mut self.watches {
            if !watch.open {
                continue;
            }
            let topic = |skills: &[Skill]| {
                skills
                    .iter()
                    .find(|skill| &skill.name == name)
                    .filter(|skill| skill.holds_until_ends())
                    .and_then(|skill| skill.triggers.get(*index))
                    .map(|trigger| trigger.topic.clone())
            };
            let kept = topic(&self.skills);
            if kept.is_some() && kept == topic(&old) {
                continue;
            }
            watch.open = false;
            watch.since_us = None;
            ends.push(Run {
                skill: name.clone(),
                events: Vec::new(),
                calls: vec![Call::Ends],
                ask: None,
                queue: false,
                from: None,
            });
        }
        ends.sort_by(|a, b| a.skill.cmp(&b.skill));
        // A list's items begin again from the topic's value kept here: what
        // is there at the reload is not news, and the next message, which is
        // the next change (`tasks/status` is published when it changes, and
        // its subscriber is kept through a reload), is compared with it (the
        // reviewer on #1042). With no value kept yet, the topic is new to the
        // program, and its first message, the bus's kept value, only says
        // what is there, as at the start.
        for watch in self.watches.values_mut() {
            watch.items = None;
        }
        for skill in &self.skills {
            for (index, trigger) in skill.triggers.iter().enumerate() {
                if let (Some((list, key)), Some((value, _, _))) =
                    (&trigger.each, self.latest.get(&trigger.topic))
                {
                    self.watches
                        .entry((skill.name.clone(), index))
                        .or_default()
                        .items = Some(items_of(trigger, list, key, value));
                }
            }
        }
        let new = &self.skills;
        let condition = |skills: &[Skill], name: &str, index: usize| {
            skills
                .iter()
                .find(|skill| skill.name == name)
                .and_then(|skill| skill.conditions.get(index).cloned())
        };
        self.holding.retain(|(name, index), _| {
            let before = condition(&old, name, *index);
            before.is_some() && before == condition(new, name, *index)
        });
        ends
    }

    pub fn skills(&self) -> &[Skill] {
        &self.skills
    }

    /// The skill whose tool `name` is and what the model is told of a call
    /// while its run is under way, if such a call is refused: a run with no
    /// pose to hold again and no look to take again has nothing to do again
    /// (a dance plays on). Refused before [`Engine::tool`], it is no run:
    /// `[again]`'s while does not begin again from it.
    pub fn refused_under_way(&self, name: &str) -> Option<(&str, &str)> {
        self.skills
            .iter()
            .find(|skill| skill.tool.as_deref() == Some(name))
            .filter(|skill| {
                !skill
                    .steps
                    .iter()
                    .any(|(_, step)| matches!(step, Step::Call(Call::Mood { .. } | Call::Look)))
            })
            .map(|skill| (skill.name.as_str(), skill.still.as_str()))
    }

    /// Every topic a trigger or a condition names, the faces a look at a
    /// face reads, and what was heard if a skill has `[again]`, once each.
    pub fn topics(&self) -> Vec<String> {
        let mut topics: Vec<String> = self
            .skills
            .iter()
            .flat_map(|skill| {
                let looks = skill
                    .steps
                    .iter()
                    .any(|(_, step)| matches!(step, Step::Call(Call::Gaze { .. })));
                skill
                    .triggers
                    .iter()
                    .map(|trigger| trigger.topic.clone())
                    .chain(skill.conditions.iter().map(|test| test.topic.clone()))
                    .chain(looks.then(|| Faces::TOPIC.to_owned()))
                    .chain(skill.again.is_some().then(|| Speech::TOPICS[0].to_owned()))
            })
            .collect();
        topics.sort();
        topics.dedup();
        topics
    }

    /// The newest value of `topic`, if it still lasts at `now_us`.
    fn fresh<'a>(
        latest: &'a HashMap<String, (Value, u64, Option<u64>)>,
        topic: &str,
        now_us: u64,
    ) -> Option<&'a Value> {
        latest
            .get(topic)
            .filter(|(_, at_us, lasting_us)| {
                lasting_us.is_none_or(|lasting| now_us < at_us + lasting)
            })
            .map(|(value, _, _)| value)
    }

    /// The condition holds on its topic's newest value that still lasts, and
    /// has since `since_us` for its `for`.
    fn condition_holds(
        latest: &HashMap<String, (Value, u64, Option<u64>)>,
        condition: &Condition,
        since_us: Option<&u64>,
        now_us: u64,
    ) -> bool {
        Self::fresh(latest, &condition.topic, now_us)
            .is_some_and(|value| condition.test.holds(value))
            && since_us.is_some_and(|since| now_us - since >= condition.for_us)
    }

    /// Every `[[while]]` condition of `skill` holds, as `condition_holds`.
    fn conditions_hold(
        latest: &HashMap<String, (Value, u64, Option<u64>)>,
        holding: &HashMap<(String, usize), u64>,
        skill: &Skill,
        now_us: u64,
    ) -> bool {
        skill
            .conditions
            .iter()
            .enumerate()
            .all(|(index, condition)| {
                Self::condition_holds(
                    latest,
                    condition,
                    holding.get(&(skill.name.clone(), index)),
                    now_us,
                )
            })
    }

    /// A run asked of the Policy, when its answer comes: its calls made
    /// again at `now_us` (a look at where the face is now), if one of its
    /// triggers still holds on its topic's newest value that lasts and its
    /// conditions hold. `None`: the trigger is gone (nobody there any more).
    /// With the run's trigger known, only that one, and with `each` only its
    /// item, still in the list and holding; its steps and those for all.
    /// A skill that holds until its trigger ends also needs that trigger's
    /// end still to come: once a reload or the trigger's own end has closed
    /// its watch, nothing would ever give back what it held.
    pub fn still(&self, run: &Run, now_us: u64) -> Option<Vec<Call>> {
        let skill = self.skills.iter().find(|skill| skill.name == run.skill)?;
        if skill.holds_until_ends()
            && !run.from.as_ref().is_some_and(|(index, _)| {
                self.watches
                    .get(&(skill.name.clone(), *index))
                    .is_some_and(|watch| watch.open)
            })
        {
            return None;
        }
        let holds = |trigger: &Trigger, item: Option<&String>| {
            Self::fresh(&self.latest, &trigger.topic, now_us).is_some_and(|value| {
                match (&trigger.each, item) {
                    (Some((list, key)), Some(name)) => {
                        value[list].as_array().into_iter().flatten().any(|item| {
                            item_name(&item[key]).as_ref() == Some(name) && trigger.test.holds(item)
                        })
                    }
                    _ => trigger.test.holds(value),
                }
            })
        };
        let trigger = match &run.from {
            Some((index, item)) => {
                let trigger = skill.triggers.get(*index)?;
                holds(trigger, item.as_ref()).then_some(Some(trigger))?
            }
            None => skill
                .triggers
                .iter()
                .any(|trigger| holds(trigger, None))
                .then_some(None)?,
        };
        let run_item = run.from.as_ref().and_then(|(_, item)| item.as_deref());
        Self::conditions_hold(&self.latest, &self.holding, skill, now_us)
            .then(|| calls(&self.latest, skill, trigger, now_us, &Value::Null, run_item))
    }

    /// A message on `topic`, at `now_us`. A payload that is not JSON is
    /// passed over.
    pub fn observe(
        &mut self,
        topic: &str,
        now_us: u64,
        payload: &[u8],
        expires_ms: Option<u64>,
    ) -> Vec<Run> {
        let Ok(value) = serde_json::from_slice::<Value>(payload) else {
            return Vec::new();
        };
        // Words still coming count too: the model may call before the end.
        if topic == Speech::TOPICS[0]
            && let Some(text) = value["text"].as_str()
        {
            self.heard
                .piece(text, value["finished"].as_bool().unwrap_or(true), now_us);
        }
        let lifespan_us = expires_ms
            .or((topic == Faces::TOPIC).then_some(FACE_LIFESPAN_MS))
            .map(|ms| ms * 1_000);
        // A timed condition held only while its topic's value lasted: after a
        // value that expired, it begins again.
        let lasted = Self::fresh(&self.latest, topic, now_us).is_some();
        self.latest
            .insert(topic.to_owned(), (value.clone(), now_us, lifespan_us));
        for skill in &self.skills {
            for (index, condition) in skill.conditions.iter().enumerate() {
                if condition.topic != topic {
                    continue;
                }
                let key = (skill.name.clone(), index);
                if condition.test.holds(&value) && lasted {
                    self.holding.entry(key).or_insert(now_us);
                } else if condition.test.holds(&value) {
                    self.holding.insert(key, now_us);
                } else {
                    self.holding.remove(&key);
                }
            }
        }
        let mut runs = Vec::new();
        let minute = self.local_minute(now_us);
        // What a run held until its trigger stops holding is given back
        // when it does.
        let ends = |watch: &mut Watch, skill: &Skill, runs: &mut Vec<Run>| {
            if std::mem::take(&mut watch.open) {
                runs.push(Run {
                    skill: skill.name.clone(),
                    events: Vec::new(),
                    calls: vec![Call::Ends],
                    ask: None,
                    queue: false,
                    from: None,
                });
            }
        };
        for skill in &self.skills {
            // The runs of a list's items, from all the skill's triggers, go
            // out in the items' order: task 9 done and task 10 failed in one
            // message are 9's run, then 10's (the reviewer on #1042).
            let mut item_runs: Vec<(String, Run)> = Vec::new();
            for (index, trigger) in skill.triggers.iter().enumerate() {
                if trigger.topic != topic {
                    continue;
                }
                let watch = self.watches.entry((skill.name.clone(), index)).or_default();
                if let Some((list, key)) = &trigger.each {
                    // Each item on its own: one that holds where, in the
                    // trigger's last message, it did not, or was not in the
                    // list. The first message since the program started
                    // only says what is there: what was there then is not
                    // news. After a reload the value kept then is the last.
                    let items = items_of(trigger, list, key, &value);
                    let came: Vec<String> = match &watch.items {
                        None => Vec::new(),
                        Some(before) => items
                            .iter()
                            .filter(|(name, holds)| **holds && before.get(*name) != Some(&true))
                            .map(|(name, _)| name.clone())
                            .collect(),
                    };
                    watch.items = Some(items);
                    for name in came {
                        if !Self::conditions_hold(&self.latest, &self.holding, skill, now_us) {
                            continue;
                        }
                        let held = format!("{key} {name} of {list} on {topic} came to hold");
                        let run = decide(
                            skill,
                            held,
                            (self.skip_ask, self.ask_policy),
                            question(skill),
                            || {
                                calls(
                                    &self.latest,
                                    skill,
                                    Some(trigger),
                                    now_us,
                                    &Value::Null,
                                    Some(&name),
                                )
                            },
                        );
                        item_runs.push((
                            name.clone(),
                            Run {
                                queue: true,
                                from: Some((index, Some(name))),
                                ..run
                            },
                        ));
                    }
                    continue;
                }
                let started = *watch.started_us.get_or_insert(now_us);
                if let Some(line) = &trigger.rearm_line {
                    if !line.test.holds(&value) {
                        watch.past_line_us = None;
                    } else if now_us - *watch.past_line_us.get_or_insert(now_us) >= line.for_us {
                        watch.disarmed = false;
                    }
                }
                if !trigger.test.holds(&value) {
                    if watch
                        .last_held_us
                        .is_none_or(|held| now_us - held >= trigger.gap_us)
                    {
                        watch.since_us = None;
                        ends(watch, skill, &mut runs);
                    }
                    continue;
                }
                let tolerated = lifespan_us.unwrap_or(u64::MAX).max(trigger.gap_us);
                if watch.since_us.is_none()
                    || watch
                        .last_held_us
                        .is_some_and(|held| now_us - held >= tolerated)
                {
                    ends(watch, skill, &mut runs);
                    watch.since_us = Some(now_us);
                    watch.away_us = now_us - watch.last_held_us.unwrap_or(started);
                    watch.spent = false;
                    watch.waiting_us = None;
                }
                watch.last_held_us = Some(now_us);
                let held_us = now_us - watch.since_us.unwrap_or(now_us);
                if held_us < trigger.for_us {
                    continue;
                }
                // Once each time it holds: a chance held back now waits for
                // the next time.
                let once = trigger.when == When::Appears || trigger.rearm;
                if (once && watch.spent) || watch.disarmed {
                    continue;
                }
                watch.spent = once;
                if trigger.rearm_line.is_some() {
                    watch.disarmed = true;
                    watch.past_line_us = None;
                }
                if (trigger.when == When::Appears && watch.away_us < trigger.absent_for_us)
                    || watch
                        .ran_us
                        .is_some_and(|ran| now_us - ran < trigger.cooldown_us)
                {
                    continue;
                }
                if skill
                    .quiet
                    .is_some_and(|quiet| minute.is_none_or(|minute| within(minute, quiet)))
                {
                    // In the quiet hours the chance is kept, as `wait` keeps
                    // one through a talk: taken at the trigger's first message
                    // after they end, while it still holds; the trigger
                    // stopping (past its `gap`) spends it. A `wait` for a
                    // condition begins when they end. An `appears` chance is
                    // spent: someone who came at 22:30 is not greeted at
                    // 08:00.
                    if trigger.when != When::Appears {
                        watch.spent = false;
                        watch.disarmed = false;
                        watch.waiting_us = None;
                    }
                    continue;
                }
                if !Self::conditions_hold(&self.latest, &self.holding, skill, now_us) {
                    // Held back by a condition (someone talking): with a
                    // `wait`, the chance is kept while the trigger holds,
                    // and taken at its next message once they hold.
                    // A chance kept is not yet had: the rearm line waits too.
                    let waited = now_us - *watch.waiting_us.get_or_insert(now_us);
                    if waited < trigger.wait_us {
                        watch.spent = false;
                        watch.disarmed = false;
                    }
                    continue;
                }
                watch.waiting_us = None;
                let held = match trigger.when {
                    When::Appears => format!(
                        "it appeared on {topic} after {} s without",
                        watch.away_us / 1_000_000
                    ),
                    When::Present => format!("{topic} has held for {} s", held_us / 1_000_000),
                };
                let mut question = question(skill);
                if question["act"] == "hello" {
                    question["away_min"] = json!(watch.away_us / 60_000_000);
                    question["there_s"] = json!(held_us / 1_000_000);
                }
                if self.skip_ask || self.ask_policy {
                    watch.ran_us = Some(now_us);
                    // Its end is told even if the Policy says no: with no
                    // run holding anything, the program lets it pass.
                    watch.open = skill.holds_until_ends();
                }
                let run = decide(
                    skill,
                    held,
                    (self.skip_ask, self.ask_policy),
                    question,
                    || {
                        calls(
                            &self.latest,
                            skill,
                            Some(trigger),
                            now_us,
                            &Value::Null,
                            None,
                        )
                    },
                );
                runs.push(Run {
                    from: Some((index, None)),
                    ..run
                });
            }
            item_runs.sort_by(|(a, _), (b, _)| item_order(a).cmp(&item_order(b)));
            runs.extend(item_runs.into_iter().map(|(_, run)| run));
        }
        runs
    }

    /// The model called `name` with `args`: the first skill whose tool it is
    /// runs its steps. A person asked, so the Policy is not asked.
    /// The arguments must be the tool's (`Null`: none given), and its
    /// `[[while]]` conditions must hold as for a trigger: on a value that
    /// still lasts, for their `for`. `Err` says why not, in words the lamp
    /// can say, or, for arguments that are not the tool's, which are.
    /// `[again]` refuses it within its `within` of the tool's last run, from
    /// when that run began, with none of its entries among the whole words
    /// of the newest thing heard (its pieces joined, as the voice joins
    /// them) if it began after that run began; the model reads its `told`.
    pub fn tool(&mut self, name: &str, args: &Value, now_us: u64) -> Result<Run, String> {
        let skill = self
            .skills
            .iter()
            .find(|skill| skill.tool.as_deref() == Some(name))
            .ok_or("No skill of mine does that just now.")?;
        arguments(skill, args)?;
        if !Self::conditions_hold(&self.latest, &self.holding, skill, now_us) {
            return Err("I can't do that just now.".into());
        }
        let ran = self.tool_ran.get(&skill.name).copied();
        if let (Some(again), Some(ran)) = (&skill.again, ran)
            && now_us.saturating_sub(ran) < again.within_us
        {
            let asked = self.heard.began_us.is_some_and(|began_us| began_us > ran) && {
                let heard = words(&self.heard.text);
                again.if_heard.iter().any(|entry| {
                    heard
                        .windows(entry.len())
                        .any(|run| run == entry.as_slice())
                })
            };
            if !asked {
                return Err(again.told.clone());
            }
        }
        self.tool_ran.insert(skill.name.clone(), now_us);
        Ok(Run {
            skill: skill.name.clone(),
            events: vec![event(
                &skill.name,
                SkillStage::Triggered,
                format!("the model called {name} with {args}"),
                None,
            )],
            calls: calls(&self.latest, skill, None, now_us, args, None),
            ask: None,
            queue: false,
            from: None,
        })
    }
}

/// What was heard of the newest thing said, joined from its pieces on
/// `voice/heard` as `crates/voice/src/thread.rs` joins them.
#[derive(Debug, Default)]
struct Heard {
    text: String,
    /// When its first piece came: the utterance counts for `[again]` only
    /// if it began after the last run began, so the ask's own words (and a
    /// noise joined to them) never unlock the next call.
    began_us: Option<u64>,
    /// When its newest piece came.
    at_us: Option<u64>,
    /// Its newest piece was the last: the next begins another.
    finished: bool,
}

impl Heard {
    /// A piece: added to what was heard, which begins again after a piece
    /// that finished it, or after one that waited more than 20 s for more.
    fn piece(&mut self, text: &str, finished: bool, now_us: u64) {
        let stale = self
            .at_us
            .is_some_and(|at| now_us.saturating_sub(at) > HEARD_WITHIN_US);
        if self.finished || stale || self.began_us.is_none() {
            self.text.clear();
            self.finished = false;
            self.began_us = Some(now_us);
        }
        add_heard(&mut self.text, text, HEARD_BYTES);
        self.at_us = Some(now_us);
        self.finished = finished;
    }
}

/// One tool call's admission, shared by the socket that waits for it and
/// the loop that runs it: whichever comes first, the run committed or the
/// caller told it timed out, wins, and the other then cannot happen.
#[derive(Clone, Default)]
pub struct Admission(std::sync::Arc<std::sync::atomic::AtomicU8>);

impl Admission {
    const PENDING: u8 = 0;
    const COMMITTED: u8 = 1;
    const CANCELLED: u8 = 2;

    fn settle(&self, to: u8) -> bool {
        use std::sync::atomic::Ordering;
        self.0
            .compare_exchange(Self::PENDING, to, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// The loop is about to answer and run it: false if the caller gave up.
    pub fn commit(&self) -> bool {
        self.settle(Self::COMMITTED)
    }

    /// The caller gives up: false if the loop committed first, whose answer
    /// is then on its way.
    pub fn cancel(&self) -> bool {
        self.settle(Self::CANCELLED)
    }
}

/// What the Policy is asked of a skill the lamp starts itself: its act, a
/// hello, news of work, a reminder the person set, or anything else.
fn question(skill: &Skill) -> Value {
    let act = match skill.policy.as_str() {
        "hello" | "news" | "reminder" => skill.policy.as_str(),
        _ => "unprompted",
    };
    json!({ "skill": skill.name, "act": act })
}

/// A trigger held: with `(skip_ask, ask_policy)`, the Policy is asked
/// `question` and the calls wait for its answer, or the ask is skipped and
/// they are made, or neither, and nothing is done. The calls are made only
/// when the run may go ahead.
fn decide(
    skill: &Skill,
    held: String,
    (skip_ask, ask_policy): (bool, bool),
    question: Value,
    made: impl FnOnce() -> Vec<Call>,
) -> Run {
    let mut events = vec![
        event(&skill.name, SkillStage::Triggered, held, None),
        event(
            &skill.name,
            SkillStage::Asked,
            format!("policy {}", skill.policy),
            None,
        ),
    ];
    let run = |events, calls, ask| Run {
        skill: skill.name.clone(),
        events,
        calls,
        ask,
        queue: false,
        from: None,
    };
    if ask_policy {
        return run(events, made(), Some(question));
    }
    if !skip_ask {
        events.push(event(
            &skill.name,
            SkillStage::Answered,
            "LAMP_SKILLS_ASK is not set: nothing the lamp starts itself is done \
             (LAMP_SKILLS_ASK=policy asks the Policy's gate, skip acts without asking)"
                .into(),
            Some(false),
        ));
        return run(events, Vec::new(), None);
    }
    events.push(event(
        &skill.name,
        SkillStage::Answered,
        "the ask is skipped (LAMP_SKILLS_ASK=skip): LAMP_SKILLS_ASK=policy asks the Policy's gate"
            .into(),
        Some(true),
    ));
    run(events, made(), None)
}

/// Whether `args` are what the skill's tool takes: its choice, one of its
/// values, and its flag, true or false, each optional; nothing else.
fn arguments(skill: &Skill, args: &Value) -> Result<(), String> {
    let given = match args {
        Value::Null => return Ok(()),
        Value::Object(given) => given,
        _ => return Err("the arguments are an object".into()),
    };
    for (key, value) in given {
        match (&skill.choice, &skill.flag) {
            (Some(choice), _) if *key == choice.name => {
                if !value
                    .as_str()
                    .is_some_and(|value| choice.one_of.iter().any(|one| one == value))
                {
                    return Err(format!("{key} is one of {:?}", choice.one_of));
                }
            }
            (_, Some(flag)) if *key == flag.name => {
                if !value.is_boolean() {
                    return Err(format!("{key} is true or false"));
                }
            }
            _ => return Err(format!("{key} is not an argument of this tool")),
        }
    }
    Ok(())
}

/// One of `count`, picked from the time: the same time picks the same, so a
/// replay is the same each time.
fn pick(now_us: u64, count: usize) -> usize {
    ((now_us.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 32) % count as u64) as usize
}

/// A skill's steps as calls now. The face to look at: the largest in the
/// newest list of faces that still lasts. With none, the other steps go on.
/// Only the steps for every trigger and those for `trigger`.
fn calls(
    latest: &HashMap<String, (Value, u64, Option<u64>)>,
    skill: &Skill,
    trigger: Option<&Trigger>,
    now_us: u64,
    args: &Value,
    item: Option<&str>,
) -> Vec<Call> {
    let fired = trigger.and_then(|trigger| trigger.name.as_ref());
    let face = Engine::fresh(latest, Faces::TOPIC, now_us).and_then(|faces| {
        serde_json::from_value::<Faces>(faces.clone())
            .ok()?
            .faces
            .into_iter()
            .max_by(|a, b| (a.ymax - a.ymin).total_cmp(&(b.ymax - b.ymin)))
            .map(|face| {
                (
                    face.xmin + face.xmax - 1.0,
                    face.ymin + face.ymax - 1.0,
                    face.ymax - face.ymin,
                )
            })
    });
    let chosen = skill
        .choice
        .as_ref()
        .and_then(|choice| args[&choice.name].as_str());
    let twice = skill
        .flag
        .as_ref()
        .is_some_and(|flag| args[&flag.name] == true);
    skill
        .steps
        .iter()
        .filter(|(on, _)| on.is_none() || on.as_ref() == fired)
        .flat_map(|(_, step)| match step {
            Step::Call(Call::Gaze { .. }) => face
                .map(|face| Call::Gaze { face: Some(face) })
                .into_iter()
                .collect(),
            // An item's run names its task to the voice, by a numeric key,
            // and only on a `from = "tasks"` ask: the voice refuses `task`
            // on any other (#1237).
            Step::Call(Call::SayFrom(source, _)) => {
                let task = (*source == Source::Tasks)
                    .then(|| item.and_then(|name| name.parse().ok()))
                    .flatten();
                vec![Call::SayFrom(*source, task)]
            }
            Step::Call(other) => vec![other.clone()],
            Step::Gesture { by, again } => {
                let names: Vec<&String> =
                    match by.iter().find(|(value, _)| Some(value.as_str()) == chosen) {
                        Some((_, names)) => names.iter().collect(),
                        None => by.iter().flat_map(|(_, names)| names).collect(),
                    };
                let name = names[pick(now_us, names.len())].clone();
                vec![Call::Gesture(name); if *again && twice { 2 } else { 1 }]
            }
        })
        .collect()
}

/// The tools file the voice reads beside the actions part's `tools.json`:
/// each skill's tool, described with the skill's description. A name an
/// earlier skill took is left out, as [`Engine::tool`] runs the first.
pub fn tools_file(skills: &[Skill]) -> Value {
    let mut tools: Vec<Value> = Vec::new();
    for skill in skills {
        if let Some(name) = &skill.tool
            && !tools.iter().any(|tool| tool["name"] == *name)
        {
            let mut tool = json!({"name": name, "description": skill.description});
            let mut properties = serde_json::Map::new();
            if let Some(choice) = &skill.choice {
                properties.insert(
                    choice.name.clone(),
                    json!({"type": "string", "enum": choice.one_of, "description": choice.about}),
                );
            }
            if let Some(flag) = &skill.flag {
                properties.insert(
                    flag.name.clone(),
                    json!({"type": "boolean", "description": flag.about}),
                );
            }
            if !properties.is_empty() {
                tool["parameters"] = json!({"type": "object", "properties": properties});
            }
            tools.push(tool);
        }
    }
    json!({ "tools": tools })
}

/// What the model is told when its call ran these calls. With a look, to
/// say only that it will look, and `"look": true`, by which the voice takes
/// its full look itself, whatever the model says (#941): the still comes in
/// the model's next turn. With a say, the note, for its answer.
pub fn told(calls: &[Call]) -> Value {
    let note = calls.iter().find_map(|call| match call {
        Call::Say(note) => Some(note.as_str()),
        _ => None,
    });
    const LOOK: &str = "Say only \"Let me take a look.\" now, in those words.";
    let next = match (calls.contains(&Call::Look), note) {
        (true, Some(note)) => format!("{LOOK} When the picture comes: {note}"),
        (true, None) => format!("{LOOK} The picture comes after it."),
        (false, Some(note)) => note.to_owned(),
        (false, None) => "Done.".to_owned(),
    };
    if calls.contains(&Call::Look) {
        json!({ "next": next, "look": true })
    } else {
        json!({ "next": next })
    }
}

/// One message of a recording made by `lamp-bus record`.
pub struct Recorded {
    pub header: Header,
    pub payload: Vec<u8>,
}

#[derive(Deserialize)]
struct Line {
    header: Option<Header>,
    payload_b64: Option<String>,
}

/// Whether a file or folder is hidden: its name begins with a dot. A folder
/// copied from a Mac carries `._NAME` AppleDouble files beside each file,
/// binary and named like it (lamp-4ace, 2026-10-16); they are not the
/// skill's.
pub fn hidden(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name.to_string_lossy().starts_with('.'))
}

/// Every message in the `.jsonl` files of `dir`, in the order of their
/// `at_us`. Declaration lines and hidden files are passed over.
pub fn read_recordings(dir: &Path) -> Result<Vec<Recorded>, String> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|error| format!("{}: {error}", dir.display()))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            !hidden(path)
                && path
                    .extension()
                    .is_some_and(|extension| extension == "jsonl")
        })
        .collect();
    files.sort();
    let mut messages = Vec::new();
    for file in files {
        let text =
            fs::read_to_string(&file).map_err(|error| format!("{}: {error}", file.display()))?;
        for (number, line) in text.lines().enumerate() {
            let wrong = |why: &str| format!("{} line {}: {why}", file.display(), number + 1);
            let line: Line =
                serde_json::from_str(line).map_err(|error| wrong(&error.to_string()))?;
            let (Some(header), Some(payload)) = (line.header, line.payload_b64) else {
                continue;
            };
            let payload = unbase64(&payload).ok_or_else(|| wrong("payload_b64 is not base64"))?;
            messages.push(Recorded { header, payload });
        }
    }
    messages.sort_by_key(|message| message.header.at_us);
    Ok(messages)
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Padded base64, as `lamp-bus record` writes it (its own copy is private to
/// its program).
fn unbase64(text: &str) -> Option<Vec<u8>> {
    let text = text.as_bytes();
    if !text.len().is_multiple_of(4) {
        return None;
    }
    let mut bytes = Vec::with_capacity(text.len() / 4 * 3);
    for (index, chunk) in text.chunks(4).enumerate() {
        let padding = chunk.iter().rev().take_while(|&&byte| byte == b'=').count();
        if padding > 2 || (padding > 0 && (index + 1) * 4 != text.len()) {
            return None;
        }
        let mut group = 0u32;
        for &byte in &chunk[..4 - padding] {
            group = group << 6 | ALPHABET.iter().position(|&letter| letter == byte)? as u32;
        }
        group <<= 6 * padding;
        bytes.extend_from_slice(&group.to_be_bytes()[1..4 - padding]);
    }
    Some(bytes)
}

/// A call made in a replay: when, from the recording's first message.
#[derive(Debug)]
pub struct Made {
    pub at_us: u64,
    pub call: Call,
}

/// A call of a skill's tool in a case, as the model makes it: `at` from
/// the recording's first message, with the arguments the model gives.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCall {
    pub name: String,
    pub at: String,
    /// The tool's arguments, its `[choice]` and its `[flag]`; none without.
    #[serde(default)]
    pub args: toml::Table,
}

/// The calls the skill in `dir` makes for one recording, with the ask skipped.
pub fn replay(dir: &Path, recording: &Path) -> Result<Vec<Made>, String> {
    replay_with(dir, recording, &[], None)
}

/// The same, with the model calling the skill's tool at the times of
/// `tools`: each after the messages recorded at or before it. A call is
/// made as the program would make it, its arguments and its conditions
/// checked on what has been replayed; one refused makes nothing. Each call
/// is a fresh run: what the program does with a call while the last run is
/// still under way (#1036, #1051) is not replayed. The marker that the
/// calls after it wait for the say is not a call made. With `clock`
/// (`"23:00"`), the case's first message is at that local time; without
/// it, the time of day is not known.
pub fn replay_with(
    dir: &Path,
    recording: &Path,
    tools: &[ToolCall],
    clock: Option<&str>,
) -> Result<Vec<Made>, String> {
    let skill = load(dir)?;
    // A name the skill's tool is not would make nothing and pass as a
    // quiet case: it is refused.
    if let Some(tool) = tools
        .iter()
        .find(|tool| skill.tool.as_deref() != Some(tool.name.as_str()))
    {
        return Err(format!(
            "call_tool {:?}: this skill's tool is {:?}",
            tool.name,
            skill.tool.as_deref().unwrap_or("none")
        ));
    }
    let mut engine = Engine::new(vec![skill], true);
    let messages = read_recordings(recording)?;
    let first_us = messages.first().map_or(0, |message| message.header.at_us);
    if let Some(clock) = clock {
        engine.clock(0, first_us, u64::from(minute_of_day(clock)?) * 60_000);
    }
    let mut calls: Vec<(u64, &str, Value)> = tools
        .iter()
        .map(|tool| {
            let args = if tool.args.is_empty() {
                Value::Null
            } else {
                serde_json::to_value(&tool.args).map_err(|error| error.to_string())?
            };
            Ok((duration_us(&tool.at)?, tool.name.as_str(), args))
        })
        .collect::<Result<_, String>>()?;
    calls.sort_by_key(|(at, _, _)| *at);
    let mut calls = calls.into_iter().peekable();
    let mut made = Vec::new();
    let keep = |made: &mut Vec<Made>, calls: Vec<Call>, at_us: u64| {
        made.extend(
            calls
                .into_iter()
                .filter(|call| *call != Call::AfterSay)
                .map(|call| Made { at_us, call }),
        );
    };
    let mut call_tools = |engine: &mut Engine, made: &mut Vec<Made>, until_us: Option<u64>| {
        while let Some((at, name, args)) =
            calls.next_if(|(at, _, _)| until_us.is_none_or(|until| *at < until))
        {
            if let Ok(run) = engine.tool(name, &args, first_us + at) {
                keep(made, run.calls, at);
            }
        }
    };
    for message in messages {
        let at_us = message.header.at_us;
        call_tools(&mut engine, &mut made, Some(at_us - first_us));
        // Time passes as the messages come: a held run whose trigger topic
        // went silent ends at the first message of any topic after.
        for run in engine.tick(at_us) {
            keep(&mut made, run.calls, at_us - first_us);
        }
        for run in engine.observe(
            &message.header.topic,
            at_us,
            &message.payload,
            message.header.expires_ms,
        ) {
            keep(&mut made, run.calls, at_us - first_us);
        }
    }
    call_tools(&mut engine, &mut made, None);
    Ok(made)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectFile {
    /// The local time of the case's first message (`"10:00"`).
    clock: Option<String>,
    #[serde(default)]
    none: bool,
    #[serde(default)]
    expect: Vec<ExpectedCall>,
    /// The model calling the skill's tool, for a skill a person starts.
    #[serde(default)]
    call_tool: Vec<ToolCall>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectedCall {
    at: Option<String>,
    within: Option<String>,
    call: String,
    #[serde(default)]
    args: toml::Table,
}

/// Whether `made` is what one `[[expect]]` says: its name, every argument
/// it names, and, with `at`, when within `within`.
fn matches(expected: &ExpectedCall, made: &Made) -> Result<bool, String> {
    let (name, args) = made.call.named();
    if name != expected.call {
        return Ok(false);
    }
    for (key, wanted) in &expected.args {
        let wanted = serde_json::to_value(wanted).map_err(|error| error.to_string())?;
        let got = &args[key];
        let same = |wanted: &Value| match (got.as_f64(), wanted.as_f64()) {
            (Some(got), Some(wanted)) => got == wanted,
            _ => got == wanted,
        };
        // A list: any one of its values.
        let any = match &wanted {
            Value::Array(values) => values.iter().any(same),
            one => same(one),
        };
        if !any {
            return Ok(false);
        }
    }
    if let Some(at) = &expected.at {
        let at = duration_us(at)?;
        let within = expected.within.as_deref().map_or(Ok(0), duration_us)?;
        if made.at_us.abs_diff(at) > within {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Each case in the skill's `tests/`: a folder of recordings and
/// `expect.toml`, which lists every call the case may make, in order, or
/// says `none = true`. How many passed and how many have no recording yet,
/// or the first that did not pass.
pub fn run_tests(dir: &Path) -> Result<(usize, usize), String> {
    load(dir)?;
    let mut cases: Vec<PathBuf> = fs::read_dir(dir.join("tests"))
        .map_err(|error| format!("tests/: {error}"))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && !hidden(path))
        .collect();
    cases.sort();
    let (mut passed, mut unrecorded) = (0, 0);
    for case in &cases {
        let wrong = |why: String| format!("{}: {why}", case.display());
        let expect: ExpectFile = toml::from_str(
            &fs::read_to_string(case.join("expect.toml"))
                .map_err(|error| wrong(format!("expect.toml: {error}")))?,
        )
        .map_err(|error| wrong(format!("expect.toml: {error}")))?;
        if expect.none != expect.expect.is_empty() {
            return Err(wrong(
                "expect.toml says none = true or lists [[expect]], one of them".into(),
            ));
        }
        // A case of a tool's call alone needs no recording.
        if read_recordings(case)?.is_empty() && expect.call_tool.is_empty() {
            unrecorded += 1;
            continue;
        }
        let made = replay_with(dir, case, &expect.call_tool, expect.clock.as_deref())?;
        let names: Vec<_> = made
            .iter()
            .map(|made| (made.at_us, made.call.named()))
            .collect();
        if made.len() != expect.expect.len() {
            return Err(wrong(format!(
                "made {} calls, {} expected: {names:?}",
                made.len(),
                expect.expect.len()
            )));
        }
        for (index, (expected, made)) in expect.expect.iter().zip(&made).enumerate() {
            if !matches(expected, made).map_err(wrong)? {
                return Err(wrong(format!(
                    "call {} is not {:?} {:?}: {names:?}",
                    index + 1,
                    expected.call,
                    expected.args
                )));
            }
        }
        passed += 1;
    }
    Ok((passed, unrecorded))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #940: with the switch, a gesture or a move says who asks; without
    /// it, the datagram is as before.
    #[test]
    fn a_body_ask_says_who_asks_only_with_the_switch() {
        let parse = |text: String| serde_json::from_str::<Value>(&text).unwrap();
        assert_eq!(
            body_ask(json!({ "gesture": "nod" }), None),
            r#"{"gesture":"nod"}"#
        );
        assert_eq!(
            parse(body_ask(json!({ "gesture": "nod" }), Some("dance"))),
            json!({"gesture": "nod", "source": "skill:dance", "priority": "skill"})
        );
        let clip = json!({"name": "wave", "step_ms": 40, "frames": []});
        assert_eq!(
            body_ask(json!({ "move": clip }), None),
            json!({ "move": clip }).to_string()
        );
        assert_eq!(
            parse(body_ask(json!({ "move": clip }), Some("dance"))),
            json!({"move": clip, "source": "skill:dance", "priority": "skill"})
        );
    }

    /// #939, (c): `say = { from = ... }` is a say of what the voice keeps,
    /// by where, at that kind's priority.
    #[test]
    fn a_say_from_names_where_the_voice_keeps_the_words() {
        let dir = tempfile::tempdir().unwrap();
        for (from, source, priority) in [
            ("schedule", Source::Schedule, 8),
            ("tasks", Source::Tasks, 5),
        ] {
            let skill = dir.path().join(from);
            std::fs::create_dir_all(&skill).unwrap();
            std::fs::write(
                skill.join("SKILL.md"),
                format!("---\nname: {from}\ndescription: A test.\n---\n"),
            )
            .unwrap();
            std::fs::write(
                skill.join("skill.toml"),
                format!(
                    "[trigger]\ntopic = \"schedule/due\"\nfield = \"pending\"\nabove = 0.5\n\
                     [ask]\npolicy = \"moment\"\n[[do]]\nsay = {{ from = \"{from}\" }}\n"
                ),
            )
            .unwrap();
            let loaded = load(&skill).unwrap();
            assert_eq!(
                loaded.steps,
                [(None, Step::Call(Call::SayFrom(source, None)))]
            );
            let (_, Step::Call(call)) = &loaded.steps[0] else {
                panic!("{:?}", loaded.steps)
            };
            assert_eq!(call.named(), ("say", json!({ "from": from })));
            assert_eq!((source.name(), source.priority()), (from, priority));
        }
    }
}
