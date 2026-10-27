//! Physical skills (`docs/skills.md`). A skill is a folder; this crate loads
//! one, decides from what the bus carries when it runs, and says which calls
//! its steps make. Sending them is the program's (`main.rs`).
//!
//! Time here is microseconds of one monotonic clock, given by the caller:
//! the program's own clock when a message arrives, a message's `at_us` when
//! a recording is replayed.
use lamp_bus::Header;
use lamp_msgs::{
    Activity, Faces, Light, LightPattern, LightRequest, NameHeard, PatternKind, RoomActivity,
    RoomPresence, SkillEvent, SkillStage, Speech, StoodUp, TouchEvent, Voice, VoiceState,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

pub mod addon;
pub mod clock;
pub mod daily;
pub mod glow;
pub mod known;
pub mod limits;
pub mod package;
pub mod requested_audio;
pub mod routine;
pub mod unplug;
pub mod visits;
pub mod watch;

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

/// `LAMP_ADDONS=on`: skills of kind `addon` run and write their proposals
/// on `addon/proposal` (`docs/addons.md`). Off unless set: without it they
/// are not loaded, and nothing new is published.
pub const ADDONS_ENVIRONMENT: &str = "LAMP_ADDONS";

/// `LAMP_REST_THROUGH_SOUNDS=on`: a `[[while]]` with `through` carries its
/// clock through those values, and only its `broken_by` begins it again
/// (#1672: Night sleep sleeps through a sound heard as no words). Off unless
/// set: both are read, checked and ignored, the clock begun again as before.
pub const REST_THROUGH_SOUNDS_ENVIRONMENT: &str = "LAMP_REST_THROUGH_SOUNDS";

/// The formats of `skill.toml` this build reads: 1 (no `format` key, or
/// `format = 1`) and 2 (`format = 2`, #3822's gap 7). In format 2 `look` is
/// presence alone and `camera` the only picture; any other is refused by
/// name, never guessed at.
pub const FORMATS: [u32; 2] = [1, 2];

/// What format 2's `look` gives without `topics` naming them: who is there,
/// how near and which way they face, and someone standing up to leave (the
/// protocol's `person.*` events, docs/control.md). The same two a pushed
/// skill's `look` reads (`dev::PUSHED_TOPICS`, #3879), so presence has one
/// definition.
pub const PRESENCE_TOPICS: [&str; 2] = ["humans/faces/tracked", "room/stood_up"];

/// `LAMP_YAWN_SOONER=on`: a trigger's `sooner` shortens its `for` while its
/// field is past its line (#3709: a yawn seen in the sitting brings Stretch
/// break forward). Off unless set: `sooner` is read, checked and ignored.
pub const SOONER_ENVIRONMENT: &str = "LAMP_YAWN_SOONER";

/// The topics a `[[while]]`'s `broken_by` may name, each with its rule
/// ([`breaks`]): words heard, the lamp's name heard, a new push on the
/// lamp, and a touch.
pub const BROKEN_BY: [&str; 4] = ["voice/heard", "name/heard", "body/hand", "controls/touch"];

/// Why a skill reads a `broken_by` topic ([`Skill::listens`]).
const BROKEN_BY_READ: &str = "a [[while]]'s broken_by";

/// Whether `value` on `topic` begins again a clock carried `through`: what
/// was heard, only with a letter or a digit in it (a sound heard as no words
/// does not); the lamp's name, any hearing of it, written down or not (El's
/// and reviewer-two's P2 on #3092: "Hey Lamp" from out of view may come with
/// no words); a hand, only a new push, pushing where `before` (the topic's
/// last value that still lasts) was not or there was none (#1672's lamp row
/// on lamp-06c1: a push held or reported steadily restarted the clock at
/// every message); a touch, any one the controls say is on (reviewer-two on
/// #3092: a tap on an awake lamp).
fn breaks(topic: &str, value: &Value, before: Option<&Value>) -> bool {
    match topic {
        "voice/heard" => value["text"]
            .as_str()
            .is_some_and(|text| text.chars().any(char::is_alphanumeric)),
        "name/heard" => serde_json::from_value::<NameHeard>(value.clone())
            .is_ok_and(|name| name.check().is_ok()),
        "body/hand" => {
            let pushed = |value: &Value| value["pushed"].as_bool() == Some(true);
            pushed(value) && !before.is_some_and(pushed)
        }
        "controls/touch" => serde_json::from_value::<TouchEvent>(value.clone())
            .is_ok_and(|touch| touch.enabled && touch.check().is_ok()),
        _ => false,
    }
}

/// The engine's local clock value, produced internally rather than by a
/// bus publisher. Clock triggers still pass the usual conditions and gate.
pub const LOCAL_CLOCK_TOPIC: &str = "clock/local";

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
pub(crate) fn words(text: &str) -> Vec<String> {
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
/// A light show's frames (#1679): 2 to 30 of them, each shown 2 to 20 s,
/// the whole show a minute at most. Slow enough to read as a light that
/// changes, never a flicker: a lamp is not a machine.
pub const SHOW_FRAMES: std::ops::RangeInclusive<usize> = 2..=30;
pub const SHOW_FRAME_US: std::ops::RangeInclusive<u64> = 2_000_000..=20_000_000;
pub const SHOW_AT_MOST_US: u64 = 60_000_000;
/// The longest a held-back chance may wait for its conditions.
const MOST_WAIT_US: u64 = 3_600_000_000;
/// How long a face message counts when its header names no lifespan: the
/// eyes' two seconds. A value on another topic counts until it is replaced.
pub(crate) const FACE_LIFESPAN_MS: u64 = 2_000;
const ROUTINE_VOICE_FRESH_US: u64 = 3_000_000;
/// How long a held run's trigger topic may say nothing, when its newest
/// value names no lifespan, before the run ends (#1021: until the air part
/// says its readings expire).
pub const SILENT_ENDS_US: u64 = 15 * 60 * 1_000_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SkillFile {
    /// The rules' format ([`FORMATS`]): left out, format 1.
    format: Option<u32>,
    /// Installed core skills can remain disabled until their lamp check.
    #[serde(default = "enabled_by_default")]
    enabled: bool,
    #[serde(default)]
    needs: Vec<String>,
    /// `core` (comes with the lamp and is never removed) or `installable`
    /// (put on and taken off by its owner); `installable` without it.
    kind: Option<String>,
    /// The skill's own version, `MAJOR.MINOR.PATCH`: what a package says.
    version: Option<String>,
    /// What the skill may read and use; nothing without it.
    #[serde(default)]
    permissions: Permissions,
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
    /// What a skill granted `prepare_unplug` says at each outcome
    /// ([`UnplugLines`]).
    unplug: Option<UnplugLines>,
    /// What the room's presence was over the last hours, for the model
    /// (#1685, "was anyone here while I was out?").
    history: Option<HistoryFile>,
    /// What the voice says of the news a `say = { from = "tasks" }` asks
    /// for, in the skill's own words (`[news.words]`, #940): the voice
    /// reads them from the installed file; here they are checked.
    news: Option<lamp_voice::skill_words::Section<lamp_voice::skill_words::NewsWords>>,
    /// The same for a `say = { from = "schedule" }`, a due reminder
    /// (`[reminder.words]`, #940).
    reminder: Option<lamp_voice::skill_words::Section<lamp_voice::skill_words::ReminderWords>>,
    /// The core onboarding runner's order and words, checked by the same
    /// validator Voice uses when it takes its service-start snapshot.
    onboarding: Option<lamp_voice::onboarding::skill::Definition>,
    /// The startle's move (#2822): Expression reads it from the installed
    /// file and plays it as its reflex; here it is checked by Expression's
    /// own check, and the skill is never run.
    startle: Option<lamp_expression::startle::Startle>,
    /// Local hours when the skill never starts itself.
    quiet: Option<QuietFile>,
    /// The most runs it starts itself a local day (#1685's nudges).
    most_a_day: Option<u32>,
    /// Local hours when alone it may start itself: `[quiet]`'s opposite.
    active: Option<QuietFile>,
    /// A budget of runs an hour, shared by every skill of its group.
    budget: Option<BudgetFile>,
    /// A rest until the next morning after runs nobody noticed.
    rest: Option<RestFile>,
    /// A search a person asked for (#1685): its looks, and what the model
    /// is told past the last.
    search: Option<SearchFile>,
    /// A function the model calls to snooze the skill or turn it off.
    stop: Option<StopFile>,
    /// A watch, turned on and off by the skill's tool ([`watch`]).
    watch: Option<watch::WatchFile>,
    /// Output ceilings are independent of a presentation's permission.
    limits: Option<routine::LimitsFile>,
    #[serde(rename = "do", default)]
    steps: Vec<StepFile>,
}

/// `[budget]`: the skills naming one `group` start at most `per_hour` runs
/// an hour between them (#1685: one nudge an hour, the lamp's whole).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BudgetFile {
    group: String,
    per_hour: u32,
}

/// `[rest]`: after `after_ignored` runs in a row that nobody noticed within
/// `within` of each, the skill rests until the next morning.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RestFile {
    after_ignored: u32,
    within: Option<String>,
}

/// `[rest]` as loaded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rest {
    pub after_ignored: u32,
    pub within_us: u64,
}

/// The most runs a day a skill may say it starts itself.
pub const MOST_A_DAY_AT_MOST: u32 = 24;
/// The most runs an hour a group may share.
pub const PER_HOUR_AT_MOST: u32 = 6;
/// How long after a run a look or a movement counts as noticing it, when
/// `[rest]` names no `within`.
pub const NOTICED_WITHIN_US: u64 = 60_000_000;
/// The morning a rest or a snooze lasts until, local minutes of the day: 06:00.
pub const MORNING: u16 = 6 * 60;
/// A face that moved this share of the picture across or down since the
/// run, or grew or shrank by this share of its height, has moved.
pub const NOTICED_MOVE: f32 = 0.15;
pub const NOTICED_SIZE: f32 = 0.25;
const HOUR_US: u64 = 3_600_000_000;
/// How long a member's wait for its group's hour still counts after it was
/// last held back for it: past that it is not waiting any more (its trigger
/// ended, or something else holds it back).
pub const BUDGET_WAIT_LIVE_US: u64 = 120_000_000;
/// `[stop]`: the function by which a person snoozes what the skill starts
/// itself, or turns it off or on again (#1685).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StopFile {
    tool: String,
    about: String,
}

/// `[stop]` as loaded: the function's name and when to call it, as the
/// model is told.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stop {
    pub tool: String,
    pub about: String,
}

/// The one argument of a `[stop]` function, and its values: `snooze` until
/// the next local morning, `off` until it is asked for again, `on` again.
pub const STOP_CHOICE: &str = "how";
pub const STOP_WAYS: [&str; 3] = ["snooze", "off", "on"];
/// The yes-or-no of a `[stop]` function its group's skills share: true for
/// every one of them, left out for the one that nudged last.
pub const STOP_ALL: &str = "all";
/// The most a `[stop]`'s `about` may say.
pub const STOP_ABOUT_MOST_BYTES: usize = 512;
/// The file in `--state-dir` that keeps the skills turned off: one line
/// each, the skill's name, and nothing else.
pub const OFF_FILE: &str = "off";
/// The file in `--state-dir` beside [`OFF_FILE`] that keeps the core skills
/// shipped off that the person turned on (#2585, `lamp-update`'s
/// `product_skills::ON`): one name a line, kept on through a round.
pub const ON_FILE: &str = "on";

/// The names kept in `file` ([`OFF_FILE`] or [`ON_FILE`]), one a line; a
/// line that is not a skill's name is passed over, and no file is none.
/// The one reader of both, for the running program and the manage helper.
/// Never read through a link (`O_NOFOLLOW`: one is refused).
pub fn read_names(file: &Path) -> std::io::Result<BTreeSet<String>> {
    use std::{io::Read, os::unix::fs::OpenOptionsExt};
    let read = || -> std::io::Result<String> {
        let mut text = String::new();
        fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(file)?
            .read_to_string(&mut text)?;
        Ok(text)
    };
    match read() {
        Ok(text) => Ok(text
            .lines()
            .map(str::trim)
            .filter(|name| plain_name(name))
            .map(str::to_owned)
            .collect()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(BTreeSet::new()),
        Err(error) => Err(error),
    }
}

/// `names` written whole to `file`, one a line: a file beside it, synced,
/// then moved over it, and the folder synced, so a power cut leaves the
/// old file or the new one whole, never an empty one, and the move itself
/// holds (the reviewer's P3 on #1837, as #1732's daily journal does). The
/// one writer of [`OFF_FILE`] and [`ON_FILE`]: a `[stop]`'s off and on,
/// and the app's ([`package::turn`]). Root's helper writes it in a folder
/// another user owns (the reviewer's P2 on #2878), so the file beside is
/// only ever made new (`O_CREAT | O_EXCL`, which never follows a link): a
/// file left there by a cut-off run is removed first, and a link there is
/// refused, its target untouched.
pub fn keep_names(file: &Path, names: &BTreeSet<String>) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let lines: String = names.iter().map(|name| format!("{name}\n")).collect();
    let new = file.with_extension("new");
    match fs::symlink_metadata(&new) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(std::io::Error::other(format!(
                "{} is a link: refused",
                new.display()
            )));
        }
        Ok(_) => fs::remove_file(&new)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let mut out = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(libc::O_NOFOLLOW)
        .mode(0o644)
        .open(&new)?;
    std::io::Write::write_all(&mut out, lines.as_bytes())?;
    out.sync_all()?;
    fs::rename(&new, file)?;
    if let Some(folder) = file.parent() {
        fs::File::open(folder)?.sync_all()?;
    }
    Ok(())
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

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChoiceFile {
    name: String,
    about: String,
    one_of: Vec<String>,
    /// The value a call that gives none is made as (#1679: a light show
    /// asked with no hint is a party). Without it, such a call has none.
    default: Option<String>,
    /// In a skill with a trigger and no tool (#1669 (e), watching someone
    /// leave): the choice is this field of the message that fired it, as
    /// it is when the steps are made. A message whose value is not in
    /// `one_of`, or has none, does not hold the trigger ([`chooses`]).
    field: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct FlagFile {
    name: String,
    about: String,
}

/// `[search]`: a tool's calls that are one search (#1685, "look for my
/// keys"): at most `looks` of them, the first at the choice's default.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchFile {
    looks: u32,
    give_up: String,
    #[serde(default)]
    found: Option<String>,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    walk: Vec<String>,
    #[serde(default)]
    looked: BTreeMap<String, String>,
}

/// A search a person asked for, as loaded: its calls are the looks of one
/// run, the first starting it and the others made while it is under way.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Search {
    /// The most looks of one search: one past them is refused with `give_up`.
    pub looks: u32,
    /// What the model is told, as an error, of a look past the last: to say
    /// kindly that it cannot see the thing from here.
    pub give_up: String,
    /// What the model is told when it calls with the choice [`FOUND`]:
    /// the thing is seen, so nothing is looked at again, nothing moves and
    /// nothing is counted, and the head stays held where it found it.
    pub found: Option<String>,
    /// What the second voice route tells the model, once a search, of a
    /// look it left with no call (docs/second-voice.md, the look's note).
    pub note: Option<String>,
    /// The places the second route's walk goes on to after the note, in
    /// order, each once a search: a place already looked at is skipped.
    pub walk: Vec<String>,
    /// By place, where a walked look's turn says it looked ("to the left").
    pub looked: BTreeMap<String, String>,
}

/// The longest a search's `looked` may say a place in: a few words.
const LOOKED_AT_MOST: usize = 64;

/// The choice a search's model calls with once it sees the thing (#1871):
/// a place that is no place, so the search can tell "found" from a look
/// that was not made.
pub const FOUND: &str = "found";

/// What the model is told of a `found` with no look held (#2262): before
/// the search's first look, or after it ended.
pub const FOUND_BEFORE_A_LOOK: &str =
    "Nothing is being looked at now: look first, with a place, and say found only once you see it.";

/// The most looks a search may say it takes: a living thing looks at a few
/// places and lets it go (CLAUDE.md's rule 8).
pub const SEARCH_LOOKS_AT_MOST: u32 = 8;

/// `[history]`: a tool's call is told who was at the lamp over the last
/// `within` (#1685), from `room/presence`, times only.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryFile {
    topic: String,
    within: String,
    /// What the record says, in the skill's own words
    /// ([`visits::Words`]); a file from before them says the built-in
    /// ones, for one release.
    words: Option<visits::Words>,
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
    /// Scalar field paths compared together by `when = "changes"`.
    changes: Option<Vec<String>>,
    field: Option<String>,
    equals: Option<toml::Value>,
    above: Option<f64>,
    below: Option<f64>,
    /// On `humans/faces/tracked`: only a face at least this share of the
    /// picture's height counts.
    near: Option<f32>,
    /// An explicitly observed empty face list, never a missing camera.
    present: Option<bool>,
    /// Local hours in which this trigger can hold.
    during: Option<QuietFile>,
    /// Consume at most one chance per local window, durably before acting.
    #[serde(default)]
    once_per_day: bool,
    /// No chance once something was heard on `voice/heard` since its
    /// `during` window opened today: the person already spoke to the lamp.
    #[serde(default)]
    unless_heard: bool,
    absent_for: Option<String>,
    #[serde(rename = "for")]
    lasting: Option<String>,
    gap: Option<String>,
    rearm: Option<RearmFile>,
    cooldown: Option<String>,
    /// A chance held back by a `[[while]]` condition waits this long, while
    /// the trigger still holds, for the conditions to hold.
    wait: Option<String>,
    /// The field rose at least this much within this while (#1684).
    rise: Option<RiseFile>,
    /// After a night the lamp rested, a chance waits for another skill's
    /// daily trigger to be judged first (#1672): its greeting comes first.
    yields_to: Option<YieldFile>,
    /// A shorter `for` while another topic's field is above a line (#3709).
    sooner: Option<SoonerFile>,
}

/// `sooner = { topic = "room/activity", field = "yawns", above = 0, for = "30m" }`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SoonerFile {
    topic: String,
    field: String,
    above: f64,
    #[serde(rename = "for")]
    lasting: String,
}

/// `yields_to = { skill = "bedtime-morning", trigger = "morning" }`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct YieldFile {
    skill: String,
    trigger: String,
}

/// `rise = { by = 100, within = "3m" }`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RiseFile {
    by: f64,
    within: String,
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
    /// On `humans/faces/tracked` alone: at most this many faces in view
    /// (#3504: a comfort break is said with nobody else there).
    faces_at_most: Option<u32>,
    /// It has held this long.
    #[serde(rename = "for")]
    lasting: Option<String>,
    /// `"holds"`: it holds while its topic has said nothing yet (#1677: a
    /// rule's topic on a lamp without the rules); a value that has run out
    /// still does not hold. Left out, a silent topic never holds.
    unknown: Option<String>,
    /// Values of the field its clock is carried through (#1672): during a
    /// spell of one of them it does not hold, so nothing runs then, but its
    /// `for` is not begun again either. With `equals` and `for`, and only
    /// with [`REST_THROUGH_SOUNDS_ENVIRONMENT`] on; off, read and ignored.
    through: Option<Vec<toml::Value>>,
    /// Topics whose messages begin that clock again ([`BROKEN_BY`]): words
    /// heard, a new push. With `through`.
    broken_by: Option<Vec<String>>,
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
    /// A fixed quiet cue rendered by the Say owner, without an asset file.
    cue: Option<String>,
    /// A leased rest intent, admitted by Expression's existing owner.
    rest: Option<routine::RestFile>,
    /// A fixed local WAV for a spoken tool request, after its confirmation.
    requested_audio: Option<String>,
    look: Option<bool>,
    /// `"say"`: this step and every one after it wait for the say to end.
    after: Option<String>,
    /// The name of the trigger this step is made for; for every one without.
    on: Option<String>,
    /// The values of the tool's `[choice]` this step is made for: only in a
    /// call that chose one of them (#1679: a scene's own light and pose).
    if_choice: Option<Vec<String>>,
    /// A light show, `shows/NAME.toml`: its frames in turn (#1679).
    show: Option<String>,
    /// An add-on's proposal (`kind = "addon"` only, `docs/addons.md`): what
    /// it suggests, which a judge keeps or drops.
    propose: Option<ProposeFile>,
}

/// `propose = { ... }`: what an add-on suggests, as its folder writes it.
/// Every word in it is the folder's, read when the skill is installed.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposeFile {
    act: String,
    why: String,
    sure: f32,
    /// How long it stays worth doing: `"30s"`, at most 30 s.
    holds: String,
    light: Option<ProposeLightFile>,
    gesture: Option<String>,
    mood: Option<ProposeMoodFile>,
    note: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposeLightFile {
    colour: String,
    level: u8,
    pattern: Option<PatternFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposeMoodFile {
    energy: Option<f32>,
    warmth: Option<f32>,
    /// How long the nudge lasts: `"10m"`, at most 10 minutes.
    #[serde(rename = "for")]
    lasting: String,
}

/// Which runs a step is made in, when not every one: those its trigger
/// started (`on`), or a call that chose one of these values (`if_choice`).
#[derive(Clone, Debug, PartialEq)]
enum Only {
    Trigger(String),
    Choice(Vec<String>),
}

/// A show's file, `shows/NAME.toml`: its frames, each a light held a while.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ShowFile {
    #[serde(rename = "light")]
    frames: Vec<FrameFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FrameFile {
    colour: String,
    level: u8,
    #[serde(rename = "for")]
    lasting: String,
    pattern: Option<PatternFile>,
}

/// One light of a show, held `lasting_us` before the next.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub colour: String,
    pub level: u8,
    pub pattern: Option<LightPattern>,
    pub lasting_us: u64,
}

/// The say service's priority for a skill's own words when its say names
/// none: the voice's normal one (`lamp_voice::say::NORMAL_PRIORITY`).
pub const SAY_PRIORITY: u8 = 5;
/// The highest priority a skill's say may ask (#1684): 8, a due reminder's,
/// which the voice says after a second of quiet. 9 is the voice's own.
pub const MOST_SAY_PRIORITY: u8 = 8;
/// How long a say may wait for the voice when it names no `expires`: not
/// begun by then, it is of no use, the moment has passed.
pub const SAY_EXPIRES_MS: u64 = 30_000;
/// The longest a say may wait (the voice's `LONGEST_EXPIRY_MS`).
const MOST_SAY_EXPIRES_MS: u64 = 3_600_000;
/// The most bytes of a skill's own words the voice takes
/// (`lamp_voice::say::MOST_WORDS_BYTES`).
pub const MOST_WORDS_BYTES: usize = 900;

/// What a skill granted `prepare_unplug` says at each of the operation's
/// outcomes, in its own `skill.toml`'s `[unplug]` (#940: the first move of
/// "Everything is a skill"). The fold, its readiness check and the voice's
/// guard are the core grant's; the words are the skill's.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UnplugLines {
    /// Said as written, once the fold and the asleep look are confirmed.
    pub ready: String,
    /// A note for the voice when the fold isn't confirmed.
    pub failed: String,
    /// A note for the voice when the person says to stay plugged in.
    pub stay: String,
    /// A note for the voice when it gets back up after the wait.
    pub timeout: String,
    /// A note for the voice, said once when a preparation that was Ready
    /// fails before any cancel, or when the body fails one after the cancel
    /// (no stand within 60 s): it can't get up by itself.
    pub check: String,
}

impl UnplugLines {
    /// The words lamp-skills said before they were the skill's, said for
    /// one release by a skill granted `prepare_unplug` whose file has no
    /// `[unplug]` (the first builder's P2 on #2632): a lamp can keep an
    /// older `skill.toml` past a newer lamp-skills, by an updater from
    /// before the skills bundle, a lamp's own `--dir` (lamp-4ace's), or a
    /// file changed on the lamp, which a round keeps (#1913). The next
    /// release removes them, and such a file is then refused.
    pub fn built_in() -> Self {
        Self {
            ready: "My arm is folded. You can unplug me now.".into(),
            failed:
                "Say briefly: I could not confirm a safe folded position. Please do not unplug yet."
                    .into(),
            stay: "Say exactly: Okay, I'll stay.".into(),
            timeout: "Say exactly: I'm still plugged in, so I'm getting back up.".into(),
            check: "Say exactly: I can't get up by myself; I need to be checked.".into(),
        }
    }

    /// Each line is words, as a say's own are: 1 to [`MOST_WORDS_BYTES`]
    /// bytes, with no control characters.
    fn checked(self) -> Result<Self, String> {
        for (name, line) in [
            ("ready", &self.ready),
            ("failed", &self.failed),
            ("stay", &self.stay),
            ("timeout", &self.timeout),
            ("check", &self.check),
        ] {
            if line.trim().is_empty()
                || line.len() > MOST_WORDS_BYTES
                || line.chars().any(char::is_control)
            {
                return Err(format!(
                    "[unplug] {name} is words, 1 to {MOST_WORDS_BYTES} bytes, with no control characters"
                ));
            }
        }
        Ok(self)
    }
}

/// Each skill granted `prepare_unplug`, by name, with its words: its own
/// `[unplug]`, or, for this release only, the built-in ones, with a line
/// for the journal saying so ([`UnplugLines::built_in`]).
pub fn unplug_words(skills: &[Skill]) -> (HashMap<String, UnplugLines>, Vec<String>) {
    let mut said = Vec::new();
    let words = skills
        .iter()
        .filter(|skill| skill.prepares_unplug())
        .map(|skill| {
            let lines = skill.unplug.clone().unwrap_or_else(|| {
                said.push(format!(
                    "{} is granted prepare_unplug and says no [unplug] words: the built-in ones are said, for this release only (#940)",
                    skill.name
                ));
                UnplugLines::built_in()
            });
            (skill.name.clone(), lines)
        })
        .collect();
    (words, said)
}

/// What the journal says of each `yields_to` among `skills`, the loaded and
/// enabled ones (#1672): one naming a skill or daily trigger that is not
/// among them is ignored, and says so. Disabled, or a routine with
/// `LAMP_SKILL_ROUTINES` off, its skill is not loaded.
pub fn yield_notes(skills: &[Skill]) -> Vec<String> {
    let mut notes = Vec::new();
    for skill in skills {
        for trigger in &skill.triggers {
            let Some((other, name)) = &trigger.yields_to else {
                continue;
            };
            let found = skills.iter().any(|loaded| {
                &loaded.name == other
                    && loaded.triggers.iter().any(|daily| {
                        daily.name.as_ref() == Some(name)
                            && daily.once_per_day
                            && daily.during.is_some()
                    })
            });
            if !found {
                notes.push(format!(
                    "{}: yields_to {other}'s {name}, which is not a loaded daily trigger: ignored, it does not wait",
                    skill.name
                ));
            }
        }
    }
    notes
}

/// What a `say` asks of the voice: a note it phrases, its exact words, or
/// what it keeps.
fn say_step(say: toml::Value) -> Result<Call, String> {
    const WHAT: &str = "say is a note in words, or { from = \"schedule\" } or { from = \"tasks\" }, \
                        or { text = ... } or { note = ... } with an optional priority and expires";
    match say {
        toml::Value::String(note) if note.trim().is_empty() => Err(WHAT.into()),
        toml::Value::String(note) => Ok(Call::Say(note)),
        toml::Value::Table(table) if table.contains_key("from") => {
            if table.len() != 1 {
                return Err(format!("{WHAT}: a say table has from and nothing else"));
            }
            match table["from"].as_str() {
                Some("schedule") => Ok(Call::SayFrom(Source::Schedule, None)),
                Some("tasks") => Ok(Call::SayFrom(Source::Tasks, None)),
                Some("onboarding") => Ok(Call::SayFrom(Source::Onboarding, None)),
                Some(other) => Err(format!(
                    "say.from is \"schedule\" or \"tasks\", not {other:?}"
                )),
                None => Err(format!("{WHAT}: from is a word")),
            }
        }
        toml::Value::Table(table) => {
            if let Some(key) = table
                .keys()
                .find(|key| !["text", "note", "priority", "expires"].contains(&key.as_str()))
            {
                return Err(format!("{WHAT}: say.{key} is not known"));
            }
            let (words, exact) = match (table.get("text"), table.get("note")) {
                (Some(text), None) => (text, true),
                (None, Some(note)) => (note, false),
                _ => return Err(format!("{WHAT}: one of text or note")),
            };
            let words = words
                .as_str()
                .filter(|words| {
                    !words.trim().is_empty()
                        && words.len() <= MOST_WORDS_BYTES
                        && !words.chars().any(char::is_control)
                })
                .ok_or(format!(
                    "say's text or note is words, 1 to {MOST_WORDS_BYTES} bytes, with no control characters"
                ))?
                .to_owned();
            let priority = match table.get("priority") {
                None => SAY_PRIORITY,
                Some(priority) => priority
                    .as_integer()
                    .and_then(|priority| u8::try_from(priority).ok())
                    .filter(|priority| *priority <= MOST_SAY_PRIORITY)
                    .ok_or(format!(
                        "say.priority is a whole number from 0 to {MOST_SAY_PRIORITY}: 9 is the voice's own"
                    ))?,
            };
            let expires_ms = match table.get("expires") {
                None => SAY_EXPIRES_MS,
                Some(expires) => {
                    expires
                        .as_str()
                        .ok_or_else(|| "say.expires is a duration".to_owned())
                        .and_then(duration_us)?
                        / 1_000
                }
            };
            if !(1_000..=MOST_SAY_EXPIRES_MS).contains(&expires_ms) {
                return Err("say.expires is 1s to 1h".into());
            }
            Ok(Call::Words(Words {
                words,
                exact,
                priority,
                expires_ms,
            }))
        }
        _ => Err(WHAT.into()),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BodyFile {
    /// Request the body's guarded, latched fold before unplugging.
    prepare_unplug: Option<bool>,
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
    /// With the reading pose: the base turned toward this place in the
    /// camera's picture, -1 its left edge to 1 its right (#1679).
    toward: Option<f32>,
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
    /// `true`: the light goes out, asked of the ring (#1677).
    out: Option<bool>,
    /// How slowly the ring fades into it, with `out`.
    over: Option<String>,
    until: Option<String>,
    pattern: Option<PatternFile>,
    /// The person's own light, set as they asked and never given back
    /// (#1679: a scene). Only in a skill its tool alone starts.
    keep: Option<bool>,
    /// Steps from the person's light as the ring shows it now, read as the
    /// run begins (#1672: the evening glow), in place of a colour.
    relative: Option<RelativeFile>,
}

/// `relative = { warmer = 1, dimmer = 1 }`: steps along the ring's own
/// scales, warmth ([`glow::EVENING_WARMTH`]) and level (1 to 5).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RelativeFile {
    warmer: Option<u8>,
    dimmer: Option<u8>,
}

/// How a skill's light moves, as its file says it (#940 item 3):
/// `{ kind = "breathe", period = "6s", low = 1 }`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PatternFile {
    kind: String,
    period: Option<String>,
    low: Option<u8>,
}

/// Whether a skill comes with the lamp or is its owner's to put on and take
/// off: like a phone's own apps and the apps one installs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SkillKind {
    /// Ships with the product (`lamp-update install` writes it), always
    /// there, and `lamp-skills remove` refuses it.
    Core,
    /// Installed and removed on a running lamp (`lamp-skills install`,
    /// `lamp-skills remove`), from a folder or a package.
    #[default]
    Installable,
    /// A partner's add-on (`docs/addons.md`): installed and removed as an
    /// installable skill, but it only proposes. It reads the add-on topics
    /// alone ([`lamp_msgs::ADDON_TOPICS`]), its steps are proposals that
    /// `lamp-skills` writes on `addon/proposal` for a judge, and it runs
    /// only with `LAMP_ADDONS=on` ([`ADDONS_ENVIRONMENT`]).
    Addon,
}

impl SkillKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Installable => "installable",
            Self::Addon => "addon",
        }
    }

    /// `kind` as `skill.toml` writes it; `installable` when it says none.
    pub fn read(kind: Option<&str>) -> Result<Self, String> {
        match kind {
            Some("core") => Ok(Self::Core),
            None | Some("installable") => Ok(Self::Installable),
            Some("addon") => Ok(Self::Addon),
            Some(other) => Err(format!(
                "kind = {other:?}: \"core\", \"installable\" or \"addon\""
            )),
        }
    }
}

/// The actions part's functions a skill's light steps call (a colour, a
/// step brighter or dimmer) and its give-back (off again): what
/// `light = true` lets it call.
pub const LIGHT_ACTIONS: [&str; 4] = [
    "light_colour",
    "light_brighter",
    "light_dimmer",
    "light_off",
];

/// The most entries of each `[permissions]` list.
const MOST_PERMITTED: usize = 32;

/// `[permissions]`: what a skill may read and use, and nothing else (least
/// privilege: a skill without the table may do nothing). Checked when it
/// loads, a skill reading or doing anything undeclared being refused with
/// why, and again where each call leaves the program ([`Permissions::refuses`]).
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Permissions {
    /// The topics it reads: those its triggers and `[[while]]` conditions
    /// name, `humans/faces/tracked` for a look at a face, `voice/heard`
    /// for a tool's `[again]`, `voice/state` for a routine's quiet gate,
    /// and `schedule/due` or `tasks/status` for a say from the voice's
    /// reminder or task news ([`Skill::reads`]).
    #[serde(default)]
    pub topics: Vec<String>,
    /// The actions part's functions it may call by name, beyond the
    /// light's own, which `light` gives. No step names one yet: this is
    /// the list such a step will be held to.
    #[serde(default)]
    pub actions: Vec<String>,
    /// It may speak: a note or the voice's own words (`say`), or a sound.
    #[serde(default)]
    pub speak: bool,
    /// It may change the light (`light`).
    #[serde(default)]
    pub light: bool,
    /// It may move the body: look at a face, a gesture, a move, a pose.
    #[serde(default)]
    pub body: bool,
    /// Presence and face position only, the protocol's `look`
    /// (docs/control.md): who is there, how near, which way they face,
    /// never the picture. In format 1 it also still gives the picture, as
    /// `camera` does ([`Permissions::sees_picture`]), so a skill written
    /// before `camera` keeps working; the app shows it as the camera.
    #[serde(default)]
    pub look: bool,
    /// The model may see a picture through the lamp's camera: the voice's
    /// full look of what is in front of the lamp (a `look = true` step).
    /// Its own grant, shown apart with a strong warning, never given by
    /// default (#3822's gap 1, the helper's decision on #3504).
    #[serde(default)]
    pub camera: bool,
    /// The skill says `format = 2`: its `look` is presence alone, never the
    /// picture, and gives the presence topics ([`PRESENCE_TOPICS`]).
    /// Format 1 (the key left out) when false.
    #[serde(skip)]
    pub format_2: bool,
    /// It may send a short text for the person's phone, unasked
    /// (`messages/out`, a watch's news: [`watch`]).
    #[serde(default)]
    pub messages: bool,
    /// It may know whether a face at the lamp is one the eyes know, and
    /// whether it is the person who asked it, never whose (a watch's: the
    /// eyes' known file, read by the program).
    #[serde(default)]
    pub known_faces: bool,
    /// Its note to the voice is told who is at the lamp, after it: a known
    /// face's name, and to be one beat warmer to someone who comes by often
    /// (#2825, Welcome back). The skill itself is never told. Off by
    /// default, so no other skill's words can name anyone.
    #[serde(default)]
    pub who: bool,
    /// It may install declarative light and volume ceilings. This grants
    /// no presentation or generic action, and no restoration of a setting.
    #[serde(default)]
    pub output_limits: bool,
    /// What an add-on may propose ([`lamp_msgs::ADDON_ACTS`]): its only
    /// output, and an add-on's alone.
    #[serde(default)]
    pub propose: Vec<String>,
    /// An add-on's say grant (#2530 part 2): `"quoted"`, the only value. An
    /// add-on that proposes `say` declares it as well, so the person reads
    /// at install that its words may reach the voice, quoted and untrusted;
    /// any other kind declaring it is refused when it loads.
    #[serde(default)]
    pub say: Option<String>,
    /// Core-only grants ([`CORE_GRANTS`]): what a skill that comes with the
    /// lamp may do that no other may, declared here rather than known by its
    /// name. Any other kind declaring one is refused when it loads, and a
    /// copy of the skill without the declaration gets nothing of it.
    #[serde(default)]
    pub core: Vec<String>,
}

/// The core-only grants a `kind = "core"` skill may list in `[permissions]
/// core`: `onboarding`, a say from the voice's onboarding invitations
/// (`say = { from = "onboarding" }`) and their first snapshot; and
/// `prepare_unplug`, the composite unplug operation (`body = {
/// prepare_unplug = true }`), its reads and the reserved never-mind tool;
/// and `sleep`, the night's sleep (#940, Night sleep): the creature's own,
/// so its runs ask nobody, the Policy or `LAMP_SKILLS_ASK`, and its steps
/// are its rest alone, no light, sound or words; and `startle`, the glance
/// at a sudden sound (#2822): its `[startle]` is the move Expression plays
/// as a reflex, and the skill has nothing else.
pub const CORE_GRANTS: [&str; 4] = ["onboarding", "prepare_unplug", "sleep", "startle"];

/// An add-on's say grant, the one value of `[permissions] say`: its words
/// reach the voice quoted and untrusted, never as an instruction.
pub const ADDON_SAY_GRANT: &str = "quoted";

impl Permissions {
    /// Why a skill with these permissions may not make `call`, if not.
    pub fn refuses(&self, call: &Call) -> Option<String> {
        // The composite unplug operation moves the body, reads completion
        // evidence and speaks separately; none of those can be undeclared.
        if *call == Call::PrepareUnplug {
            if !self.grants("prepare_unplug") {
                return Some(
                    "body.prepare_unplug needs core = [\"prepare_unplug\"] in [permissions]".into(),
                );
            }
            if !self.body || !self.speak {
                return Some(
                    "body.prepare_unplug needs body = true and speak = true in [permissions]"
                        .into(),
                );
            }
            return unplug::READ_TOPICS
                .iter()
                .find_map(|topic| self.refuses_reading(topic));
        }
        if matches!(call, Call::SayFrom(Source::Onboarding, _)) && !self.grants("onboarding") {
            return Some(
                "a say from onboarding needs core = [\"onboarding\"] in [permissions]".into(),
            );
        }
        let (key, given) = match call {
            Call::Gaze { .. }
            | Call::Gesture(_)
            | Call::Move(_)
            | Call::Mood { .. }
            | Call::Rest(_) => ("body", self.body),
            Call::Light { .. }
            | Call::Relative { .. }
            | Call::Dim { .. }
            | Call::Out { .. }
            | Call::Show { .. } => ("light", self.light),
            Call::Say(_)
            | Call::Words(_)
            | Call::SayFrom(..)
            | Call::Sound { .. }
            | Call::Cue
            | Call::RequestedAudio(_) => ("speak", self.speak),
            Call::Look => ("camera", self.sees_picture()),
            Call::Text(_) => ("messages", self.messages),
            Call::Propose(proposal) => {
                return (!self.propose.contains(&proposal.act)).then(|| {
                    format!(
                        "propose {} needs {:?} in [permissions] propose, which it does not give",
                        proposal.act, proposal.act
                    )
                });
            }
            Call::PrepareUnplug => unreachable!("handled above"),
            Call::AfterSay | Call::Ends => return None,
        };
        (!given).then(|| {
            format!(
                "{} needs {key} = true in [permissions], which it does not give",
                call.named().0
            )
        })
    }

    /// Whether the model may see the camera's picture: `camera`, or format
    /// 1's `look`, which gave it before `camera` was its own grant.
    pub fn sees_picture(&self) -> bool {
        self.camera || (self.look && !self.format_2)
    }

    /// Whether it lists the core-only grant `grant` ([`CORE_GRANTS`]).
    pub fn grants(&self, grant: &str) -> bool {
        self.core.iter().any(|given| given == grant)
    }

    /// Core-only grants are known ones, once each, and a core skill's alone.
    fn check_core(&self, kind: SkillKind) -> Result<(), String> {
        if let Some(wrong) = self
            .core
            .iter()
            .find(|grant| !CORE_GRANTS.contains(&grant.as_str()))
        {
            return Err(format!(
                "[permissions] core: {wrong:?} is not one of {CORE_GRANTS:?}"
            ));
        }
        if (1..self.core.len()).any(|at| self.core[..at].contains(&self.core[at])) {
            return Err("[permissions] core lists one twice".into());
        }
        match self.core.first() {
            Some(grant) if kind != SkillKind::Core => Err(format!(
                "[permissions] core = [{grant:?}] requires kind = core: the core {grant} grant \
                 is the lamp's own skills' alone, never an {} skill's",
                kind.name()
            )),
            _ => Ok(()),
        }
    }

    /// The add-on's say grant ([`Permissions::say`]): `"quoted"` only, an
    /// add-on's alone, and declared by every add-on that proposes `say`.
    fn check_say(&self, kind: SkillKind) -> Result<(), String> {
        let proposes_say = self.propose.iter().any(|act| act == "say");
        match self.say.as_deref() {
            Some(given) if given != ADDON_SAY_GRANT => Err(format!(
                "[permissions] say is {ADDON_SAY_GRANT:?} only, not {given:?}"
            )),
            Some(_) if kind != SkillKind::Addon => Err(
                "[permissions] say = \"quoted\" is an add-on's say grant, never another kind's"
                    .into(),
            ),
            Some(_) if !proposes_say => {
                Err("[permissions] say = \"quoted\" goes with propose naming \"say\"".into())
            }
            None if proposes_say => {
                Err("an add-on that proposes say declares say = \"quoted\" in [permissions]".into())
            }
            _ => Ok(()),
        }
    }

    /// Why new output ceilings may not be installed. Cleanup of ceilings
    /// already accepted belongs to the service even after this is revoked.
    pub fn refuses_output_limits(&self) -> Option<String> {
        (!self.output_limits).then(|| {
            "[limits] needs output_limits = true in [permissions], which it does not give".into()
        })
    }

    /// Why a skill with these permissions may not read `topic`, if not.
    pub fn refuses_reading(&self, topic: &str) -> Option<String> {
        // Format 2's `look` is the protocol's: the presence topics with it.
        if self.format_2 && self.look && PRESENCE_TOPICS.contains(&topic) {
            return None;
        }
        (!self.topics.iter().any(|given| given == topic))
            .then(|| format!("{topic} is not in [permissions] topics"))
    }

    /// Why a skill with these permissions may not call the actions part's
    /// function `name`, if not.
    pub fn refuses_action(&self, name: &str) -> Option<String> {
        let light = self.light && LIGHT_ACTIONS.contains(&name);
        (!light && !self.actions.iter().any(|given| given == name)).then(|| {
            if LIGHT_ACTIONS.contains(&name) {
                format!("the actions part's {name} needs light = true in [permissions]")
            } else {
                format!("the actions part's {name} is not in [permissions] actions")
            }
        })
    }

    /// What they give, in words, for a list of skills: `nothing` for none.
    pub fn said(&self) -> String {
        let mut parts = Vec::new();
        if !self.topics.is_empty() {
            parts.push(format!("reads {}", self.topics.join(", ")));
        }
        let may: Vec<&str> = [
            ("speak", self.speak),
            ("light", self.light),
            ("body", self.body),
            ("look", self.look && self.format_2),
            ("camera", self.sees_picture()),
            ("messages", self.messages),
            ("known faces", self.known_faces),
            ("who is at the lamp", self.who),
            ("output_limits", self.output_limits),
        ]
        .into_iter()
        .filter_map(|(name, given)| given.then_some(name))
        .collect();
        if !may.is_empty() {
            parts.push(may.join(", "));
        }
        if !self.actions.is_empty() {
            parts.push(format!("actions {}", self.actions.join(", ")));
        }
        if !self.propose.is_empty() {
            parts.push(format!("proposes {}", self.propose.join(", ")));
        }
        if !self.core.is_empty() {
            parts.push(format!("core {}", self.core.join(", ")));
        }
        if parts.is_empty() {
            "nothing".into()
        } else {
            parts.join("; ")
        }
    }

    /// Each list's entries well formed, once each.
    fn check(&self) -> Result<(), String> {
        for (what, list, ok) in [
            ("topics", &self.topics, topic_name as fn(&str) -> bool),
            ("actions", &self.actions, function_name),
        ] {
            if list.len() > MOST_PERMITTED {
                return Err(format!(
                    "[permissions] {what} lists at most {MOST_PERMITTED}"
                ));
            }
            if let Some(wrong) = list.iter().find(|entry| !ok(entry)) {
                return Err(format!(
                    "[permissions] {what}: {wrong:?} is not {}",
                    if what == "topics" {
                        "a topic: tokens of [a-z][a-z0-9_]{0,31} joined by /"
                    } else {
                        "a function: a lowercase letter, then at most 47 lowercase letters, digits and _"
                    }
                ));
            }
            if (1..list.len()).any(|at| list[..at].contains(&list[at])) {
                return Err(format!("[permissions] {what} lists one twice"));
            }
        }
        if let Some(wrong) = self
            .propose
            .iter()
            .find(|act| !lamp_msgs::ADDON_ACTS.contains(&act.as_str()))
        {
            return Err(format!(
                "[permissions] propose: {wrong:?} is not one of {:?}",
                lamp_msgs::ADDON_ACTS
            ));
        }
        if (1..self.propose.len()).any(|at| self.propose[..at].contains(&self.propose[at])) {
            return Err("[permissions] propose lists one twice".into());
        }
        // A note's words would ride on the person's next turn, which has
        // every function: refused until notes are typed, never words
        // (the first builder, 21 Oct, #2530).
        if self.propose.iter().any(|act| act == "note") {
            return Err(
                "[permissions] propose: \"note\" is refused for now: its words would reach the person's next turn, which has every function"
                    .into(),
            );
        }
        Ok(())
    }
}

/// A topic's name as the bus takes it (`lamp_bus`'s rule): tokens of
/// `[a-z][a-z0-9_]{0,31}` joined by `/`, 96 bytes at most.
fn topic_name(name: &str) -> bool {
    name.len() <= lamp_bus::MOST_TOPIC_BYTES
        && name.split('/').all(|token| {
            (1..=32).contains(&token.len())
                && token.as_bytes()[0].is_ascii_lowercase()
                && token
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        })
}

/// A function's name as the provider link takes it: a lowercase letter,
/// then at most 47 lowercase letters, digits and `_`.
fn function_name(name: &str) -> bool {
    let mut letters = name.bytes();
    letters
        .next()
        .is_some_and(|first| first.is_ascii_lowercase())
        && name.len() <= 48
        && letters.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

/// What a skill of kind `addon` may not have (`docs/addons.md`, "The
/// `addon` kind's manifest"): it only proposes, it starts itself by its
/// triggers alone, and every proposal goes to a judge, so no output of its
/// own, no tool a model could call, no Policy's question and no step but a
/// proposal.
fn addon_file(file: &SkillFile, no_triggers: bool) -> Result<(), String> {
    let refused = |what: &str| {
        Err(format!(
            "an add-on only proposes: {what} is refused for kind = \"addon\""
        ))
    };
    if file.version.is_none() {
        return Err("an add-on says its version: version = \"MAJOR.MINOR.PATCH\"".into());
    }
    if no_triggers {
        return Err("an add-on starts itself by its triggers alone: it needs one".into());
    }
    let p = &file.permissions;
    for (given, what) in [
        (p.speak, "speak"),
        (p.light, "light"),
        (p.body, "body"),
        (p.look, "look"),
        (p.camera, "camera"),
        (p.messages, "messages"),
        (p.known_faces, "known_faces"),
        (p.who, "who"),
        (p.output_limits, "output_limits"),
        (!p.actions.is_empty(), "actions"),
    ] {
        if given {
            return refused(&format!("{what} in [permissions]"));
        }
    }
    if p.propose.is_empty() {
        return Err("an add-on lists what it may propose in [permissions] propose".into());
    }
    if let Some(topic) = p
        .topics
        .iter()
        .find(|topic| !lamp_msgs::ADDON_TOPICS.contains(&topic.as_str()))
    {
        return refused(&format!(
            "reading {topic} (an add-on reads only {:?})",
            lamp_msgs::ADDON_TOPICS
        ));
    }
    for (there, what) in [
        (
            file.ask.is_some(),
            "[ask] (its judge is asked about every proposal)",
        ),
        (
            file.tool.is_some(),
            "a tool (a person's call is the model's, not a partner's)",
        ),
        (file.choice.is_some(), "[choice]"),
        (file.flag.is_some(), "[flag]"),
        (file.again.is_some(), "[again]"),
        (file.still.is_some(), "still"),
        (file.history.is_some(), "[history]"),
        (file.search.is_some(), "[search]"),
        (file.stop.is_some(), "[stop]"),
        (file.watch.is_some(), "[watch]"),
        (file.limits.is_some(), "[limits]"),
        (file.rest.is_some(), "[rest]"),
    ] {
        if there {
            return refused(what);
        }
    }
    for step in &file.steps {
        if step.propose.is_none()
            || step.body.is_some()
            || step.light.is_some()
            || step.say.is_some()
            || step.sound.is_some()
            || step.cue.is_some()
            || step.rest.is_some()
            || step.requested_audio.is_some()
            || step.look.is_some()
            || step.show.is_some()
            || step.after.is_some()
            || step.if_choice.is_some()
        {
            return refused("a step that is not a proposal");
        }
    }
    Ok(())
}

/// A proposal as an add-on's folder writes it, checked as the judge will
/// read it, under the add-on's name and version. Numbered when it is sent.
fn proposal(
    file: ProposeFile,
    addon: &str,
    version: &str,
) -> Result<lamp_msgs::AddonProposal, String> {
    let holds_ms = u32::try_from(duration_us(&file.holds)? / 1_000).unwrap_or(u32::MAX);
    let light = match file.light {
        None => None,
        Some(light) => Some(lamp_msgs::AddonLight {
            pattern: light
                .pattern
                .map(|pattern| pattern_of(pattern, Some(light.level)))
                .transpose()?,
            colour: light.colour,
            level: light.level,
        }),
    };
    let mood = match file.mood {
        None => None,
        Some(mood) => Some(lamp_msgs::AddonMood {
            energy: mood.energy,
            warmth: mood.warmth,
            for_ms: u32::try_from(duration_us(&mood.lasting)? / 1_000).unwrap_or(u32::MAX),
        }),
    };
    if let Some(gesture) = &file.gesture
        && !gesture_known(gesture)
    {
        return Err(format!(
            "propose: gesture {gesture:?} is not one of the body's"
        ));
    }
    let proposal = lamp_msgs::AddonProposal {
        // A placeholder that passes the check: the program numbers it.
        proposal: 1,
        addon: addon.into(),
        version: version.into(),
        act: file.act,
        why: file.why,
        sure: file.sure,
        holds_ms,
        light,
        gesture: file.gesture,
        mood,
        note: file.note,
    };
    proposal
        .check()
        .map_err(|lamp_msgs::Invalid(why)| format!("propose: {why}"))?;
    Ok(lamp_msgs::AddonProposal {
        proposal: 0,
        ..proposal
    })
}

/// `MAJOR.MINOR.PATCH`, each a whole number without a sign or a leading
/// zero (`1.0.0`, `2.13.4`).
pub fn version_ok(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.len() <= 9
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (*part == "0" || !part.starts_with('0'))
        })
}

/// A skill as loaded: everything in it checked.
#[derive(Clone, Debug, PartialEq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub limits: Option<routine::Limits>,
    pub needs: Vec<String>,
    /// Core or installable.
    pub kind: SkillKind,
    /// Its version, if `skill.toml` says one.
    pub version: Option<String>,
    /// What it may read and use, every read and step within them.
    pub permissions: Permissions,
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
    /// A search a person asked for (#1685): its calls are one run's looks.
    pub search: Option<Search>,
    /// With `[history]`: how far back a call is told who was at the lamp
    /// (#1685), and in what words (#940).
    pub history: Option<visits::History>,
    /// Local minutes of the day, from and to, when it never starts itself.
    quiet: Option<(u16, u16)>,
    /// The most runs it starts itself a local day.
    pub most_a_day: Option<u32>,
    /// Local minutes of the day, from and to, when alone it may start itself.
    active: Option<(u16, u16)>,
    /// Its group and that group's runs an hour, as this skill names them.
    pub budget: Option<(String, u32)>,
    /// When it rests after runs nobody noticed.
    pub rest: Option<Rest>,
    /// The function that snoozes it or turns it off (#1685).
    pub stop: Option<Stop>,
    /// Each step, and the runs it is made in when not every one: one
    /// trigger's, or a call's that chose one of its values.
    steps: Vec<(Option<Only>, Step)>,
    /// A watch, on only while a person asked for it ([`watch`]).
    pub watch: Option<watch::Spec>,
    /// With the `prepare_unplug` grant, what it says ([`UnplugLines`]).
    pub unplug: Option<UnplugLines>,
}

impl Skill {
    pub fn is_routine(&self) -> bool {
        self.limits.is_some()
            || self
                .steps
                .iter()
                .any(|(_, step)| matches!(step, Step::Call(Call::Rest(_))))
    }

    fn scheduled_run(&self, trigger: Option<&Trigger>) -> bool {
        self.is_routine()
            || trigger.is_some_and(|trigger| trigger.during.is_some() || trigger.once_per_day)
    }
    /// Every topic the skill reads, once each, and what reads it: what the
    /// program listens to for it ([`Skill::listens`]), and the topic of what
    /// a `say = { from = … }` has the voice say, `schedule/due` for the
    /// person's reminder and `tasks/status` for the news of their work
    /// ([`Source::topic`]). A skill may have the person's own words said at
    /// its moment only if it declares them. A routine also declares the
    /// voice state its quiet gate requires, even without a `[[while]]`.
    /// Output-owner receipts and other safety bookkeeping (the light shown,
    /// the body's clips, a hand, the pause) belong to the program itself.
    pub fn reads(&self) -> Vec<(String, &'static str)> {
        let mut reads = self.listens();
        // The privacy switch, looked at as it goes, never listened to: the
        // voice says it a hundred times a second.
        if self.watch.is_some() {
            reads.push((watch::PRIVACY.to_owned(), "a watch"));
        }
        if self.prepares_unplug() {
            for topic in unplug::READ_TOPICS {
                if !reads.iter().any(|(read, _)| read == topic) {
                    reads.push((topic.to_owned(), "unplug preparation"));
                }
            }
        }
        for (_, step) in &self.steps {
            if let Step::Call(Call::SayFrom(source, _)) = step
                && !reads.iter().any(|(topic, _)| topic == source.topic())
            {
                let by = match source {
                    Source::Schedule => "a say from schedule",
                    Source::Tasks => "a say from tasks",
                    Source::Onboarding => "a say from onboarding",
                };
                reads.push((source.topic().to_owned(), by));
            }
        }
        reads
    }

    /// The topics the program subscribes to for the skill ([`Engine::topics`]),
    /// once each, and what reads it: its triggers, its `[[while]]`
    /// conditions and what begins their clocks again (`broken_by`), a look
    /// at a face (the faces the eyes see), and a tool's
    /// `[again]` (what was heard), and a routine's quiet Voice gate. A say's
    /// words come from the voice, so their topic is read ([`Skill::reads`])
    /// and not listened to.
    fn listens(&self) -> Vec<(String, &'static str)> {
        let looks = self
            .steps
            .iter()
            .any(|(_, step)| matches!(step, Step::Call(Call::Gaze { .. })));
        let mut reads: Vec<(String, &'static str)> = self
            .triggers
            .iter()
            .map(|trigger| (trigger.topic.clone(), "a trigger"))
            .chain(
                self.conditions
                    .iter()
                    .map(|test| (test.topic.clone(), "a [[while]]")),
            )
            .chain(looks.then(|| (Faces::TOPIC.to_owned(), "a look at a face")))
            .chain(
                self.history
                    .is_some()
                    .then(|| (RoomPresence::TOPIC.to_owned(), "[history]")),
            )
            .chain(
                self.again
                    .is_some()
                    .then(|| (Speech::TOPICS[0].to_owned(), "[again]")),
            )
            .chain(self.triggers.iter().filter_map(|trigger| {
                trigger
                    .sooner
                    .as_ref()
                    .map(|sooner| (sooner.topic.clone(), "a trigger's sooner"))
            }))
            .chain(
                self.triggers
                    .iter()
                    .any(|trigger| trigger.unless_heard)
                    .then(|| (Speech::TOPICS[0].to_owned(), "a trigger's unless_heard")),
            )
            .chain(
                self.watch
                    .iter()
                    .flat_map(|_| watch::LISTENS)
                    .map(|topic| (topic.to_owned(), "a watch")),
            )
            // A watch's arrivals by sound, only where its permissions name
            // the ears' room sound classes (#2562).
            .chain(
                self.watch
                    .iter()
                    .filter(|_| self.permissions.refuses_reading(watch::SOUNDS).is_none())
                    .map(|_| (watch::SOUNDS.to_owned(), "a watch's arrivals by sound")),
            )
            // And the voice's state beside them, where they name it too: a
            // window the ears scored with the lamp's own voice unknown is
            // taken only when it says the lamp wasn't speaking (#2562).
            .chain(
                self.watch
                    .iter()
                    .filter(|_| {
                        self.permissions.refuses_reading(watch::SOUNDS).is_none()
                            && self.permissions.refuses_reading(watch::VOICE).is_none()
                    })
                    .map(|_| (watch::VOICE.to_owned(), "a watch's arrivals by sound")),
            )
            .chain(
                self.is_routine()
                    .then(|| (VoiceState::TOPIC.to_owned(), "a routine's quiet gate")),
            )
            // Last, so a topic read for anything else keeps that reason, and
            // only this one is left unheard with the switch off.
            .chain(self.conditions.iter().flat_map(|test| {
                test.broken_by
                    .iter()
                    .map(|topic| (topic.clone(), BROKEN_BY_READ))
            }))
            .collect();
        let mut seen = Vec::new();
        reads.retain(|(topic, _)| {
            let new = !seen.contains(topic);
            seen.push(topic.clone());
            new
        });
        reads
    }

    /// What the model is told of a call that says the search found the
    /// thing (`[search]`'s `found`, #1871): its choice [`FOUND`]. None for
    /// any other call, or a skill with no search or no `found`.
    pub fn search_found(&self, args: &Value) -> Option<&str> {
        let found = self.search.as_ref()?.found.as_deref()?;
        let choice = self.choice.as_ref()?;
        (args.get(&choice.name).and_then(Value::as_str) == Some(FOUND)).then_some(found)
    }

    /// The arguments a search's next look is made with, after `looked` of
    /// its looks (#1685): each as asked, the first at the choice's default
    /// (the desk, where a thing is looked for first) only when the call
    /// names no place, so a place the person named is where the lamp looks
    /// (the peer read on #1766); none past `looks`, the model told
    /// `give_up` instead. A skill with no search takes every call as asked.
    pub fn search_look(&self, args: &Value, looked: u32) -> Result<Value, String> {
        let Some(search) = &self.search else {
            return Ok(args.clone());
        };
        if looked >= search.looks {
            return Err(search.give_up.clone());
        }
        let mut args = args.clone();
        if looked == 0
            && let Some(choice) = &self.choice
            && let Some(default) = &choice.default
            && args.get(&choice.name).is_none()
        {
            if !args.is_object() {
                args = json!({});
            }
            args[&choice.name] = json!(default);
        }
        Ok(args)
    }

    /// The core unplug skill is not advertised until all parts are enabled.
    /// An add-on's proposals, as its folder writes them, not yet numbered.
    pub fn proposals(&self) -> Vec<&lamp_msgs::AddonProposal> {
        self.steps
            .iter()
            .filter_map(|(_, step)| match step {
                Step::Call(Call::Propose(proposal)) => Some(proposal.as_ref()),
                _ => None,
            })
            .collect()
    }

    pub fn prepares_unplug(&self) -> bool {
        self.steps
            .iter()
            .any(|(_, step)| *step == Step::Call(Call::PrepareUnplug))
    }

    /// Whether a step holds something until its trigger stops holding.
    fn holds_until_ends(&self) -> bool {
        self.steps
            .iter()
            .any(|(_, step)| matches!(step, Step::Call(call) if call.held_until_ends()))
    }
}

/// The line a trigger's field must stay past for a while before the trigger
/// runs again.
#[derive(Clone, Debug, PartialEq)]
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
    NoFaces,
    /// At most this many faces in `humans/faces/tracked`, by a list the
    /// eyes said whole (#3504).
    FacesAtMost(usize),
    Equals(String, Value),
    NotEquals(String, Value),
    Above(String, f64),
    Below(String, f64),
    /// The field is a number: a rise with no level of its own.
    Number(String),
}

impl Test {
    fn holds(&self, value: &Value) -> bool {
        match self {
            Self::NoFaces => serde_json::from_value::<Faces>(value.clone())
                .is_ok_and(|faces| faces.check().is_ok() && faces.faces.is_empty()),
            Self::FacesAtMost(most) => serde_json::from_value::<Faces>(value.clone())
                .is_ok_and(|faces| faces.check().is_ok() && faces.faces.len() <= *most),
            Self::Faces(near) => value["faces"].as_array().is_some_and(|faces| {
                faces.iter().any(|face| {
                    face["ymax"].as_f64().unwrap_or(0.0) - face["ymin"].as_f64().unwrap_or(0.0)
                        >= f64::from(*near)
                })
            }),
            Self::Equals(field, wanted) => equal(at(value, field), wanted),
            Self::NotEquals(field, unwanted) => !equal(at(value, field), unwanted),
            Self::Above(field, line) => at(value, field).as_f64().is_some_and(|got| got > *line),
            Self::Below(field, line) => at(value, field).as_f64().is_some_and(|got| got < *line),
            Self::Number(field) => at(value, field).as_f64().is_some(),
        }
    }
}

impl Test {
    /// What stopped being so when the test no longer holds, for the
    /// journal: `present no longer false`, `level no longer above 35`.
    fn lapsed(&self) -> String {
        fn shown(value: &Value) -> String {
            value
                .as_str()
                .map_or_else(|| value.to_string(), str::to_owned)
        }
        match self {
            Self::NoFaces => "present no longer false".into(),
            Self::Faces(_) => "present no longer true".into(),
            Self::FacesAtMost(most) => format!("more than {most} faces in view"),
            Self::Equals(field, wanted) => format!("{field} no longer {}", shown(wanted)),
            Self::NotEquals(field, unwanted) => format!("{field} now {}", shown(unwanted)),
            Self::Above(field, line) => format!("{field} no longer above {line}"),
            Self::Below(field, line) => format!("{field} no longer below {line}"),
            Self::Number(field) => format!("{field} no longer a number"),
        }
    }

    /// What it wants, for the journal: `state resting`, `a face`.
    fn shows(&self) -> String {
        fn shown(value: &Value) -> String {
            value
                .as_str()
                .map_or_else(|| value.to_string(), str::to_owned)
        }
        match self {
            Self::NoFaces => "no face".into(),
            Self::Faces(_) => "a face".into(),
            Self::FacesAtMost(most) => format!("at most {most} faces"),
            Self::Equals(field, wanted) => format!("{field} {}", shown(wanted)),
            Self::NotEquals(field, unwanted) => format!("{field} not {}", shown(unwanted)),
            Self::Above(field, line) => format!("{field} above {line}"),
            Self::Below(field, line) => format!("{field} below {line}"),
            Self::Number(field) => format!("{field} a number"),
        }
    }
}

/// A once-a-day trigger's watch reset after it had begun holding, said once
/// in the journal with why and for how long it held: on lamp-06c1 bedtime
/// never came in 23 minutes and nothing said why (#1672, row 7).
fn watch_reset(skill: &Skill, trigger: &Trigger, held_us: u64, why: String) -> Run {
    Run {
        generation: None,
        skill: skill.name.clone(),
        events: vec![event(
            &skill.name,
            SkillStage::Answered,
            format!(
                "{} watch reset after {} s: {why}",
                trigger.name.as_deref().unwrap_or("trigger"),
                held_us / 1_000_000
            ),
            Some(false),
        )],
        calls: Vec::new(),
        ask: None,
        queue: false,
        from: None,
    }
}

/// Why a trigger with `during` is out of it: outside its window, or no
/// trusted local moment to tell. None while inside.
fn out_of_window(trigger: &Trigger, moment: Option<&daily::Moment>) -> Option<String> {
    let window = trigger.during.as_ref()?;
    match moment {
        None => Some("no trusted local moment".into()),
        Some(moment) if !window.contains(moment.minute) => Some(format!(
            "left its window {}\u{2013}{}",
            window.opens(),
            window.closes()
        )),
        Some(_) => None,
    }
}

/// The field `path` names in `value`: a dot steps into an object, so
/// `scores.music` is the `music` of `scores` (#1669: `ears/classes` keeps its
/// scores in one object). A field that is not there is null.
fn at<'a>(value: &'a Value, path: &str) -> &'a Value {
    path.split('.').fold(value, |value, name| &value[name])
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
    /// It holds while its topic has said nothing yet (`unknown = "holds"`).
    unknown_holds: bool,
    /// The values its clock is carried through (`through`, #1672).
    through: Vec<Value>,
    /// The topics that begin its clock again (`broken_by`).
    broken_by: Vec<String>,
}

impl Condition {
    /// Whether `value` is one its clock is carried through (`through`).
    fn carries(&self, value: &Value) -> bool {
        match &self.test {
            Test::Equals(field, _) => {
                let got = at(value, field);
                self.through.iter().any(|through| equal(got, through))
            }
            _ => false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum When {
    /// Holds where it did not just before: once each time.
    Appears,
    /// Holds, and has for `for`.
    Present,
    /// First eligible observation, changed selected values, or eligibility
    /// restored after the trigger or one of its while conditions stopped.
    Changes,
}

const MOST_CHANGE_FIELDS: usize = 8;
const MOST_CHANGE_PATH_BYTES: usize = 128;
const MOST_CHANGE_STRING_BYTES: usize = 256;

#[derive(Clone, Debug, PartialEq)]
pub struct Trigger {
    pub topic: String,
    /// What a step's `on` calls it.
    pub name: Option<String>,
    /// The list each of whose items is watched on its own, and the field
    /// that names an item. The test is then of an item.
    pub each: Option<(String, String)>,
    test: Test,
    pub when: When,
    /// Selected scalar fields; empty for the existing trigger modes.
    pub changes: Vec<String>,
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
    pub during: Option<daily::Window>,
    pub once_per_day: bool,
    /// No chance once something was heard since `during` opened today; a
    /// daily trigger's day is spent then, said once.
    pub unless_heard: bool,
    /// It holds only while its field rose this much within a while (#1684).
    pub rise: Option<Rise>,
    /// The skill and its daily trigger this one's chance waits for, after a
    /// night the lamp rested (#1672): `yields_to`.
    pub yields_to: Option<(String, String)>,
    /// Its shorter `for` while another topic's field is above a line
    /// ([`SOONER_ENVIRONMENT`], #3709).
    pub sooner: Option<Sooner>,
}

/// A trigger's `sooner` (#3709): while `test` holds on `topic`'s newest
/// value that still lasts, the trigger needs to have held only `for_us`.
/// With `rearm`, it still takes one chance each time it holds: brought
/// forward, never a second time.
#[derive(Clone, Debug, PartialEq)]
pub struct Sooner {
    pub topic: String,
    test: Test,
    pub for_us: u64,
}

/// A trigger's `rise` (#1684): the field's newest reading is at least `by`
/// above the lowest of the readings of its topic in the `within_us` before
/// it. Smoke near the lamp raises PM2.5 by hundreds within minutes; a city's
/// haze raises it by tens over hours, so a rise tells them apart where a
/// level can't.
#[derive(Clone, Debug, PartialEq)]
pub struct Rise {
    pub field: String,
    pub by: f64,
    pub within_us: u64,
}

/// The most readings a rise keeps: an hour of readings every second. Past
/// it the oldest goes, which can only make a rise smaller, never larger.
const MOST_RISE_READINGS: usize = 3_600;

/// The readings a rise compares with, as a sliding minimum: each kept
/// reading is lower than every one kept after it, so the first is the
/// lowest of the while.
#[derive(Debug, Default)]
struct Readings {
    kept: std::collections::VecDeque<(u64, f64)>,
    /// Whether the newest reading rose, for an answer that comes later.
    risen: bool,
}

impl Readings {
    /// Whether `value` at `now_us` is at least `rise.by` above the lowest
    /// reading of the `rise.within_us` before it; then it is kept. With no
    /// reading in that while (the sensor was silent, or the program has
    /// just started or reloaded), nothing rose.
    fn rose(&mut self, value: f64, now_us: u64, rise: &Rise) -> bool {
        let from = now_us.saturating_sub(rise.within_us);
        while self.kept.front().is_some_and(|(at, _)| *at < from) {
            self.kept.pop_front();
        }
        self.risen = self
            .kept
            .front()
            .is_some_and(|(_, lowest)| value - lowest >= rise.by);
        while self.kept.back().is_some_and(|(_, kept)| *kept >= value) {
            self.kept.pop_back();
        }
        self.kept.push_back((now_us, value));
        if self.kept.len() > MOST_RISE_READINGS {
            self.kept.pop_front();
        }
        self.risen
    }
}

fn enabled_by_default() -> bool {
    true
}

/// One call a step makes, as the program sends it and a test expects it.
#[derive(Clone, Debug, PartialEq)]
pub enum Call {
    /// The body folds and proves it is safe before the skill says ready.
    PrepareUnplug,
    /// To the body's gaze socket, as the eyes send a face: its centre from
    /// the middle of the picture (-1 to 1) and its height as a share of it.
    /// Before a run it names no face.
    Gaze {
        face: Option<(f32, f32, f32)>,
    },
    /// A gesture by name, to the body's mood socket.
    Gesture(String),
    /// A move of the skill's own (`moves/NAME.json`), checked as the body
    /// checks it, sent whole to the body's mood socket and played there as
    /// a gesture.
    Move(lamp_body::moves::Move),
    /// A pose held on the body's mood socket for `lasting_us`, said again
    /// every second; the body comes back when it is no longer said. With
    /// `toward`, the reading pose's base is turned toward that place in the
    /// camera's picture (#1679).
    Mood {
        mood: String,
        lasting_us: u64,
        toward: Option<f32>,
        /// A search's pose (#1685): let go within a fifth of a second of the
        /// voice listening to someone, and the run with it.
        until_spoken: bool,
    },
    /// A colour, to the actions part's `light_colour`, then stepped to its
    /// level; held for its while in a run with no say. With `keep`, it is
    /// the person's light, set as they asked and never given back (#1679).
    Light {
        colour: String,
        level: Option<u8>,
        lasting: Option<String>,
        /// How it moves, asked of the ring with `LAMP_LIGHT_REQUESTS=on`.
        pattern: Option<LightPattern>,
        keep: bool,
        /// Held until the trigger stops holding, then given back (#1681:
        /// the pale breath of a lamp that is a little unwell).
        until_ends: bool,
    },
    /// A light some steps warmer and dimmer than the person's own as the
    /// run begins (#1672), made into a [`Call::Light`] by
    /// [`glow::relative`] then: with a breath, the light breathes from the
    /// person's level down `dimmer` steps and back; without, it is steady
    /// `dimmer` steps down. Always at least a level down, never up: at
    /// level 1 nothing is asked. Given back as any skill's light.
    Relative {
        warmer: u8,
        dimmer: u8,
        lasting: Option<String>,
        breathe_ms: Option<u32>,
    },
    /// A light show (`shows/NAME.toml`): each frame's light in turn, held
    /// its while, then the light given back (#1679). Only in a skill its
    /// tool alone starts: never on its own.
    Show {
        name: String,
        frames: Vec<Frame>,
    },
    /// Steps dimmer, to the actions part's `light_dimmer`; with
    /// `until_ends`, held until the trigger stops holding, then given back.
    Dim {
        steps: u8,
        until_ends: bool,
    },
    /// The light goes out (#1677): asked of the ring only
    /// (`LAMP_LIGHT_REQUESTS=on`) as the light shown, off, faded into over
    /// `over_ms` when given; with `until_ends`, held until the trigger stops
    /// holding, then given back, and the person's light comes back by itself.
    Out {
        over_ms: Option<u32>,
        until_ends: bool,
    },
    /// A note for the voice to say in its own words.
    Say(String),
    /// A skill's own words as its say table sets them out (#1684): said as
    /// written (`text`) or phrased by the model (`note`), at a priority, and
    /// spent if not begun within a while.
    Words(Words),
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
    Sound {
        name: String,
        path: PathBuf,
    },
    Cue,
    Rest(routine::Rest),
    /// Only a private offer is returned to Voice. Skills never dispatches it.
    RequestedAudio(requested_audio::Asset),
    /// The calls after this wait for the say to end: the skill's own say, or
    /// in a skill its tool starts, the line the model says after the call.
    AfterSay,
    /// A short text for the person's phone, on `messages/out`: only a
    /// watch's ([`watch`]), never a step.
    Text(String),
    /// An add-on's proposal (`docs/addons.md`), written on `addon/proposal`
    /// with its number stamped as it is sent: never an output by itself.
    Propose(Box<lamp_msgs::AddonProposal>),
    /// The trigger of a run that holds something until it ends stopped
    /// holding: what it held is given back. Alone in its run.
    Ends,
}

/// A say written as a table: `{ text = "..." }` or `{ note = "..." }`, with
/// its `priority` and `expires` (#1684).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Words {
    /// The words, or the note.
    pub words: String,
    /// `text`: the voice says them as they are written, never in words of
    /// the model's own (the say service's `text`). `note`: a note it phrases.
    pub exact: bool,
    /// The say service's priority, 0 to [`MOST_SAY_PRIORITY`].
    pub priority: u8,
    /// Not begun within this, it is spent.
    pub expires_ms: u64,
}

impl Words {
    /// The say service's ask, without the skill's name.
    pub fn ask(&self) -> Value {
        let mut ask = json!({ "priority": self.priority, "expires_in_ms": self.expires_ms });
        ask[self.key()] = json!(self.words);
        ask
    }

    /// `text` for exact words, `note` for a note.
    pub fn key(&self) -> &'static str {
        if self.exact { "text" } else { "note" }
    }
}

/// Where the words of a [`Call::SayFrom`] are kept, in the voice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The reminder Schedule has due (`schedule/due`).
    Schedule,
    /// The news of work handed to an agent (`tasks/status`).
    Tasks,
    /// A current invitation owned by the onboarding runner.
    Onboarding,
}

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Self::Schedule => "schedule",
            Self::Tasks => "tasks",
            Self::Onboarding => "onboarding",
        }
    }

    /// The topic its words are of, which a skill having the voice say them
    /// reads ([`Skill::reads`]): the person's reminder due, or the news of
    /// their work.
    pub fn topic(self) -> &'static str {
        match self {
            Self::Schedule => lamp_msgs::ScheduleDue::TOPIC,
            Self::Tasks => lamp_msgs::TaskStatus::TOPIC,
            Self::Onboarding => lamp_msgs::OnboardingPending::TOPIC,
        }
    }

    /// The say service's priority: a reminder at its time, after a second
    /// of quiet (8, as the voice's own reminders); news in a pause, after
    /// 15 s of quiet (5, as the voice's own news).
    pub fn priority(self) -> u8 {
        match self {
            Self::Schedule => 8,
            Self::Tasks | Self::Onboarding => 5,
        }
    }
}

impl Call {
    /// Whether it holds the light until its trigger stops holding.
    pub fn held_until_ends(&self) -> bool {
        matches!(
            self,
            Self::Light {
                until_ends: true,
                ..
            } | Self::Dim {
                until_ends: true,
                ..
            } | Self::Out {
                until_ends: true,
                ..
            }
        )
    }

    /// The call's name and its arguments, as a test names them.
    pub fn named(&self) -> (&'static str, Value) {
        match self {
            Self::PrepareUnplug => ("body.prepare_unplug", json!({})),
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
            Self::Mood {
                mood,
                lasting_us,
                toward,
                until_spoken,
            } => {
                let mut args = json!({ "mood": mood, "for_ms": lasting_us / 1_000 });
                if let Some(toward) = toward {
                    args["toward"] = json!(toward);
                }
                if *until_spoken {
                    args["until_spoken"] = json!(true);
                }
                ("body.mood", args)
            }
            Self::Light {
                colour,
                level,
                lasting,
                pattern,
                keep,
                until_ends,
            } => {
                let mut args = json!({ "colour": colour });
                if let Some(level) = level {
                    args["level"] = json!(level);
                }
                if let Some(lasting) = lasting {
                    args["for"] = json!(lasting);
                }
                if let Some(pattern) = pattern {
                    args["pattern"] = json!(pattern);
                }
                if *keep {
                    args["keep"] = json!(true);
                }
                if *until_ends {
                    args["until"] = json!("ends");
                }
                ("light", args)
            }
            Self::Show { name, frames } => {
                ("show", json!({ "show": name, "frames": frames.len() }))
            }
            Self::Dim { steps, until_ends } => {
                let mut args = json!({ "dim": steps });
                if *until_ends {
                    args["until"] = json!("ends");
                }
                ("light", args)
            }
            Self::Out {
                over_ms,
                until_ends,
            } => {
                let mut args = json!({ "out": true });
                if let Some(over_ms) = over_ms {
                    args["over_ms"] = json!(over_ms);
                }
                if *until_ends {
                    args["until"] = json!("ends");
                }
                ("light", args)
            }
            Self::Say(note) => ("say", json!({ "note": note })),
            Self::Words(words) => {
                let mut args =
                    json!({ "priority": words.priority, "expires_ms": words.expires_ms });
                args[words.key()] = json!(words.words);
                ("say", args)
            }
            Self::SayFrom(source, task) => {
                let mut args = json!({ "from": source.name() });
                if let Some(task) = task {
                    args[if *source == Source::Onboarding {
                        "invitation"
                    } else {
                        "task"
                    }] = json!(task);
                }
                ("say", args)
            }
            Self::Look => ("look", json!({})),
            Self::Sound { name, .. } => ("sound", json!({ "sound": name })),
            Self::Cue => ("sound", json!({ "cue": "hum" })),
            Self::Rest(rest) => ("rest", json!({ "op": rest.op })),
            Self::Relative {
                warmer,
                dimmer,
                lasting,
                breathe_ms,
            } => {
                let mut args = json!({ "relative": { "warmer": warmer, "dimmer": dimmer } });
                if let Some(lasting) = lasting {
                    args["for"] = json!(lasting);
                }
                // A case names the breath its light keeps (#3691: Glad's
                // and Lively's breaths, Low's steady light).
                if let Some(period_ms) = breathe_ms {
                    args["breathe_ms"] = json!(period_ms);
                }
                ("light", args)
            }
            Self::RequestedAudio(_) => ("requested_audio", json!({})),
            Self::AfterSay => ("after_say", json!({})),
            Self::Text(text) => ("text", json!({ "text": text })),
            Self::Propose(proposal) => {
                let mut args = json!(proposal);
                // Numbered as it is sent: a test names what it proposes.
                if let Some(args) = args.as_object_mut() {
                    args.remove("proposal");
                }
                ("propose", args)
            }
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

/// A field's path: names joined by dots, none empty (Jarvis on #1920: a
/// typo as `scores.` loaded and never held).
fn path(field: &str) -> Result<(), String> {
    if field.split('.').any(str::is_empty) {
        return Err(format!(
            "field {field:?}: names joined by dots, none of them empty"
        ));
    }
    Ok(())
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
    if let Some(field) = &field {
        path(field)?;
    }
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
        Some("changes") => When::Changes,
        Some(other) => {
            return Err(format!(
                "when = {other:?}: only \"appears\", \"present\" and \"changes\" are known"
            ));
        }
    };
    let changes = match (when, file.changes) {
        (When::Changes, Some(fields)) if (1..=MOST_CHANGE_FIELDS).contains(&fields.len()) => {
            for (index, field) in fields.iter().enumerate() {
                path(field)?;
                if field.len() > MOST_CHANGE_PATH_BYTES
                    || field.chars().any(char::is_control)
                    || fields[..index].contains(field)
                {
                    return Err(format!(
                        "changes: distinct field paths of at most {MOST_CHANGE_PATH_BYTES} bytes, without control characters"
                    ));
                }
            }
            fields
        }
        (When::Changes, _) => {
            return Err(format!(
                "when = \"changes\" requires changes with 1 to {MOST_CHANGE_FIELDS} field paths"
            ));
        }
        (_, Some(_)) => return Err("changes goes only with when = \"changes\"".into()),
        (_, None) => Vec::new(),
    };
    if when == When::Changes
        && (file.field.is_none()
            || file.each.is_some()
            || file.key.is_some()
            || file.near.is_some()
            || file.present.is_some()
            || file.absent_for.is_some()
            || file.lasting.is_some()
            || file.gap.is_some()
            || file.rearm.is_some()
            || file.cooldown.is_some()
            || file.wait.is_some()
            || file.rise.is_some()
            || file.during.is_some()
            || file.once_per_day
            || file.unless_heard
            || file.yields_to.is_some()
            || file.sooner.is_some())
    {
        return Err(
            "when = \"changes\" needs a field test, without each, key, near, present, absent_for, for, gap, rearm, cooldown, wait, rise, during, once_per_day, unless_heard, yields_to or sooner"
                .into(),
        );
    }
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
    let during = file
        .during
        .map(|window| daily::Window::parse(&window.from, &window.to))
        .transpose()
        .map_err(|error| format!("during: {error}"))?;
    if file.once_per_day && (during.is_none() || file.name.is_none() || file.each.is_some()) {
        return Err("once_per_day needs a named trigger with during, and no each".into());
    }
    if file.unless_heard && during.is_none() {
        return Err("unless_heard goes with during: heard since the window opened".into());
    }
    // An item's run is planned apart from the topic's, and hearing is
    // checked only for the topic's (the reviewer on #2664) (chosen, until a
    // skill needs both).
    if file.unless_heard && file.each.is_some() {
        return Err("unless_heard goes with a trigger on the whole topic, not each".into());
    }
    // A face that came is what waits for the other skill's greeting: the
    // chance is kept while it stays, as `wait` keeps one.
    let yields_to = match file.yields_to {
        None => None,
        Some(_) if when != When::Appears || file.each.is_some() => {
            return Err("yields_to goes with when = \"appears\" on the whole topic".into());
        }
        Some(to) => {
            if !plain_name(&to.skill) {
                return Err(format!(
                    "yields_to.skill {:?} is not a skill's name",
                    to.skill
                ));
            }
            if !plain_name(&to.trigger) || to.trigger.contains('-') {
                return Err(format!(
                    "yields_to.trigger {:?} is not a trigger's name",
                    to.trigger
                ));
            }
            Some((to.skill, to.trigger))
        }
    };
    let each = match (file.each, file.key) {
        (None, None) => None,
        (Some(list), Some(key)) if !list.is_empty() && !key.is_empty() => {
            path(&list)?;
            path(&key)?;
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
    let rise = match file.rise {
        None => None,
        Some(rise) => {
            let Some(field) = file.field.clone() else {
                return Err("rise is of a field: name it".into());
            };
            if file.equals.is_some() || file.below.is_some() || each.is_some() {
                return Err(
                    "rise goes with a field and, for a level too, above: not with equals, below or each"
                        .into(),
                );
            }
            if !(rise.by.is_finite() && rise.by > 0.0) {
                return Err("rise.by is a number above 0".into());
            }
            let within_us = duration_us(&rise.within)?;
            if !(1_000_000..=MOST_WAIT_US).contains(&within_us) {
                return Err("rise.within is 1s to 1h".into());
            }
            Some(Rise {
                field,
                by: rise.by,
                within_us,
            })
        }
    };
    let mut test = match file.present {
        None => match (&rise, &file.field, file.above) {
            (Some(_), Some(field), None) => {
                path(field)?;
                Test::Number(field.clone())
            }
            _ => test(&file.topic, file.field, file.equals, file.above, file.below)?,
        },
        Some(present)
            if file.topic == Faces::TOPIC
                && file.field.is_none()
                && file.equals.is_none()
                && file.above.is_none()
                && file.below.is_none() =>
        {
            if present {
                Test::Faces(0.0)
            } else {
                Test::NoFaces
            }
        }
        Some(_) => return Err("present is only for a face trigger without a field test".into()),
    };
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
    let for_us = optional(&file.lasting)?;
    let sooner = match file.sooner {
        None => None,
        Some(sooner) => {
            if for_us == 0 {
                return Err("sooner shortens a trigger's for: give the trigger a for".into());
            }
            if !sooner.above.is_finite() {
                return Err("sooner.above is a number".into());
            }
            let sooner_us = duration_us(&sooner.lasting)?;
            if sooner_us == 0 || sooner_us >= for_us {
                return Err("sooner.for is shorter than the trigger's for, and above 0".into());
            }
            Some(Sooner {
                test: crate::test(
                    &sooner.topic,
                    Some(sooner.field),
                    None,
                    Some(sooner.above),
                    None,
                )?,
                topic: sooner.topic,
                for_us: sooner_us,
            })
        }
    };
    // A compound comparison can turn false because a selected child is
    // malformed, incorrectly rearming its unchanged valid scalar tuple.
    if when == When::Changes && matches!(&test, Test::Equals(_, Value::Array(_) | Value::Object(_)))
    {
        return Err("when = \"changes\" needs a scalar comparison, not an array or object".into());
    }
    Ok(Trigger {
        test,
        topic: file.topic,
        name: file.name,
        each,
        when,
        changes,
        absent_for_us: optional(&file.absent_for)?,
        for_us,
        gap_us: optional(&file.gap)?,
        rearm,
        rearm_line,
        cooldown_us: optional(&file.cooldown)?,
        wait_us,
        rise,
        during,
        once_per_day: file.once_per_day,
        unless_heard: file.unless_heard,
        yields_to,
        sooner,
    })
}

/// One skill folder, or why it cannot be loaded.
pub fn load(dir: &Path) -> Result<Skill, String> {
    let read =
        |file: &str| fs::read_to_string(dir.join(file)).map_err(|error| format!("{file}: {error}"));
    let description_text = read("SKILL.md")?;
    let (name, description) = front_matter(&description_text)?;
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
    let manifest_text = read("skill.toml")?;
    let mut file: SkillFile =
        toml::from_str(&manifest_text).map_err(|error| format!("skill.toml: {error}"))?;
    match file.format {
        None | Some(1) => {}
        Some(2) => file.permissions.format_2 = true,
        Some(other) => {
            return Err(format!(
                "skill.toml: format {other} is not one this lamp reads (1 or 2)"
            ));
        }
    }
    let kind = SkillKind::read(file.kind.as_deref())?;
    file.permissions.check_core(kind)?;
    file.permissions.check_say(kind)?;
    if kind == SkillKind::Addon {
        addon_file(&file, file.trigger.is_none())?;
    } else if !file.permissions.propose.is_empty() {
        return Err("[permissions] propose is an add-on's: kind = \"addon\"".into());
    }
    if file.steps.iter().any(|step| step.requested_audio.is_some()) {
        let only_say = |step: &StepFile| {
            matches!(&step.say, Some(toml::Value::String(note)) if !note.trim().is_empty() && note.len() <= requested_audio::MOST_CONFIRMATION_BYTES && !note.chars().any(char::is_control))
                && step.body.is_none()
                && step.light.is_none()
                && step.sound.is_none()
                && step.cue.is_none()
                && step.rest.is_none()
                && step.look.is_none()
                && step.requested_audio.is_none()
                && step.after.is_none()
                && step.on.is_none()
                && step.if_choice.is_none()
                && step.show.is_none()
                && step.propose.is_none()
        };
        let only_audio = |step: &StepFile| {
            step.requested_audio.is_some()
                && step.after.as_deref() == Some("say")
                && step.body.is_none()
                && step.light.is_none()
                && step.sound.is_none()
                && step.cue.is_none()
                && step.rest.is_none()
                && step.look.is_none()
                && step.say.is_none()
                && step.on.is_none()
                && step.if_choice.is_none()
                && step.show.is_none()
                && step.propose.is_none()
        };
        if file.tool.is_none()
            || file.trigger.is_some()
            || file.ask.is_some()
            || !file.conditions.is_empty()
            || file.choice.is_some()
            || file.flag.is_some()
            || file.limits.is_some()
            || file.history.is_some()
            || file.most_a_day.is_some()
            || file.active.is_some()
            || file.budget.is_some()
            || file.rest.is_some()
            || file.search.is_some()
            || file.stop.is_some()
            || file.watch.is_some()
            || file.unplug.is_some()
            || file.steps.len() != 2
            || !only_say(&file.steps[0])
            || !only_audio(&file.steps[1])
        {
            return Err("requested_audio needs a tool alone, one short say confirmation then one requested_audio with after = \"say\", and no other actions, limits, triggers, conditions, arguments, history, search or watch behavior".into());
        }
    }
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
    // A watch is turned on and off by its tool alone, and does what its
    // `[watch]` says: no trigger and no steps ([`watch`]).
    if file.watch.is_some() {
        if !triggers.is_empty() {
            return Err(
                "a watch is turned on only by its tool, when a person asks: never on its own, so no trigger"
                    .into(),
            );
        }
        if file.tool.is_none() {
            return Err("a watch is turned on and off by its tool: it needs one".into());
        }
        if !file.steps.is_empty() {
            return Err("a watch has no [[do]]: what it does is its [watch]".into());
        }
        if file.flag.is_some()
            || file.again.is_some()
            || file.still.is_some()
            || file.quiet.is_some()
            || !file.conditions.is_empty()
            || file.ask.is_some()
        {
            return Err(
                "a watch takes no [flag], [again], still, [quiet], [[while]] or [ask]".into(),
            );
        }
        match &file.choice {
            Some(choice)
                if choice.default.is_none() && {
                    let mut values = choice.one_of.clone();
                    values.sort();
                    values == ["off", "on"]
                } => {}
            _ => {
                return Err(
                    "a watch's tool has one [choice] of exactly \"on\" and \"off\", with no default"
                        .into(),
                );
            }
        }
    } else if file.permissions.grants("startle") {
        // The startle (#2822) is Expression's reflex: its move is this
        // file's `[startle]`, and lamp-skills never runs it.
        match &file.startle {
            Some(startle) => {
                startle.checked().map_err(|error| error.0)?;
            }
            None => return Err("a skill granted core = [\"startle\"] needs [startle]".into()),
        }
        if !triggers.is_empty()
            || file.tool.is_some()
            || !file.steps.is_empty()
            || file.ask.is_some()
            || !file.conditions.is_empty()
        {
            return Err(
                "a skill granted core = [\"startle\"] is Expression's reflex: no trigger, tool, [ask], [[while]] or [[do]]"
                    .into(),
            );
        }
    } else if file.startle.is_some() {
        return Err("[startle] belongs only to a skill granted core = [\"startle\"]".into());
    } else if (triggers.is_empty() && file.tool.is_none()) || file.steps.is_empty() {
        return Err("skill.toml needs a trigger or a tool, and at least one [[do]]".into());
    }
    let watch = file.watch.map(watch::spec).transpose()?;
    let policy = file
        .ask
        .as_ref()
        .map(|ask| ask.policy.clone())
        .unwrap_or_default();
    // An add-on asks nobody here: each proposal goes to its judge. Nor
    // does the night's sleep (#940): it is the creature's own.
    let sleep = file.permissions.grants("sleep");
    if sleep && file.ask.is_some() {
        return Err("a skill granted core = [\"sleep\"] asks nobody: no [ask]".into());
    }
    if !triggers.is_empty() && policy.is_empty() && kind != SkillKind::Addon && !sleep {
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
        if tool == unplug::STAY_TOOL {
            return Err(format!(
                "tool {tool:?} is reserved for the unplug cancel of prepare-unplug"
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
        match &choice.field {
            None => argument(&choice.name, &choice.about)?,
            Some(field) => {
                if file.tool.is_some() || triggers.is_empty() || choice.default.is_some() {
                    return Err(
                        "a choice read from a field is a trigger's: a skill with a trigger, no tool and no default"
                            .into(),
                    );
                }
                if !plain_name(&choice.name) || field.is_empty() {
                    return Err(format!(
                        "choice {:?}: a plain name, and the trigger's field it is read from",
                        choice.name
                    ));
                }
            }
        }
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
        if let Some(default) = &choice.default
            && !choice.one_of.contains(default)
        {
            return Err(format!(
                "choice {:?}: its default {default:?} must be one of {:?}",
                choice.name, choice.one_of
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
        let alone = condition.topic == Faces::TOPIC
            && condition.field.is_none()
            && condition.equals.is_none()
            && condition.not_equals.is_none()
            && condition.above.is_none()
            && condition.below.is_none()
            && condition.present.is_none();
        let test = match (condition.present, condition.not_equals) {
            _ if condition.faces_at_most.is_some() => match condition.faces_at_most {
                Some(most @ 1..=8) if alone => Test::FacesAtMost(most as usize),
                _ => {
                    return Err(format!(
                        "faces_at_most, 1 to 8, is known only on {}, alone",
                        Faces::TOPIC
                    ));
                }
            },
            (None, Some(unwanted)) => match (
                condition.field,
                condition.equals,
                condition.above,
                condition.below,
            ) {
                (Some(field), None, None, None) => Test::NotEquals(
                    {
                        path(&field)?;
                        field
                    },
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
        let unknown_holds = match condition.unknown.as_deref() {
            None => false,
            Some("holds") if condition.lasting.is_none() && condition.present.is_none() => true,
            Some("holds") => {
                return Err(format!(
                    "on {}: unknown = \"holds\" goes with a field's test, without for or present",
                    condition.topic
                ));
            }
            Some(other) => {
                return Err(format!(
                    "on {}: unknown is \"holds\" or left out, not {other:?}",
                    condition.topic
                ));
            }
        };
        let through = match condition.through {
            None => Vec::new(),
            Some(_) if !matches!(test, Test::Equals(..)) || condition.lasting.is_none() => {
                return Err(format!(
                    "on {}: through goes with a field's equals and a for",
                    condition.topic
                ));
            }
            Some(values) if values.is_empty() => {
                return Err(format!(
                    "on {}: through lists at least one value",
                    condition.topic
                ));
            }
            Some(values) => {
                let values: Vec<Value> = values
                    .into_iter()
                    .map(|value| serde_json::to_value(value).map_err(|error| error.to_string()))
                    .collect::<Result<_, _>>()?;
                // A value it is carried through never holds (reviewer-two's
                // P3 on #3092).
                if let Test::Equals(_, wanted) = &test
                    && values.iter().any(|value| equal(value, wanted))
                {
                    return Err(format!(
                        "on {}: through lists values other than the one it equals",
                        condition.topic
                    ));
                }
                values
            }
        };
        let broken_by = match condition.broken_by {
            None => Vec::new(),
            Some(_) if through.is_empty() => {
                return Err(format!(
                    "on {}: broken_by goes with through: it begins again the clock through carries",
                    condition.topic
                ));
            }
            Some(topics) => {
                if let Some(other) = topics
                    .iter()
                    .find(|topic| !BROKEN_BY.contains(&topic.as_str()))
                {
                    return Err(format!(
                        "on {}: broken_by takes {BROKEN_BY:?}, not {other:?}",
                        condition.topic
                    ));
                }
                topics
            }
        };
        // An invalid selected value must not reset a while's clock and
        // rearm the unchanged tuple. Keep those inputs independent instead
        // of introducing a second set of condition clocks for Changes.
        for trigger in triggers
            .iter()
            .filter(|trigger| trigger.when == When::Changes && trigger.topic == condition.topic)
        {
            let field = match &test {
                Test::Equals(field, _)
                | Test::NotEquals(field, _)
                | Test::Above(field, _)
                | Test::Below(field, _)
                | Test::Number(field) => field,
                Test::Faces(_) | Test::NoFaces | Test::FacesAtMost(_) => continue,
            };
            if let Some(selected) = trigger.changes.iter().find(|selected| {
                selected.as_str() == field.as_str()
                    || selected
                        .strip_prefix(field.as_str())
                        .is_some_and(|rest| rest.starts_with('.'))
                    || field
                        .strip_prefix(selected.as_str())
                        .is_some_and(|rest| rest.starts_with('.'))
            }) {
                return Err(format!(
                    "when = \"changes\": selected field {selected:?} overlaps a while field {field:?} on {}",
                    condition.topic
                ));
            }
        }
        conditions.push(Condition {
            topic: condition.topic,
            test,
            for_us: condition.lasting.as_deref().map_or(Ok(0), duration_us)?,
            unknown_holds,
            through,
            broken_by,
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
    // The nudges' limits (#1685): each on what the lamp starts itself, so
    // only in a skill with a trigger, and not with an each trigger, whose
    // items' runs are news, not nudges (chosen).
    let limited = file.most_a_day.is_some()
        || file.active.is_some()
        || file.budget.is_some()
        || file.rest.is_some();
    if limited && (triggers.is_empty() || triggers.iter().any(|trigger| trigger.each.is_some())) {
        return Err(
            "most_a_day, [active], [budget] and [rest] limit what a skill starts itself: in a skill with a trigger, not an each trigger"
                .into(),
        );
    }
    if file
        .most_a_day
        .is_some_and(|most| !(1..=MOST_A_DAY_AT_MOST).contains(&most))
    {
        return Err(format!("most_a_day is 1 to {MOST_A_DAY_AT_MOST}"));
    }
    let active = match file.active {
        None => None,
        Some(active) => {
            let (from, to) = (minute_of_day(&active.from)?, minute_of_day(&active.to)?);
            if from == to {
                return Err("active: from and to are different times".into());
            }
            Some((from, to))
        }
    };
    let budget = match file.budget {
        None => None,
        Some(budget) => {
            if !plain_name(&budget.group) || budget.group.contains('_') {
                return Err(format!(
                    "[budget] group {:?}: lowercase letters, digits and hyphens",
                    budget.group
                ));
            }
            if !(1..=PER_HOUR_AT_MOST).contains(&budget.per_hour) {
                return Err(format!("[budget] per_hour is 1 to {PER_HOUR_AT_MOST}"));
            }
            Some((budget.group, budget.per_hour))
        }
    };
    let rest = match file.rest {
        None => None,
        Some(rest) => {
            if !(1..=5).contains(&rest.after_ignored) {
                return Err("[rest] after_ignored is 1 to 5".into());
            }
            let within_us = rest
                .within
                .as_deref()
                .map_or(Ok(NOTICED_WITHIN_US), duration_us)?;
            if !(10_000_000..=300_000_000).contains(&within_us) {
                return Err("[rest] within is 10s to 5m".into());
            }
            Some(Rest {
                after_ignored: rest.after_ignored,
                within_us,
            })
        }
    };
    // What a person stops is what the lamp starts itself, so only in a
    // skill with a trigger, and not with an each trigger, whose items' runs
    // are news they asked for (chosen, as `[quiet]` is not with one).
    let stop = match file.stop {
        None => None,
        Some(_) if triggers.is_empty() || triggers.iter().any(|trigger| trigger.each.is_some()) => {
            return Err(
                "[stop] stops what a skill starts itself: in a skill with a trigger, not an each trigger"
                    .into(),
            );
        }
        Some(stop) => {
            if !function_name(&stop.tool) {
                return Err(format!(
                    "[stop] tool {:?}: a lowercase letter, then at most 47 lowercase letters, digits and _",
                    stop.tool
                ));
            }
            if file.tool.as_deref() == Some(stop.tool.as_str()) {
                return Err("[stop] tool is a function of its own, not the skill's tool".into());
            }
            if stop.about.trim().is_empty()
                || stop.about.len() > STOP_ABOUT_MOST_BYTES
                || stop.about.chars().any(char::is_control)
            {
                return Err(format!(
                    "[stop] about is 1 to {STOP_ABOUT_MOST_BYTES} bytes, with no control characters: when the model calls it"
                ));
            }
            Some(Stop {
                tool: stop.tool,
                about: stop.about,
            })
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
    // A tool alone starts it: a person asked, so what it does may be kept
    // or played as they asked, never on its own (#1679).
    let asked_alone = file.tool.is_some() && triggers.is_empty();
    let mut steps = Vec::new();
    // The runs each step is for, beside it: one for each step made.
    let mut ons: Vec<Option<Only>> = Vec::new();
    let mut said = false;
    let mut after_say = false;
    for step in file.steps {
        if step.body.is_none()
            && step.light.is_none()
            && step.say.is_none()
            && step.sound.is_none()
            && step.cue.is_none()
            && step.rest.is_none()
            && step.requested_audio.is_none()
            && step.look.is_none()
            && step.show.is_none()
            && step.propose.is_none()
        {
            return Err(
                "a [[do]] step names body, light, say, sound, cue, rest, requested_audio, look, show or propose"
                    .into(),
            );
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
        let on = match (step.on.clone(), &step.if_choice) {
            (Some(_), Some(_)) => {
                return Err(
                    "a step names its trigger (on) or its choice (if_choice), not both".into(),
                );
            }
            (Some(on), None) => Some(Only::Trigger(on)),
            (None, Some(values)) => {
                let Some(choice) = &file.choice else {
                    return Err("if_choice needs a [choice]".into());
                };
                if values.is_empty()
                    || values.iter().any(|value| !choice.one_of.contains(value))
                    || (1..values.len()).any(|at| values[..at].contains(&values[at]))
                {
                    return Err(format!(
                        "if_choice lists values of the choice, each once: {:?}",
                        choice.one_of
                    ));
                }
                // One note to the model a call: what it says is the same for
                // every choice.
                if step.say.is_some()
                    || step.sound.is_some()
                    || step.cue.is_some()
                    || step.rest.is_some()
                    || step.requested_audio.is_some()
                    || step.look.is_some()
                {
                    return Err("a step for some choices only makes body, light or show".into());
                }
                Some(Only::Choice(values.clone()))
            }
            (None, None) => None,
        };
        match step.after.as_deref() {
            None => {}
            Some("say") if after_say => {}
            // The skill's say in an earlier step, or the model's line.
            Some("say") if (said || file.tool.is_some()) && step.say.is_none() => {
                after_say = true;
                steps.push(Step::Call(Call::AfterSay));
                // The wait for the say is every run's, whatever this step's
                // own choice: the steps after it wait in each (#1679).
                ons.push(None);
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
            if let Some(prepare) = body.prepare_unplug {
                if !prepare || file.tool.is_none() || !triggers.is_empty() {
                    return Err("prepare_unplug = true needs a tool and no trigger".into());
                }
                if !file.permissions.grants("prepare_unplug") {
                    return Err(
                        "prepare_unplug belongs only to a core skill granted core = [\"prepare_unplug\"] in [permissions]".into(),
                    );
                }
                steps.push(Step::Call(Call::PrepareUnplug));
            }
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
                (None, None) if body.toward.is_some() => {
                    return Err(
                        "toward turns the reading pose: it goes with mood = \"reading\"".into(),
                    );
                }
                (None, None) => {}
                (Some(mood), Some(lasting)) => {
                    if !MOODS.contains(&mood.as_str()) {
                        return Err(format!("mood {mood:?} is not one of {MOODS:?}"));
                    }
                    let lasting_us = duration_us(&lasting)?;
                    if lasting_us == 0 || lasting_us > MOOD_AT_MOST_US {
                        return Err("a mood's for is more than 0 and at most 2m".into());
                    }
                    if body
                        .toward
                        .is_some_and(|toward| !toward.is_finite() || toward.abs() > 1.0)
                    {
                        return Err("toward is a place across the picture, -1 to 1".into());
                    }
                    steps.push(Step::Call(Call::Mood {
                        mood,
                        lasting_us,
                        toward: body.toward,
                        // A search's poses end when someone speaks (#1685).
                        until_spoken: file.search.is_some(),
                    }));
                }
                _ => return Err("a body mood goes with its for, and a for with a mood".into()),
            }
        }
        if let Some(light) = step.light {
            let with_tool = file.tool.is_some();
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
            let call = light_call(file).map_err(|why| format!("{place}: {why}"))?;
            // Made against the light shown as an automatic run begins.
            if matches!(call, Call::Relative { .. }) && with_tool {
                return Err(format!(
                    "{place}: relative is only for a skill its triggers start, without a tool"
                ));
            }
            if matches!(call, Call::Light { keep: true, .. }) && !asked_alone {
                return Err(format!(
                    "{place}: keep = true only in a skill started by its tool alone: only a light a person asked for is theirs to keep"
                ));
            }
            steps.push(Step::Call(call));
        }
        if let Some(show) = step.show {
            if !asked_alone {
                return Err(
                    "show only in a skill started by its tool alone: a light show is played only when a person asks"
                        .into(),
                );
            }
            if !plain_name(&show) {
                return Err(format!("show {show:?} is not a file name in shows/"));
            }
            let place = format!("shows/{show}.toml");
            let file: ShowFile =
                toml::from_str(&read(&place)?).map_err(|error| format!("{place}: {error}"))?;
            let frames = show_frames(file).map_err(|why| format!("{place}: {why}"))?;
            steps.push(Step::Call(Call::Show { name: show, frames }));
        }
        if let Some(say) = step.say {
            let call = say_step(say)?;
            if matches!(call, Call::SayFrom(Source::Onboarding, _)) {
                let canonical = file.permissions.grants("onboarding")
                    && conditions.is_empty()
                    && triggers.len() == 1
                    && triggers[0].test == Test::Equals("ready".into(), json!(true))
                    && triggers[0].topic == lamp_msgs::OnboardingPending::TOPIC
                    && triggers[0]
                        .each
                        .as_ref()
                        .is_some_and(|(list, key)| list == "invitations" && key == "invitation");
                if !canonical {
                    return Err("say.from onboarding belongs only to a core onboarding skill, granted core = [\"onboarding\"] in [permissions], on its invitation trigger".into());
                }
            }
            // A tool's run tells the model what to say, and what the voice
            // keeps is not the model's to say (Fourth on #1033).
            if matches!(call, Call::SayFrom(..)) && file.tool.is_some() {
                return Err(
                    "say.from, in a skill with a tool: what the voice keeps is said when a trigger starts the skill, never from the model's call"
                        .into(),
                );
            }
            // A tool's say is the note for the model's own answer, said in
            // its turn: there is no say of the skill's to word or rank.
            if matches!(call, Call::Words(_)) && file.tool.is_some() {
                return Err(
                    "say as a table of text or note, in a skill with a tool: the model answers the call in its own turn, so its say is a note"
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
        if let Some(cue) = step.cue {
            if cue != "hum" || file.tool.is_some() {
                return Err("cue is hum, only in a skill without a tool".into());
            }
            steps.push(Step::Call(Call::Cue));
        }
        if let Some(rest) = step.rest {
            if file.tool.is_some() || triggers.iter().any(|trigger| trigger.each.is_some()) {
                return Err("rest is only for automatic, non-item triggers".into());
            }
            steps.push(Step::Call(Call::Rest(rest.checked()?)));
        }
        if let Some(propose) = step.propose {
            if kind != SkillKind::Addon {
                return Err("propose is an add-on's step: kind = \"addon\"".into());
            }
            let version = file.version.clone().unwrap_or_default();
            steps.push(Step::Call(Call::Propose(Box::new(proposal(
                propose, &name, &version,
            )?))));
        }
        if let Some(relative) = step.requested_audio {
            let asset = requested_audio::Asset::load(dir, &relative)?;
            if !asset.matches_definition(&description_text, &manifest_text) {
                return Err("requested audio definition changed while loading".into());
            }
            steps.push(Step::Call(Call::RequestedAudio(asset)));
        }
        ons.resize(steps.len(), on);
    }
    let steps: Vec<(Option<Only>, Step)> = ons.into_iter().zip(steps).collect();
    // A kept light is the person's: a light given back, a dim, an out or a
    // show in the same skill would give back the light from before the run
    // over it at the run's end (the review on #1730).
    let kept = steps
        .iter()
        .any(|(_, step)| matches!(step, Step::Call(Call::Light { keep: true, .. })));
    if kept
        && steps.iter().any(|(_, step)| {
            matches!(
                step,
                Step::Call(
                    Call::Light { keep: false, .. }
                        | Call::Dim { .. }
                        | Call::Out { .. }
                        | Call::Show { .. }
                )
            )
        })
    {
        return Err(
            "keep = true goes with no light given back, dim, out or show in the same skill: its run would give back the light from before it over the one kept"
                .into(),
        );
    }
    if steps
        .iter()
        .any(|(_, step)| *step == Step::Call(Call::PrepareUnplug))
        && (steps.len() != 1 || file.choice.is_some() || file.flag.is_some())
    {
        return Err("prepare_unplug is the skill's only step, without choice or flag".into());
    }
    // One say or sound a run: for each trigger, its own steps and those for
    // all. A step for some choices makes none.
    let says = |name: Option<&String>| {
        steps
            .iter()
            .filter(|(on, step)| {
                matches!(
                    step,
                    Step::Call(
                        Call::Say(_)
                            | Call::Words(_)
                            | Call::SayFrom(..)
                            | Call::Sound { .. }
                            | Call::Cue
                    )
                ) && match on {
                    None => true,
                    Some(Only::Trigger(on)) => Some(on) == name,
                    Some(Only::Choice(_)) => false,
                }
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
        && steps
            .iter()
            .any(|(_, step)| matches!(step, Step::Call(call) if call.held_until_ends()))
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
        if steps
            .iter()
            .any(|(_, step)| matches!(step, Step::Call(call) if call.held_until_ends()))
        {
            return Err(
                "until = \"ends\" in a skill with an each trigger: an item's run has no end".into(),
            );
        }
    }
    if file.limits.is_some() {
        limits::source(&name)?;
        if let Some(why) = file.permissions.refuses_output_limits() {
            return Err(format!("not permitted: {why}"));
        }
    }
    if let Some(version) = &file.version
        && !version_ok(version)
    {
        return Err(format!(
            "version = {version:?}: MAJOR.MINOR.PATCH, three whole numbers (\"1.0.0\")"
        ));
    }
    file.permissions.check()?;
    // A search is a person's ask (#1685): a tool alone starts it, each call
    // is a look, and its first is at the choice's default (the desk).
    let search = match file.search {
        None => None,
        Some(search) => {
            if !asked_alone
                || !steps
                    .iter()
                    .any(|(_, step)| *step == Step::Call(Call::Look))
            {
                return Err(
                    "[search] is a tool's looks: in a skill started by its tool alone, with look = true"
                        .into(),
                );
            }
            if file
                .choice
                .as_ref()
                .is_none_or(|choice| choice.default.is_none())
            {
                return Err(
                    "[search] begins at its choice's default: a [choice] with a default".into(),
                );
            }
            if !(1..=SEARCH_LOOKS_AT_MOST).contains(&search.looks) {
                return Err(format!(
                    "[search] looks is 1 to {SEARCH_LOOKS_AT_MOST}: a few places, then it lets it go"
                ));
            }
            if search.give_up.trim().is_empty() || search.give_up.len() > 256 {
                return Err(
                    "[search] give_up is what the model is told, in at most 256 bytes".into(),
                );
            }
            if let Some(found) = &search.found {
                if found.trim().is_empty() || found.len() > 256 {
                    return Err(
                        "[search] found is what the model is told, in at most 256 bytes".into(),
                    );
                }
                if file
                    .choice
                    .as_ref()
                    .is_none_or(|choice| !choice.one_of.iter().any(|one| one == FOUND))
                {
                    return Err(format!(
                        "[search] found is called with the choice {FOUND:?}: one_of names it"
                    ));
                }
            }
            if let Some(note) = &search.note
                && (note.trim().is_empty() || note.len() > 256)
            {
                return Err("[search] note is what the model is told, in at most 256 bytes".into());
            }
            if !search.walk.is_empty() && search.note.is_none() {
                return Err("[search] walk goes on after its note: a note with it".into());
            }
            let places = file
                .choice
                .as_ref()
                .map(|choice| choice.one_of.as_slice())
                .unwrap_or_default();
            let is_place = |place: &String| place != FOUND && places.contains(place);
            for (at, place) in search.walk.iter().enumerate() {
                if !is_place(place) || search.walk[..at].contains(place) {
                    return Err(format!(
                        "[search] walk names each place once, from its choice's one_of: {place:?}"
                    ));
                }
            }
            for (place, words) in &search.looked {
                if !is_place(place) || words.trim().is_empty() || words.len() > LOOKED_AT_MOST {
                    return Err(format!(
                        "[search] looked says a place of its choice's one_of in at most {LOOKED_AT_MOST} bytes: {place:?}"
                    ));
                }
            }
            let default = file
                .choice
                .as_ref()
                .and_then(|choice| choice.default.as_ref());
            if !search.walk.is_empty()
                && let Some(place) = search
                    .walk
                    .iter()
                    .chain(default)
                    .find(|place| !search.looked.contains_key(*place))
            {
                return Err(format!(
                    "[search] looked says each place of its walk and its default: {place:?}"
                ));
            }
            Some(Search {
                looks: search.looks,
                give_up: search.give_up,
                found: search.found,
                note: search.note,
                walk: search.walk,
                looked: search.looked,
            })
        }
    };
    // Who was at the lamp (#1685): a person asks, so a tool alone starts
    // it, and it reads `room/presence` alone, which carries times only.
    let history = match file.history {
        None => None,
        Some(history) => {
            if file.tool.is_none() || !triggers.is_empty() {
                return Err(
                    "[history] is told to a tool's call: in a skill started by its tool alone"
                        .into(),
                );
            }
            if history.topic != RoomPresence::TOPIC {
                return Err(format!(
                    "[history] topic is {}: who was at the lamp, times only",
                    RoomPresence::TOPIC
                ));
            }
            let within_us = duration_us(&history.within)?;
            if !(60_000_000..=visits::KEPT_US).contains(&within_us) {
                return Err("[history] within is 1m to 24h: what the lamp keeps".into());
            }
            Some(visits::History {
                within_us,
                built_in_words: history.words.is_none(),
                words: history
                    .words
                    .map_or_else(|| Ok(visits::Words::built_in()), visits::Words::checked)?,
            })
        }
    };
    // The words the voice says for a `say = { from = ... }` are the skill
    // whose say it is (#940): checked here as the voice checks them, and
    // only in a skill with that say.
    let says_from = |source: Source| {
        steps
            .iter()
            .any(|(_, step)| matches!(step, Step::Call(Call::SayFrom(from, _)) if *from == source))
    };
    if let Some(news) = file.news {
        if !says_from(Source::Tasks) {
            return Err(
                "[news] is the words of a say = { from = \"tasks\" }: only with one".into(),
            );
        }
        news.words.checked()?;
    }
    if let Some(reminder) = file.reminder {
        if !says_from(Source::Schedule) {
            return Err(
                "[reminder] is the words of a say = { from = \"schedule\" }: only with one".into(),
            );
        }
        reminder.words.checked()?;
    }
    match (file.onboarding, file.permissions.grants("onboarding")) {
        (Some(definition), true) => {
            if manifest_text.len() as u64 > lamp_voice::onboarding::skill::MOST_MANIFEST_BYTES {
                return Err("onboarding skill manifest exceeds 65536 bytes".into());
            }
            definition.checked()?;
        }
        (None, true) => return Err("a core onboarding skill requires [onboarding] data".into()),
        (Some(_), false) => {
            return Err("[onboarding] data belongs only to a core onboarding skill".into());
        }
        (None, false) => {}
    }
    // The words are the grant's: a skill granted it says them all, and no
    // other skill has them (#940).
    let unplug = match (file.unplug, file.permissions.grants("prepare_unplug")) {
        (Some(lines), true) => Some(lines.checked()?),
        (Some(_), false) => {
            return Err(
                "[unplug] is what a skill granted core = [\"prepare_unplug\"] says: only with that grant"
                    .into(),
            );
        }
        // A file from before `[unplug]` says the built-in words, for this
        // release only ([`UnplugLines::built_in`], [`unplug_words`]).
        (None, _) => None,
    };
    // The night's sleep is its rest alone: nothing a person sees or hears
    // comes from it beyond what Expression's sleep does (#940).
    if file.permissions.grants("sleep")
        && (file.tool.is_some()
            || steps
                .iter()
                .any(|(_, step)| !matches!(step, Step::Call(Call::Rest(_)))))
    {
        return Err("a skill granted core = [\"sleep\"] has only rest steps and no tool".into());
    }
    let skill = Skill {
        name,
        description,
        enabled: file.enabled,
        limits: file.limits.map(routine::LimitsFile::checked).transpose()?,
        needs: file.needs,
        kind,
        version: file.version,
        permissions: file.permissions,
        triggers,
        conditions,
        policy,
        tool: file.tool,
        choice: file.choice,
        flag: file.flag,
        again,
        still,
        search,
        history,
        quiet,
        most_a_day: file.most_a_day,
        active,
        budget,
        rest,
        stop,
        steps,
        watch,
        unplug,
    };
    // A watch shows a faint mark while it is on, texts the person, and
    // tells a known face from one it does not know: each needs its key.
    if let Some(watch) = &skill.watch {
        for (given, why) in [
            (
                skill.permissions.light,
                "a watch shows its mark while on, which needs light = true",
            ),
            (
                skill.permissions.messages,
                "a watch texts the person, which needs messages = true",
            ),
            (
                skill.permissions.known_faces,
                "a watch tells a known face from one it does not know, which needs known_faces = true",
            ),
        ] {
            if !given {
                return Err(format!("not permitted: {why} in [permissions]"));
            }
        }
        if let Some(why) = skill.permissions.refuses(&watch.mark) {
            return Err(format!("not permitted: {why}"));
        }
    }
    // Least privilege: every read and every step within what it declares,
    // or it is not loaded, and the journal says what it lacks.
    for (topic, by) in skill.reads() {
        if let Some(why) = skill.permissions.refuses_reading(&topic) {
            return Err(format!("not permitted: {by} reads {topic}, and {why}"));
        }
        if skill.kind == SkillKind::Addon && !lamp_msgs::ADDON_TOPICS.contains(&topic.as_str()) {
            return Err(format!(
                "not permitted: {by} reads {topic}, and an add-on reads only {:?}",
                lamp_msgs::ADDON_TOPICS
            ));
        }
    }
    for (_, step) in &skill.steps {
        // A gesture's clip is picked as it runs; any clip needs the body.
        let call = match step {
            Step::Call(call) => call.clone(),
            Step::Gesture { .. } => Call::Gesture(String::new()),
        };
        if let Some(why) = skill.permissions.refuses(&call) {
            return Err(format!("not permitted: {why}"));
        }
    }
    Ok(skill)
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
    at(value, list)
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| Some((item_name(at(item, key))?, trigger.test.holds(item))))
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
    if let Some(relative) = file.relative {
        if file.colour.is_some()
            || file.level.is_some()
            || file.dim.is_some()
            || file.out.is_some()
            || file.over.is_some()
            || file.until.is_some()
            || file.keep.is_some()
        {
            return Err(
                "relative takes only for and a breathe pattern: its colour and level are the person's light, stepped"
                    .into(),
            );
        }
        let warmer = relative.warmer.unwrap_or(0);
        let dimmer = relative.dimmer.unwrap_or(0);
        if warmer > 1 || dimmer > 4 || warmer + dimmer == 0 {
            return Err("relative is warmer = 0 or 1 and dimmer = 0 to 4, not both 0".into());
        }
        let Some(lasting) = file.lasting else {
            return Err("relative needs a for: it is a glow, given back".into());
        };
        if duration_us(&lasting)? > 600_000_000 {
            return Err("a light's for is at most 10m".into());
        }
        let breathe_ms = file
            .pattern
            .map(|pattern| {
                if pattern.kind != "breathe" || pattern.low.is_some() {
                    return Err(
                        "relative takes a breathe pattern with no low: it breathes down its dimmer steps"
                            .to_owned(),
                    );
                }
                let ms = match &pattern.period {
                    Some(period) => u32::try_from(duration_us(period)? / 1_000)
                        .map_err(|_| format!("pattern period {period:?} is too long"))?,
                    None => LightPattern::BREATHE_DEFAULT_MS,
                };
                if !LightPattern::BREATHE_MS.contains(&ms) {
                    return Err(format!(
                        "a breath takes {} to {} ms",
                        LightPattern::BREATHE_MS.start(),
                        LightPattern::BREATHE_MS.end()
                    ));
                }
                Ok(ms)
            })
            .transpose()?;
        return Ok(Call::Relative {
            warmer,
            dimmer,
            lasting: Some(lasting),
            breathe_ms,
        });
    }
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
    if file.over.is_some() && file.out != Some(true) {
        return Err("over goes with out = true".into());
    }
    match file.out {
        Some(true) => {
            if file.colour.is_some()
                || file.dim.is_some()
                || file.level.is_some()
                || file.lasting.is_some()
                || file.pattern.is_some()
            {
                return Err(
                    "out takes no colour, dim, level, for or pattern: only over and until".into(),
                );
            }
            // As the ring takes a fade (`LightRequest::FADE_MS`), whole ms.
            let over_ms = file
                .over
                .as_deref()
                .map(|over| {
                    let ms = duration_us(over)? / 1_000;
                    u32::try_from(ms)
                        .ok()
                        .filter(|ms| LightRequest::FADE_MS.contains(ms))
                        .ok_or_else(|| {
                            format!(
                                "over = {over:?}: from {} s to {} s",
                                LightRequest::FADE_MS.start() / 1_000,
                                LightRequest::FADE_MS.end() / 1_000
                            )
                        })
                })
                .transpose()?;
            return Ok(Call::Out {
                over_ms,
                until_ends,
            });
        }
        Some(false) => return Err("out is true or left out".into()),
        None => {}
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
            let keep = file.keep == Some(true);
            // Held until the trigger ends, a light has no while of its own,
            // and it is given back, so it is never the person's (#1681).
            if until_ends && (file.lasting.is_some() || keep) {
                return Err(
                    "until = \"ends\" with a colour takes no for and no keep: it is held until the trigger ends, then given back"
                        .into(),
                );
            }
            // A kept light is the person's own, steady and for good: it has
            // no while and no pattern, which are a skill's light's.
            if keep && (file.lasting.is_some() || file.pattern.is_some()) {
                return Err(
                    "a kept light takes no for and no pattern: it is the person's own light".into(),
                );
            }
            let pattern = file
                .pattern
                .map(|pattern| pattern_of(pattern, file.level))
                .transpose()?;
            Ok(Call::Light {
                colour,
                level: file.level,
                lasting: file.lasting,
                pattern,
                keep,
                until_ends,
            })
        }
        (None, Some(steps)) if (1..=4).contains(&steps) => {
            if file.level.is_some() || file.lasting.is_some() {
                return Err("dim takes no level and no for".into());
            }
            if file.keep.is_some() {
                return Err("keep goes with a colour: a dim is given back".into());
            }
            if file.pattern.is_some() {
                return Err("dim takes no pattern".into());
            }
            Ok(Call::Dim { steps, until_ends })
        }
        _ => {
            Err("a light is a colour (with a level and a for), dim = 1 to 4, or out = true".into())
        }
    }
}

/// A show's frames as its file says them (#1679): 2 to 30, each a colour,
/// a level and a while of 2 to 20 s, with a pattern the ring plays if any,
/// the whole show a minute at most.
fn show_frames(file: ShowFile) -> Result<Vec<Frame>, String> {
    if !SHOW_FRAMES.contains(&file.frames.len()) {
        return Err(format!(
            "a show has {} to {} [[light]] frames",
            SHOW_FRAMES.start(),
            SHOW_FRAMES.end()
        ));
    }
    let mut frames = Vec::new();
    for (at, frame) in file.frames.into_iter().enumerate() {
        let number = at + 1;
        if !Light::COLOURS.contains(&frame.colour.as_str()) {
            return Err(format!(
                "frame {number}: colour {:?} is not one of {:?}",
                frame.colour,
                Light::COLOURS
            ));
        }
        if !(1..=5).contains(&frame.level) {
            return Err(format!("frame {number}: level is 1 to 5"));
        }
        let lasting_us =
            duration_us(&frame.lasting).map_err(|why| format!("frame {number}: {why}"))?;
        if !SHOW_FRAME_US.contains(&lasting_us) {
            return Err(format!(
                "frame {number}: a frame is shown 2s to 20s: slow enough to read as a light, never a flicker"
            ));
        }
        let pattern = frame
            .pattern
            .map(|pattern| pattern_of(pattern, Some(frame.level)))
            .transpose()
            .map_err(|why| format!("frame {number}: {why}"))?;
        frames.push(Frame {
            colour: frame.colour,
            level: frame.level,
            pattern,
            lasting_us,
        });
    }
    if frames.iter().map(|frame| frame.lasting_us).sum::<u64>() > SHOW_AT_MOST_US {
        return Err("a show lasts a minute at most".into());
    }
    Ok(frames)
}

/// A light file's pattern for a light at `level` (#940 item 3): a kind the
/// ring plays, its period in its kind's bounds (its default when none),
/// for a breath a low from 1 to under the level, 1 when none, and for a
/// candle a level of 3 or more. A pattern needs the light's level named:
/// it is the top a breath rises to, a candle dips under and a pulse rises
/// over.
fn pattern_of(file: PatternFile, level: Option<u8>) -> Result<LightPattern, String> {
    let kind = match file.kind.as_str() {
        "breathe" => PatternKind::Breathe,
        "candle" => PatternKind::Candle,
        "pulse" => PatternKind::Pulse,
        other => {
            return Err(format!(
                "pattern kind {other:?} is not breathe, candle or pulse (no blink, flash or rainbow: a lamp is not a machine)"
            ));
        }
    };
    let Some(level) = level else {
        return Err("a pattern needs the light's level, the top it moves up to".into());
    };
    let period_ms = match &file.period {
        Some(period) => u32::try_from(duration_us(period)? / 1_000)
            .map_err(|_| format!("pattern period {period:?} is too long"))?,
        None => LightPattern::default_ms(kind),
    };
    if file.low.is_some() && kind != PatternKind::Breathe {
        return Err("only a breath takes low: a candle dips a fifth under its level, and a pulse rises over it".into());
    }
    let pattern = LightPattern {
        kind,
        period_ms,
        low: file.low.unwrap_or(1),
    };
    pattern.check(level).map_err(|why| why.0)?;
    Ok(pattern)
}

/// Every enabled skill under `root`, and why each other folder did not
/// load. Disabled skills are still validated and reserve their tool names
/// in the complete inventory ([`load_each`]).
pub fn load_all(root: &Path) -> (Vec<Skill>, Vec<String>) {
    let (mut skills, mut errors) = (Vec::new(), Vec::new());
    for (folder, loaded) in load_each(root) {
        match loaded {
            Ok(skill) if skill.enabled => skills.push(skill),
            Ok(_) => {}
            Err(why) => errors.push(format!("{}: {why}", folder.display())),
        }
    }
    (skills, errors)
}

/// Every skill folder under `root` but those whose name begins with a dot,
/// in the folders' order, including disabled skills, each loaded or why
/// not. A tool's name is one skill's: the core skills take theirs first,
/// then the installable ones,
/// each in the folders' order, and a skill whose tool's name is taken is not
/// loaded. So an installable skill never takes a core skill's function from
/// the model, and no second skill of a name is passed over without a word
/// ([`tools_file`] and [`Engine::tool`] would give the name to the first).
pub fn load_each(root: &Path) -> Vec<(PathBuf, Result<Skill, String>)> {
    let mut folders: Vec<PathBuf> = fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    folders.sort();
    let mut loaded: Vec<(PathBuf, Result<Skill, String>)> = folders
        .into_iter()
        .map(|folder| {
            let skill = load(&folder);
            (folder, skill)
        })
        .collect();
    // Each tool's name, and the skill that has it.
    // With, for a `[stop]` function, the `[budget]` group of its skill: a
    // `[stop]` may be shared by the skills of one group (#1685), one
    // function for them all.
    let mut taken: BTreeMap<String, (String, SkillKind, Option<String>)> = BTreeMap::new();
    for kind in [SkillKind::Core, SkillKind::Installable, SkillKind::Addon] {
        for (_, result) in &mut loaded {
            let Ok(skill) = result else { continue };
            if skill.kind != kind {
                continue;
            }
            let group = skill.budget.as_ref().map(|(group, _)| group.clone());
            let functions: Vec<(String, Option<String>)> = skill
                .tool
                .iter()
                .map(|tool| (tool.clone(), None))
                .chain(
                    skill
                        .stop
                        .as_ref()
                        .map(|stop| (stop.tool.clone(), group.clone())),
                )
                .collect();
            if let Some((tool, mine, (owner, owner_kind, theirs))) =
                functions.iter().find_map(|(tool, shared)| {
                    taken
                        .get(tool)
                        .filter(|(_, _, theirs)| shared.is_none() || theirs != shared)
                        .map(|owner| (tool, shared, owner))
                })
            {
                let stops = if mine.is_some() || theirs.is_some() {
                    "; a `[stop]` is shared only by the skills of one `[budget]` group"
                } else {
                    ""
                };
                *result = Err(format!(
                    "its tool {tool:?} is the {} skill {owner}'s: a function the model calls is one skill's, \
                     a core skill's first{stops}",
                    owner_kind.name()
                ));
                continue;
            }
            for (tool, shared) in functions {
                taken
                    .entry(tool)
                    .or_insert((skill.name.clone(), kind, shared));
            }
        }
    }
    loaded
}

/// One bounded snapshot, never a history of publications. Invalid selected
/// values do not replace it or create another chance when valid data returns.
#[derive(Default)]
struct Changes {
    values: Option<Vec<Value>>,
    eligible: bool,
}

impl Changes {
    fn bounded_scalar(value: &Value) -> bool {
        match value {
            Value::Null | Value::Bool(_) | Value::Number(_) => true,
            Value::String(text) => text.len() <= MOST_CHANGE_STRING_BYTES,
            _ => false,
        }
    }

    fn scalar_at<'a>(value: &'a Value, field: &str) -> Option<&'a Value> {
        let value = field
            .split('.')
            .try_fold(value, |value, name| value.get(name))?;
        Self::bounded_scalar(value).then_some(value)
    }

    /// Invalid selected predicate/choice inputs are not eligibility changes.
    /// A valid false predicate needs no choice or other selected fields:
    /// pending=0 legitimately has no oldest.id or oldest.delivery.
    fn selected_inputs_valid(skill: &Skill, trigger: &Trigger, value: &Value) -> bool {
        let predicate_valid = match &trigger.test {
            Test::Equals(field, _)
            | Test::NotEquals(field, _)
            | Test::Above(field, _)
            | Test::Below(field, _)
            | Test::Number(field) => {
                !trigger.changes.contains(field) || Self::scalar_at(value, field).is_some()
            }
            Test::Faces(_) | Test::NoFaces | Test::FacesAtMost(_) => true,
        };
        predicate_valid
            && (!trigger.test.holds(value)
                || skill
                    .choice
                    .as_ref()
                    .and_then(|choice| choice.field.as_ref())
                    .is_none_or(|field| {
                        !trigger.changes.contains(field)
                            // Match chooses: a choice names a top-level field.
                            || value.get(field).is_some_and(Self::bounded_scalar)
                    }))
    }

    fn observe(&mut self, fields: &[String], value: &Value) -> bool {
        let values: Option<Vec<Value>> = fields
            .iter()
            .map(|field| Self::scalar_at(value, field).cloned())
            .collect();
        let Some(values) = values else {
            return false;
        };
        let changed = !self.eligible || self.values.as_ref() != Some(&values);
        self.values = Some(values);
        self.eligible = true;
        changed
    }
}

#[derive(Default)]
struct Watch {
    started_us: Option<u64>,
    last_held_us: Option<u64>,
    /// Since when it has held, through dropouts shorter than the gap.
    since_us: Option<u64>,
    /// How long it had not held before that: since [`Watch::visited_us`].
    away_us: u64,
    /// When it last held in a hold that lasted its `for`: what `away_us` is
    /// counted from. A hold shorter than `for` (a face glimpsed, lost past
    /// the `gap`, then back) is no visit, so the visit after it is not
    /// "away 3 s" (lamp-52e6, #2825).
    visited_us: Option<u64>,
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
    /// With `rise`: the readings it compares with, since the start or the
    /// last reload.
    readings: Readings,
    /// `when = "changes"`: the last eligible selected values and whether
    /// the trigger and its while conditions have remained eligible.
    changes: Changes,
    /// With `yields_to`: when this time it holds first waited, and said so.
    yielded_us: Option<u64>,
    /// Each gate a due chance was said dropped by ([`dropped`], #2602): the
    /// hold it was said in and when. A run clears them.
    said: HashMap<&'static str, (Option<u64>, u64)>,
}

/// A gate's line is not said again for the same trigger within this long,
/// even in a new hold: a face that flickers past its `gap` says it once.
const DROPPED_LINE_US: u64 = 60_000_000;
/// The most lines kept for a host that never takes them.
const DROPPED_LINES_KEPT: usize = 256;

/// A due chance a gate dropped or held back (#2602), for the journal:
/// `<skill> not run: <why>`, once a hold for each gate and not again for
/// that gate within [`DROPPED_LINE_US`]. Taken with [`Engine::take_dropped`].
fn dropped(
    lines: &mut Vec<String>,
    watch: &mut Watch,
    skill: &str,
    gate: &'static str,
    now_us: u64,
    why: impl FnOnce() -> String,
) {
    let hold = watch.since_us;
    if watch.said.get(gate).is_some_and(|(on, at)| {
        (hold.is_some() && *on == hold) || now_us.saturating_sub(*at) < DROPPED_LINE_US
    }) {
        return;
    }
    watch.said.insert(gate, (hold, now_us));
    if lines.len() < DROPPED_LINES_KEPT {
        lines.push(format!("{skill} not run: {}", why()));
    }
}

/// How long is left of `us`, for the journal: `7 min`, or `40 s` under one.
fn left(us: u64) -> String {
    if us >= 60_000_000 {
        format!("{} min", us.div_ceil(60_000_000))
    } else {
        format!("{} s", us.div_ceil(1_000_000))
    }
}

/// Where a `yields_to` trigger's chance stands against the daily trigger it
/// yields to (#1672).
enum Yield {
    /// Nothing to wait for: no rested night, the skill or its trigger not
    /// loaded, outside its window, no journal, or its day already claimed.
    No,
    /// Its day, not yet claimed, and nothing to wait for: its trigger does
    /// not hold, or its chance was dropped, or it cannot act. This chance
    /// is taken, and claims that day for it: one morning moment a day.
    Open(i64),
    /// Its trigger holds since `since_us`, its chance still to come on
    /// `day`: this chance waits while that hold lasts unbroken.
    Pending { since_us: u64, day: i64 },
    /// It took its chance today, at this time.
    Fired(u64),
}

/// A trigger that held: what was said of it, and the calls to make.
#[derive(Debug)]
pub struct Run {
    /// Scheduled runs belong to their original definitions. Ordinary runs
    /// retain legacy revalidation across reloads. An allocation identity
    /// cannot wrap back to an earlier generation.
    generation: Option<Arc<()>>,
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
    generation: Arc<()>,
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
    /// The runs each skill with `most_a_day` started: its day and how many.
    day_runs: HashMap<String, (i64, u32)>,
    /// When each group's runs began, within the last hour, by group.
    group_runs: HashMap<String, std::collections::VecDeque<u64>>,
    /// By group, each member waiting for its hour: since when, and when it
    /// was last held back for it.
    budget_waits: HashMap<String, HashMap<String, (u64, u64)>>,
    /// When each member of a group last started or was asked: of the
    /// skills sharing a `[stop]`, the latest is what a call without `all`
    /// stops.
    group_ran: HashMap<String, u64>,
    /// Each resting skill's last nudge and how many in a row went unnoticed.
    nudges: HashMap<String, Nudge>,
    /// Until when each snoozed skill starts nothing itself, by skill: in
    /// memory only, so a restart forgets it (#1685).
    snoozed: HashMap<String, u64>,
    /// The skills turned off, by name, until they are asked for again.
    off: BTreeSet<String>,
    /// Where `off` is kept across a restart ([`OFF_FILE`]), if anywhere.
    off_file: Option<PathBuf>,
    /// Who was at the lamp, from `room/presence`, for a `[history]` (#1685).
    visits: visits::Visits,
    /// With `LAMP_WHO_WAS_HERE_KNOWN=on`: the eyes' known file, for the
    /// mark of someone the lamp knows ([`known`]), never a name.
    known_file: Option<PathBuf>,
    /// With `LAMP_WHO_WAS_HERE_NAMES=on` too (#1997): the names of those who
    /// said yes to being named, from the eyes' `tellable` beside the known
    /// file.
    naming: bool,
    /// Scheduled windows require a trusted sample; legacy quiet hours keep
    /// their raw local-clock semantics independently.
    daily_clock_trusted: bool,
    /// A daily chance is durable before its calls or Policy question leave.
    daily: Option<daily::Journal>,
    /// The first storage failure is reported once; reloads do not retry it.
    daily_faulted: bool,
    /// The local day the last night the lamp rested began, from the routine
    /// manager ([`Engine::rested`]): a `yields_to` waits only the morning
    /// after.
    rested_day: Option<i64>,
    /// Each daily trigger judged today, by skill and trigger name: its
    /// occurrence day, and when it took its chance (none: dropped or let
    /// go). In memory: the journal says a day was claimed, not when.
    judged: HashMap<(String, String), (i64, Option<u64>)>,
    /// The journal's lines of due chances dropped ([`dropped`]), not yet taken.
    dropped: Vec<String>,
    /// With [`REST_THROUGH_SOUNDS_ENVIRONMENT`] on: a `[[while]]`'s
    /// `through` and `broken_by` are kept (#1672).
    rest_through_sounds: bool,
    /// With [`SOONER_ENVIRONMENT`] on: a trigger's `sooner` is kept (#3709).
    sooner: bool,
}

/// A skill's last run, for `[rest]`: when it began, whether anyone noticed
/// it within its while, and the unnoticed runs in a row before it.
#[derive(Clone, Copy, Debug, Default)]
struct Nudge {
    at_us: u64,
    within_us: u64,
    after: u32,
    /// The largest face as the run began: its centre across and down, and
    /// its height, as shares of the picture.
    face: Option<[f32; 3]>,
    /// What the eyes said the person at the desk was doing as the run
    /// began (`room/activity`, #1678), if they said it with the sitting's
    /// baseline in: before it, a baseline coming in mid-run could say
    /// `head_down` of someone who did nothing (the reviewer's P3 on #1938).
    activity: Option<Activity>,
    noticed: bool,
    /// The newest faces in its while showed nobody: they got up.
    gone: bool,
    judged: bool,
    ignored: u32,
    resting_until_us: u64,
}

/// The largest face of a `humans/faces/tracked` value: its centre across
/// and down and its height, as shares of the picture.
fn largest_face(value: &Value) -> Option<[f32; 3]> {
    serde_json::from_value::<Faces>(value.clone())
        .ok()?
        .faces
        .into_iter()
        .max_by(|a, b| (a.ymax - a.ymin).total_cmp(&(b.ymax - b.ymin)))
        .map(|face| {
            [
                (face.xmin + face.xmax) / 2.0,
                (face.ymin + face.ymax) / 2.0,
                face.ymax - face.ymin,
            ]
        })
}

/// `skill`'s run was made at `now_us`: with `[rest]`, whether it is
/// noticed is watched for from now, from the largest face now in view.
fn nudged(
    nudges: &mut HashMap<String, Nudge>,
    latest: &HashMap<String, (Value, u64, Option<u64>)>,
    skill: &Skill,
    now_us: u64,
) {
    let Some(rest) = skill.rest else {
        return;
    };
    let face = Engine::fresh(latest, Faces::TOPIC, now_us).and_then(largest_face);
    let activity = Engine::fresh(latest, RoomActivity::TOPIC, now_us)
        .and_then(|value| serde_json::from_value::<RoomActivity>(value.clone()).ok())
        .filter(|activity| activity.baseline)
        .map(|activity| activity.state);
    let nudge = nudges.entry(skill.name.clone()).or_default();
    *nudge = Nudge {
        at_us: now_us,
        within_us: rest.within_us,
        after: rest.after_ignored,
        face,
        activity,
        noticed: false,
        gone: false,
        judged: false,
        ..*nudge
    };
}

/// The local day `now_us` is in: its date with the local time known, else
/// a day of the engine's own clock.
fn day_of(clock: Option<(i32, u64, u64)>, now_us: u64) -> i64 {
    match clock {
        Some((offset, at_us, unix_ms)) => {
            let ms = i128::from(unix_ms) + (i128::from(now_us) - i128::from(at_us)) / 1_000;
            (ms.div_euclid(60_000) + i128::from(offset)).div_euclid(1_440) as i64
        }
        None => (now_us / 86_400_000_000) as i64,
    }
}

/// The local moment at `now_us` and its Unix time, with a recent,
/// plausible local clock only: what new scheduled behaviour goes by.
fn trusted_moment(
    clock: Option<(i32, u64, u64)>,
    trusted: bool,
    now_us: u64,
) -> Option<(daily::Moment, u64)> {
    let (offset, at_us, unix_ms) = clock.filter(|_| trusted)?;
    let elapsed = now_us.checked_sub(at_us)?;
    if elapsed > 120_000_000 {
        return None;
    }
    let unix_ms = unix_ms.checked_add(elapsed / 1_000)?;
    let moment = daily::Moment::at(&lamp_local_time::LocalTime::Offset(offset), unix_ms).ok()?;
    Some((moment, unix_ms))
}

/// How long before `now_us` the current occurrence of `window` opened, on
/// the engine's clock, if the local time is trusted and inside it.
fn opened_us_ago(
    clock: Option<(i32, u64, u64)>,
    trusted: bool,
    window: &daily::Window,
    now_us: u64,
) -> Option<u64> {
    let (moment, unix_ms) = trusted_moment(clock, trusted, now_us)?;
    window
        .opened_ms_ago(moment.minute, unix_ms % 60_000)
        .map(|ms| ms * 1_000)
}

/// When the next [`MORNING`] comes after `now_us`, on the engine's clock:
/// 12 h on with the local time not known.
fn next_morning_us(clock: Option<(i32, u64, u64)>, now_us: u64) -> u64 {
    let Some((offset, at_us, unix_ms)) = clock else {
        return now_us + 12 * HOUR_US;
    };
    let ms = i128::from(unix_ms) + (i128::from(now_us) - i128::from(at_us)) / 1_000;
    let of_day = (ms + i128::from(offset) * 60_000).rem_euclid(86_400_000);
    let morning = i128::from(MORNING) * 60_000;
    let wait = if of_day < morning {
        morning - of_day
    } else {
        86_400_000 - of_day + morning
    };
    now_us + (wait as u64) * 1_000
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
    /// Who was here's mark of someone the lamp knows, from the eyes' known
    /// file at `path` (`LAMP_WHO_WAS_HERE_KNOWN=on`, [`known`]): off unless
    /// given, and told only to a skill with `known_faces = true`.
    pub fn marking_known(mut self, path: PathBuf) -> Self {
        self.known_file = Some(path);
        self
    }

    /// With the mark: a visit by someone who said yes to being named to
    /// others (the eyes' `tellable`, `LAMP_WHO_WAS_HERE_NAMES=on`, #1997) is
    /// told with their name, only to a skill with `known_faces = true`.
    pub fn naming_who_said_yes(mut self) -> Self {
        self.naming = true;
        self
    }

    /// `skip_ask`: act without asking the Policy (`LAMP_SKILLS_ASK=skip`).
    /// Without it, or `asking_the_policy`, nothing a skill starts is done.
    pub fn new(skills: Vec<Skill>, skip_ask: bool) -> Self {
        Self {
            generation: Arc::new(()),
            skills: skills.into_iter().filter(|skill| skill.enabled).collect(),
            watches: HashMap::new(),
            latest: HashMap::new(),
            holding: HashMap::new(),
            tool_ran: HashMap::new(),
            heard: Heard::default(),
            skip_ask,
            ask_policy: false,
            clock: None,
            day_runs: HashMap::new(),
            group_runs: HashMap::new(),
            budget_waits: HashMap::new(),
            group_ran: HashMap::new(),
            nudges: HashMap::new(),
            snoozed: HashMap::new(),
            off: BTreeSet::new(),
            off_file: None,
            visits: visits::Visits::default(),
            known_file: None,
            naming: false,
            daily_clock_trusted: false,
            daily: None,
            daily_faulted: false,
            rested_day: None,
            judged: HashMap::new(),
            dropped: Vec::new(),
            rest_through_sounds: false,
            sooner: false,
        }
    }

    /// A `[[while]]` with `through` carries its clock through a spell of
    /// those values, and only a message its `broken_by` names begins it
    /// again (`LAMP_REST_THROUGH_SOUNDS=on`, #1672). Without it both are
    /// ignored: any value that does not hold begins the clock again.
    pub fn resting_through_sounds(mut self) -> Self {
        self.rest_through_sounds = true;
        self
    }

    /// A trigger's `sooner` shortens its `for` while its field is past its
    /// line (`LAMP_YAWN_SOONER=on`, #3709). Without it `sooner` is ignored.
    pub fn sooner(mut self) -> Self {
        self.sooner = true;
        self
    }

    /// The lines of due chances dropped since the last take, each
    /// `<skill> not run: <why>` (#2602): the program says each in its journal.
    pub fn take_dropped(&mut self) -> Vec<String> {
        std::mem::take(&mut self.dropped)
    }

    /// The local day the last night the lamp rested began (the routine
    /// manager's [`routine::Manager::rested_day`]), or none.
    pub fn rested(&mut self, day: Option<i64>) {
        self.rested_day = day;
    }

    /// Where a `yields_to` chance stands now ([`Yield`]).
    fn yield_state(&self, (name, trigger): &(String, String), now_us: u64) -> Yield {
        let Some(moment) = self.local_moment(now_us) else {
            return Yield::No;
        };
        if self.rested_day.and_then(|day| day.checked_add(1)) != Some(moment.day) {
            return Yield::No;
        }
        let Some(skill) = self.skills.iter().find(|skill| &skill.name == name) else {
            return Yield::No;
        };
        let Some((index, window)) = skill
            .triggers
            .iter()
            .enumerate()
            .find(|(_, other)| other.name.as_ref() == Some(trigger) && other.once_per_day)
            .and_then(|(index, other)| Some((index, other.during.as_ref()?)))
        else {
            return Yield::No;
        };
        let Some(day) = window
            .contains(moment.minute)
            .then(|| window.occurrence_day(moment.day, moment.minute))
            .flatten()
        else {
            return Yield::No;
        };
        let judged = self.judged.get(&(name.clone(), trigger.clone()));
        if let Some((on, Some(at_us))) = judged
            && *on == day
        {
            return Yield::Fired(*at_us);
        }
        // Claimed on disk and not in this run's memory: taken before a
        // restart. A face here since is no news to an `appears` trigger,
        // and one that comes later is greeted as on any day.
        if self.daily_faulted
            || self
                .daily
                .as_ref()
                .is_none_or(|journal| journal.claimed(name, trigger, day))
        {
            return Yield::No;
        }
        let dropped = judged.is_some_and(|(on, _)| *on == day);
        // What cannot take its chance is not waited for: off, snoozed, or a
        // run that would not act.
        let acts = self.skip_ask
            || self.ask_policy
            || skill.kind == SkillKind::Addon
            || skill.permissions.grants("sleep");
        let able = acts
            && !self.off.contains(name)
            && self.snoozed.get(name).is_none_or(|until| now_us >= *until);
        // Waited for only while its trigger holds with its chance still to
        // come, in its `for` or its `wait` (the reviewer's P2 on #2757): a
        // face it never holds for is not waited for until 11:00.
        let holding = self
            .watches
            .get(&(name.clone(), index))
            .filter(|watch| !watch.spent && !watch.disarmed)
            .and_then(|watch| watch.since_us);
        match holding {
            Some(since_us) if able && !dropped => Yield::Pending { since_us, day },
            _ => Yield::Open(day),
        }
    }

    /// The skills turned off are kept in `dir` (`--state-dir`), one line
    /// each in [`OFF_FILE`], its skill's name and nothing else, and read
    /// from it now ([`read_names`]): off stays off across a restart, a
    /// snooze does not. The names read are returned. Without it, off lasts
    /// until the program stops.
    pub fn keep_off_in(&mut self, dir: &Path) -> Result<Vec<String>, String> {
        let file = dir.join(OFF_FILE);
        self.off = read_names(&file).map_err(|error| format!("{}: {error}", file.display()))?;
        self.off_file = Some(file);
        Ok(self.off.iter().cloned().collect())
    }

    /// The skills turned off read again from where they are kept, which
    /// the app's off and on change there ([`package::turn`], #2815): true
    /// when they changed. Kept as they were when the file can't be read.
    pub fn reread_off(&mut self) -> bool {
        let Some(file) = &self.off_file else {
            return false;
        };
        match read_names(file) {
            Ok(off) if off != self.off => {
                self.off = off;
                true
            }
            Ok(_) => false,
            Err(error) => {
                eprintln!(
                    "lamp-skills: the skills off not read again from {}: {error}",
                    file.display()
                );
                false
            }
        }
    }

    /// The skills turned off, written whole where they are kept
    /// ([`keep_names`]). False when they are not kept, or could not be.
    fn keep_off(&self) -> bool {
        let Some(file) = &self.off_file else {
            return false;
        };
        let kept = keep_names(file, &self.off);
        if let Err(error) = &kept {
            eprintln!(
                "lamp-skills: the skills off not kept in {}: {error}",
                file.display()
            );
        }
        kept.is_ok()
    }

    /// Whether `skill` is snoozed at `now_us` or turned off.
    pub fn stopped(&self, skill: &str, now_us: u64) -> bool {
        self.off.contains(skill) || self.snoozed.get(skill).is_some_and(|until| now_us < *until)
    }

    /// A call of a `[stop]` function: `snooze`, `off` or `on` for its skill,
    /// and the words for the model to pass on, as the note of a tool's say.
    /// Shared by a group's skills (#1685), it stops the one of them that
    /// started or was asked last, or with `all` (or none of them yet) every
    /// one of them, and the words say which.
    fn stop(
        &mut self,
        sharing: &[String],
        function: &str,
        args: &Value,
        now_us: u64,
    ) -> Result<Run, String> {
        const HOW: &str = "Say how: snooze, off or on.";
        let shared = sharing.len() > 1;
        let (how, all) = match args {
            Value::Object(given)
                if given
                    .keys()
                    .all(|key| key == STOP_CHOICE || (shared && key == STOP_ALL))
                    && given.get(STOP_ALL).is_none_or(Value::is_boolean) =>
            {
                (
                    given.get(STOP_CHOICE).and_then(Value::as_str),
                    given.get(STOP_ALL).and_then(Value::as_bool) == Some(true),
                )
            }
            _ => (None, false),
        };
        let group = self
            .skills
            .iter()
            .find(|skill| skill.name == sharing[0])
            .and_then(|skill| skill.budget.as_ref())
            .map(|(group, _)| group.clone());
        let last = sharing
            .iter()
            .filter_map(|skill| self.group_ran.get(skill).map(|at| (*at, skill)))
            .max()
            .map(|(_, skill)| skill.clone())
            .filter(|_| shared && !all);
        let targets: Vec<String> = last.map_or_else(|| sharing.to_vec(), |one| vec![one]);
        // What the person hears it is about: "these" for a skill's own, else
        // the one by its name, or the group's.
        let these = match (shared, targets.as_slice()) {
            (false, _) => "these".to_owned(),
            (true, [one]) => format!("the {} nudges", one.replace('-', " ")),
            (true, _) => format!("the {} nudges", group.as_deref().unwrap_or("wellbeing")),
        };
        // What the app turned since is kept: read again before writing.
        if matches!(how, Some("off" | "on")) {
            self.reread_off();
        }
        let note = match how {
            Some("snooze") => {
                let until = next_morning_us(self.clock, now_us);
                for skill in &targets {
                    self.snoozed.insert(skill.clone(), until);
                }
                format!(
                    "Done: none of {these} until tomorrow morning. Tell them so in a few words."
                )
            }
            Some("off") => {
                for skill in &targets {
                    self.snoozed.remove(skill);
                    self.off.insert(skill.clone());
                }
                if self.keep_off() {
                    format!(
                        "Done: none of {these} until they ask for them again. Tell them so in a few words."
                    )
                } else {
                    format!(
                        "Done, but only until the lamp restarts: none of {these} until then, or until they ask for them again. Tell them so in a few words."
                    )
                }
            }
            Some("on") => {
                let mut changed = false;
                for skill in &targets {
                    self.snoozed.remove(skill);
                    changed |= self.off.remove(skill);
                }
                if changed {
                    self.keep_off();
                }
                let these = if shared { these } else { "these".to_owned() };
                format!("Done: {these} come again as before. Tell them so in a few words.")
            }
            _ => return Err(HOW.into()),
        };
        Ok(Run {
            skill: targets[0].clone(),
            events: vec![event(
                &targets[0],
                SkillStage::Triggered,
                format!("the model called {function} with {args}, for {targets:?}"),
                None,
            )],
            calls: vec![Call::Say(note)],
            ask: None,
            queue: false,
            from: None,
            generation: None,
        })
    }

    /// Ask the Policy's gate (#958, `lamp-policy --gate`) on `skills/ask`
    /// and act only on its `say_now` (`LAMP_SKILLS_ASK=policy`).
    pub fn asking_the_policy(mut self) -> Self {
        self.ask_policy = true;
        self
    }

    /// Without a writable journal, once-per-day triggers stay quiet.
    /// Explicitly installing a journal clears a latched daily storage fault.
    pub fn with_daily_journal(mut self, journal: daily::Journal) -> Self {
        self.daily = Some(journal);
        self.daily_faulted = false;
        self
    }

    /// The local time is known: at `now_us` on the engine's clock it is
    /// `unix_ms`, `utc_offset_minutes` east of UTC. Told again as often as
    /// wanted, so a wall clock set late (by NTP) is followed. Until then, a
    /// skill with quiet hours never starts itself. This is a trusted clock
    /// injection for embedded callers/replays; live hosts use
    /// [`Engine::clock_in_with_trust`] with their clock's current evidence.
    pub fn clock(&mut self, utc_offset_minutes: i32, now_us: u64, unix_ms: u64) {
        self.clock = Some((utc_offset_minutes, now_us, unix_ms));
        self.daily_clock_trusted = true;
    }

    /// [`Engine::clock`] in `local`, with its offset at `unix_ms` (#1384): a
    /// zone's changes with summer time, so quiet hours stay at the same clock
    /// hours across one, which a fixed offset can't do.
    pub fn clock_in(&mut self, local: &lamp_local_time::LocalTime, now_us: u64, unix_ms: u64) {
        self.clock(local.offset_minutes_at(unix_ms), now_us, unix_ms);
    }

    /// Whether a resting skill's run was noticed, from what the bus says,
    /// within its while (#1685): a face turned to the lamp (`facing`), the
    /// largest face moved ([`NOTICED_MOVE`] of the picture across or down,
    /// or [`NOTICED_SIZE`] of its height nearer or farther: leaning in,
    /// sitting back, turning to it), or anything heard (`voice/heard`, its
    /// words never kept). Nobody in the newest faces of its while counts
    /// too: they got up.
    fn notice(&mut self, topic: &str, value: &Value, now_us: u64) {
        let open = |nudge: &Nudge| !nudge.judged && now_us <= nudge.at_us + nudge.within_us;
        if topic == Speech::TOPICS[0] {
            if value["text"]
                .as_str()
                .is_some_and(|text| !text.trim().is_empty())
            {
                for nudge in self.nudges.values_mut().filter(|nudge| open(nudge)) {
                    nudge.noticed = true;
                }
            }
            return;
        }
        // The eyes' own word on the person at the desk (#1678), when they
        // say it: standing up and leaving is noticing; so is any change of
        // what they are doing from what it was as the run began (looking up
        // from the page, leaning in or back), and away is gone.
        if topic == StoodUp::TOPIC {
            for nudge in self.nudges.values_mut().filter(|nudge| open(nudge)) {
                nudge.noticed = true;
            }
            return;
        }
        if topic == RoomActivity::TOPIC {
            let Ok(activity) = serde_json::from_value::<RoomActivity>(value.clone()) else {
                return;
            };
            for nudge in self.nudges.values_mut().filter(|nudge| open(nudge)) {
                if activity.state == Activity::Away {
                    nudge.gone = true;
                } else if nudge.activity.is_some_and(|was| was != activity.state) {
                    nudge.noticed = true;
                }
            }
            return;
        }
        if topic != Faces::TOPIC {
            return;
        }
        let Ok(faces) = serde_json::from_value::<Faces>(value.clone()) else {
            return;
        };
        let facing = faces.faces.iter().any(|face| face.facing == Some(true));
        let now = largest_face(value);
        for nudge in self.nudges.values_mut().filter(|nudge| open(nudge)) {
            nudge.gone = faces.faces.is_empty();
            let moved = match (nudge.face, now) {
                (Some(was), Some(now)) => {
                    (now[0] - was[0]).abs() >= NOTICED_MOVE
                        || (now[1] - was[1]).abs() >= NOTICED_MOVE
                        || (now[2] - was[2]).abs() >= NOTICED_SIZE * was[2]
                }
                _ => false,
            };
            if facing || moved {
                nudge.noticed = true;
            }
        }
    }

    /// Each resting skill's run whose while is over: noticed, or nobody in
    /// view as it ended (they got up), and the count of runs unnoticed in a
    /// row begins again; else it grows, and at its skill's `after_ignored`
    /// the skill rests until the next morning ([`MORNING`], local).
    fn judge_nudges(&mut self, now_us: u64) {
        let clock = self.clock;
        for nudge in self.nudges.values_mut() {
            if nudge.judged || now_us <= nudge.at_us + nudge.within_us {
                continue;
            }
            nudge.judged = true;
            if nudge.noticed || nudge.gone {
                nudge.ignored = 0;
                continue;
            }
            nudge.ignored += 1;
            if nudge.ignored >= nudge.after {
                nudge.ignored = 0;
                nudge.resting_until_us = next_morning_us(clock, now_us);
            }
        }
    }

    /// A run asked of the Policy was made at `now_us`, its answer `say_now`
    /// and its trigger still holding: with `[rest]`, whether it is noticed
    /// is watched for from now (#1685). Without the Policy asked, a run is
    /// watched for as it is started.
    pub fn made(&mut self, skill: &str, now_us: u64) {
        if let Some(skill) = self.skills.iter().find(|known| known.name == skill) {
            nudged(&mut self.nudges, &self.latest, skill, now_us);
        }
    }

    /// Until when `skill` rests after runs nobody noticed, if it does.
    pub fn resting_until(&self, skill: &str, now_us: u64) -> Option<u64> {
        self.nudges
            .get(skill)
            .map(|nudge| nudge.resting_until_us)
            .filter(|until| now_us < *until)
    }

    /// Sample the live clock without letting an unsynchronized date start
    /// daily/windowed work or approve a pending scheduled run. Losing trust
    /// immediately invalidates the previous scheduled sample, while ordinary
    /// quiet-hour checks keep using the raw clock.
    pub fn clock_in_with_trust(
        &mut self,
        local: &lamp_local_time::LocalTime,
        now_us: u64,
        unix_ms: u64,
        trusted: bool,
    ) {
        self.clock_in(local, now_us, unix_ms);
        self.daily_clock_trusted = trusted;
    }

    /// Hosts need not query kernel time for ordinary skills or disabled
    /// routine definitions. This only describes the currently enabled set.
    pub fn needs_daily_clock(&self) -> bool {
        self.skills.iter().any(|skill| {
            skill.is_routine()
                || skill.triggers.iter().any(|trigger| {
                    trigger.topic == LOCAL_CLOCK_TOPIC
                        || trigger.during.is_some()
                        || trigger.once_per_day
                })
        })
    }

    /// Time passes with no message: a run held until its trigger ends ends
    /// when the trigger topic's newest value has expired (its header's
    /// `expires_ms`) or, naming no lifespan, when the topic has said nothing
    /// for [`SILENT_ENDS_US`]. Its spell is over with it.
    pub fn tick(&mut self, now_us: u64) -> Vec<Run> {
        self.judge_nudges(now_us);
        self.invalidate_changes(now_us);
        let mut runs = Vec::new();
        let moment = self.local_moment(now_us);
        for skill in &self.skills {
            for (index, trigger) in skill.triggers.iter().enumerate() {
                let Some(watch) = self.watches.get_mut(&(skill.name.clone(), index)) else {
                    continue;
                };
                let Some((_, at_us, lifespan_us)) = self.latest.get(&trigger.topic) else {
                    continue;
                };
                let silent_us = now_us.saturating_sub(*at_us);
                let silent = silent_us >= lifespan_us.unwrap_or(SILENT_ENDS_US);
                let outside = out_of_window(trigger, moment.as_ref());
                if watch.open && (silent || outside.is_some()) {
                    if trigger.once_per_day
                        && let Some(since) = watch.since_us
                    {
                        let why = outside.unwrap_or_else(|| {
                            format!(
                                "went silent: nothing new on {} for {} s",
                                trigger.topic,
                                silent_us / 1_000_000
                            )
                        });
                        let held_us = watch.last_held_us.unwrap_or(now_us).saturating_sub(since);
                        runs.push(watch_reset(skill, trigger, held_us, why));
                    }
                    watch.open = false;
                    watch.since_us = None;
                    runs.push(Run {
                        generation: None,
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

    /// A wall-clock sample drives clock triggers even in a silent room.
    /// There is no synthetic event when no skill uses the local clock.
    pub fn clock_tick(&mut self, now_us: u64) -> Vec<Run> {
        if !self.skills.iter().any(|skill| {
            skill
                .triggers
                .iter()
                .any(|trigger| trigger.topic == LOCAL_CLOCK_TOPIC)
        }) {
            return Vec::new();
        }
        let value = match self.local_moment(now_us) {
            Some(moment) => json!({"known": true, "day": moment.day, "minute": moment.minute}),
            None => json!({"known": false}),
        };
        self.observe(
            LOCAL_CLOCK_TOPIC,
            now_us,
            &serde_json::to_vec(&value).unwrap(),
            Some(500),
        )
    }

    /// Minutes since local midnight at `now_us`, if the local time is known.
    /// A `[history]` skill's calls, its note to the model followed by the
    /// lamp's record of who was at it over its `within` (#1685): each time
    /// of day when the local time is known, and how long ago.
    fn told_history(&self, skill: &Skill, calls: Vec<Call>, now_us: u64) -> Vec<Call> {
        let Some(history) = &skill.history else {
            return calls;
        };
        // The mark of someone the lamp knows only for a skill that may know
        // a known face (`known_faces`); any other is told "someone".
        let marked = self.known_file.is_some() && skill.permissions.known_faces;
        let telling = match (marked, self.naming) {
            (false, _) => visits::Telling::Someone,
            (true, false) => visits::Telling::Known,
            (true, true) => visits::Telling::Named,
        };
        let record = self.visits.told_as(
            now_us,
            history.within_us,
            visits::when(now_us, |at_us| self.local_minute(at_us), &history.words),
            telling,
            &history.words,
        );
        calls
            .into_iter()
            .map(|call| match call {
                Call::Say(note) => Call::Say(format!("{note}\n\n{record}")),
                other => other,
            })
            .collect()
    }

    fn local_minute(&self, now_us: u64) -> Option<u16> {
        let (offset, at_us, unix_ms) = self.clock?;
        let ms = i128::from(unix_ms) + (i128::from(now_us) - i128::from(at_us)) / 1_000;
        let minutes = ms.div_euclid(60_000) + i128::from(offset);
        Some(minutes.rem_euclid(1_440) as u16)
    }

    /// New scheduled behavior needs a recent, plausible local clock.
    /// Existing quiet-hours semantics remain unchanged.
    fn local_moment(&self, now_us: u64) -> Option<daily::Moment> {
        trusted_moment(self.clock, self.daily_clock_trusted, now_us).map(|(moment, _)| moment)
    }

    /// The skills loaded again. A timed condition keeps when it began to
    /// hold only if it is the same condition as before. A run held until its
    /// trigger ends ends now, as at a normal end, if its skill is gone, no
    /// longer holds anything until its trigger ends, or no longer has that
    /// trigger (the same place in the skill on the same topic): nothing else
    /// would ever end it (the reviewer on #1066). Those ends are returned.
    /// Pending scheduled runs cannot be approved against new definitions.
    /// Their daily claims remain consumed; ordinary runs still revalidate.
    pub fn replace(&mut self, skills: Vec<Skill>) -> Vec<Run> {
        self.generation = Arc::new(());
        let old = std::mem::replace(
            &mut self.skills,
            skills.into_iter().filter(|skill| skill.enabled).collect(),
        );
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
                generation: None,
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
        // A rise begins again too (#1684): an edit may have changed its
        // field or its while, and readings kept for another are not its.
        for ((name, index), watch) in &mut self.watches {
            watch.items = None;
            watch.readings = Readings::default();
            let unchanged = old
                .iter()
                .find(|skill| &skill.name == name)
                .zip(self.skills.iter().find(|skill| &skill.name == name))
                .is_some_and(|(before, after)| {
                    before.triggers.get(*index) == after.triggers.get(*index)
                        && before.conditions == after.conditions
                });
            if !unchanged {
                watch.changes = Changes::default();
            }
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
    /// face reads, and what was heard if a skill has `[again]`, once each
    /// ([`Skill::listens`]), and only those its `[permissions]` lists.
    pub fn topics(&self) -> Vec<String> {
        let mut topics: Vec<String> = self
            .skills
            .iter()
            .flat_map(|skill| {
                skill
                    .listens()
                    .into_iter()
                    .filter(|(topic, _)| skill.permissions.refuses_reading(topic).is_none())
                    // What only begins a carried clock again is heard only
                    // with the switch on (reviewer-two on #3092): off, the
                    // engine reads nothing it did not before.
                    .filter(|(_, why)| self.rest_through_sounds || *why != BROKEN_BY_READ)
                    .map(|(topic, _)| topic)
            })
            .collect();
        // What judges a `[rest]` is the program's own reading, as the voice's
        // state for a step after the say is: never the skill's (#1685).
        if self.skills.iter().any(|skill| skill.rest.is_some()) {
            topics.extend([
                Faces::TOPIC.to_owned(),
                Speech::TOPICS[0].to_owned(),
                RoomActivity::TOPIC.to_owned(),
                StoodUp::TOPIC.to_owned(),
            ]);
        }
        // A search reads what was heard before its call, to know which
        // turn asked for it (#2179): the program's own reading too.
        if self.skills.iter().any(|skill| skill.search.is_some()) {
            topics.push(Speech::TOPICS[0].to_owned());
        }
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
        // A topic that has said nothing yet: held only when asked to be.
        if condition.unknown_holds && !latest.contains_key(&condition.topic) {
            return true;
        }
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
        if skill.is_routine() && !Self::routine_voice_fresh(latest, now_us) {
            return false;
        }
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

    /// Which `[[while]]` condition of `skill` does not hold, for the journal:
    /// its topic, field and the last value it said (`voice/state state was
    /// listening`). Only a number, a yes or no, or a one-word value is shown,
    /// never a person's words: anything else is named by its kind (#2861).
    fn held_back(
        latest: &HashMap<String, (Value, u64, Option<u64>)>,
        holding: &HashMap<(String, usize), u64>,
        skill: &Skill,
        now_us: u64,
    ) -> String {
        fn shown(value: &Value) -> String {
            match value {
                Value::Null => "nothing".into(),
                Value::Bool(_) | Value::Number(_) => value.to_string(),
                Value::String(word)
                    if !word.is_empty()
                        && word.len() <= 32
                        && word
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') =>
                {
                    word.clone()
                }
                Value::String(_) => "a text".into(),
                Value::Array(items) => format!("a list of {}", items.len()),
                Value::Object(_) => "an object".into(),
            }
        }
        if skill.is_routine() && !Self::routine_voice_fresh(latest, now_us) {
            return format!("{} not resting just now", VoiceState::TOPIC);
        }
        let held = skill
            .conditions
            .iter()
            .enumerate()
            .find(|(index, condition)| {
                !Self::condition_holds(
                    latest,
                    condition,
                    holding.get(&(skill.name.clone(), *index)),
                    now_us,
                )
            });
        let Some((index, condition)) = held else {
            return "a condition".into();
        };
        let topic = &condition.topic;
        let (field, value) = match &condition.test {
            Test::Faces(_) | Test::NoFaces | Test::FacesAtMost(_) => ("faces", None),
            Test::Equals(field, _)
            | Test::NotEquals(field, _)
            | Test::Above(field, _)
            | Test::Below(field, _)
            | Test::Number(field) => (field.as_str(), Some(field)),
        };
        let Some((last, _, _)) = latest.get(topic) else {
            return format!("{topic} {field}: nothing said yet");
        };
        let was = match value {
            Some(path) => shown(at(last, path)),
            None => format!("{} faces", last["faces"].as_array().map_or(0, Vec::len)),
        };
        if Self::fresh(latest, topic, now_us).is_none() {
            return format!("{topic} {field} was {was}, and that ran out");
        }
        match holding.get(&(skill.name.clone(), index)) {
            Some(since) if condition.test.holds(last) => format!(
                "{topic} {field} was {was} for {} s of its {} s",
                now_us.saturating_sub(*since) / 1_000_000,
                condition.for_us / 1_000_000
            ),
            _ => format!("{topic} {field} was {was}"),
        }
    }

    fn routine_voice_fresh(
        latest: &HashMap<String, (Value, u64, Option<u64>)>,
        now_us: u64,
    ) -> bool {
        latest.get(VoiceState::TOPIC).is_some_and(|(value, at, _)| {
            *at <= now_us
                && now_us - *at < ROUTINE_VOICE_FRESH_US
                && serde_json::from_value::<VoiceState>(value.clone())
                    .is_ok_and(|voice| voice.state == Voice::Resting)
        })
    }

    /// The first `[[while]]` condition of `skill` that does not hold, for
    /// the journal: `voice/state state resting for 5 s`.
    fn unmet(
        latest: &HashMap<String, (Value, u64, Option<u64>)>,
        holding: &HashMap<(String, usize), u64>,
        skill: &Skill,
        now_us: u64,
    ) -> String {
        if skill.is_routine() && !Self::routine_voice_fresh(latest, now_us) {
            return format!("{} state resting, said within 3 s", VoiceState::TOPIC);
        }
        skill
            .conditions
            .iter()
            .enumerate()
            .find(|(index, condition)| {
                !Self::condition_holds(
                    latest,
                    condition,
                    holding.get(&(skill.name.clone(), *index)),
                    now_us,
                )
            })
            .map_or_else(String::new, |(_, condition)| {
                let mut wanted = format!("{} {}", condition.topic, condition.test.shows());
                if condition.for_us > 0 {
                    wanted += &format!(" for {} s", condition.for_us / 1_000_000);
                }
                wanted
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
        let trigger = run
            .from
            .as_ref()
            .and_then(|(index, _)| skill.triggers.get(*index));
        if run.generation.as_ref().map_or_else(
            || skill.scheduled_run(trigger),
            |generation| !Arc::ptr_eq(&self.generation, generation),
        ) {
            return None;
        }
        if skill.holds_until_ends()
            && !run.from.as_ref().is_some_and(|(index, _)| {
                self.watches
                    .get(&(skill.name.clone(), *index))
                    .is_some_and(|watch| watch.open)
            })
        {
            return None;
        }
        // A rise holds as it did at its newest reading, the one the answer
        // is to: nothing newer came to compare.
        let rose = |index: usize, trigger: &Trigger| {
            trigger.rise.is_none()
                || self
                    .watches
                    .get(&(skill.name.clone(), index))
                    .is_some_and(|watch| watch.readings.risen)
        };
        let holds = |index: usize, trigger: &Trigger, item: Option<&String>| {
            if trigger.once_per_day && self.daily_faulted {
                return false;
            }
            if trigger.during.as_ref().is_some_and(|window| {
                self.local_moment(now_us)
                    .is_none_or(|moment| !window.contains(moment.minute))
            }) {
                return false;
            }
            rose(index, trigger)
                && Self::fresh(&self.latest, &trigger.topic, now_us).is_some_and(|value| {
                    match (&trigger.each, item) {
                        (Some((list, key)), Some(name)) => at(value, list)
                            .as_array()
                            .into_iter()
                            .flatten()
                            .any(|item| {
                                item_name(at(item, key)).as_ref() == Some(name)
                                    && trigger.test.holds(item)
                            }),
                        _ => trigger.test.holds(value) && chooses(skill, value),
                    }
                })
        };
        let trigger = match &run.from {
            Some((index, item)) => {
                let trigger = skill.triggers.get(*index)?;
                holds(*index, trigger, item.as_ref()).then_some(Some(trigger))?
            }
            None => skill
                .triggers
                .iter()
                .enumerate()
                .any(|(index, trigger)| holds(index, trigger, None))
                .then_some(None)?,
        };
        let run_item = run.from.as_ref().and_then(|(_, item)| item.as_deref());
        Self::conditions_hold(&self.latest, &self.holding, skill, now_us)
            .then(|| calls(&self.latest, skill, trigger, now_us, &Value::Null, run_item))
    }

    /// Remember lost eligibility even between trigger-topic publications:
    /// a brief talk, or an expired while value renewed before the next due
    /// message, must not disappear into an unchanged selected tuple.
    fn invalidate_changes(&mut self, now_us: u64) {
        for skill in &self.skills {
            if !skill
                .triggers
                .iter()
                .any(|trigger| trigger.when == When::Changes)
            {
                continue;
            }
            let conditions = Self::conditions_hold(&self.latest, &self.holding, skill, now_us);
            for (index, trigger) in skill.triggers.iter().enumerate() {
                if trigger.when == When::Changes
                    && (!conditions
                        || Self::fresh(&self.latest, &trigger.topic, now_us).is_none_or(|value| {
                            Changes::selected_inputs_valid(skill, trigger, value)
                                && !(trigger.test.holds(value) && chooses(skill, value))
                        }))
                    && let Some(watch) = self.watches.get_mut(&(skill.name.clone(), index))
                {
                    watch.changes.eligible = false;
                }
            }
        }
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
        // Check expiry before replacing the previous value with a fresh one.
        self.invalidate_changes(now_us);
        // Whether a run of a resting skill was noticed (#1685), then each
        // whose while is over judged.
        self.notice(topic, &value, now_us);
        self.judge_nudges(now_us);
        // Who was at the lamp, for a `[history]` (#1685): times only.
        if topic == RoomPresence::TOPIC
            && let Ok(presence) = serde_json::from_value::<RoomPresence>(value.clone())
            && presence.check().is_ok()
        {
            self.visits
                .seen(presence.someone, presence.nobody_s, presence.by, now_us);
            // Someone the lamp knows at it, with the mark on: only someone
            // who asked to be remembered, as the eyes write them. The name
            // is let go here, unread past whether there is one.
            // With names on, and only for someone the eyes say said yes to
            // being named (`tellable`), the name is kept with the spell.
            if presence.someone
                && let Some(path) = self.known_file.as_deref()
                && let Some(name) = known::name_in(path, known::MARK_FRESH)
            {
                self.visits.known_here();
                if self.naming && known::tellable_in(&known::tellable_of(path)).contains(&name) {
                    self.visits.named(&name);
                }
            }
        }
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
        let before = Self::fresh(&self.latest, topic, now_us).cloned();
        let lasted = before.is_some();
        let routine_voice_lasted = Self::routine_voice_fresh(&self.latest, now_us);
        // The voice's last value said within the routine's 3 s, whatever it
        // was: a carried clock never bridges a gap in what the voice said
        // (reviewer-two's second P2 on #3092), since `voice/state` has a
        // period and no lifespan.
        let voice_recent = self
            .latest
            .get(VoiceState::TOPIC)
            .is_some_and(|(_, at, _)| *at <= now_us && now_us - *at < ROUTINE_VOICE_FRESH_US);
        self.latest
            .insert(topic.to_owned(), (value.clone(), now_us, lifespan_us));
        for skill in &self.skills {
            for (index, condition) in skill.conditions.iter().enumerate() {
                if condition.topic != topic {
                    continue;
                }
                let key = (skill.name.clone(), index);
                // A spell it is carried through (#1672) does not hold now,
                // but its clock goes on from where it was, while the last
                // value lasted; and the value after it goes on with it,
                // though a routine's quiet gate wanted the voice resting
                // before. For a routine's voice, only while the voice's
                // last value is within its 3 s: after a gap, it begins again.
                let recent = !skill.is_routine() || topic != VoiceState::TOPIC || voice_recent;
                let carried =
                    self.rest_through_sounds && lasted && recent && condition.carries(&value);
                let after_carried = self.rest_through_sounds
                    && recent
                    && before
                        .as_ref()
                        .is_some_and(|before| condition.carries(before));
                let lasted = lasted
                    && (!skill.is_routine()
                        || topic != VoiceState::TOPIC
                        || routine_voice_lasted
                        || after_carried);
                if condition.test.holds(&value) && lasted {
                    self.holding.entry(key).or_insert(now_us);
                } else if condition.test.holds(&value) {
                    self.holding.insert(key, now_us);
                } else if !carried {
                    self.holding.remove(&key);
                }
            }
        }
        // Words heard or a new push: the clocks carried through a sound
        // begin again at the next value that holds.
        if self.rest_through_sounds && breaks(topic, &value, before.as_ref()) {
            for skill in &self.skills {
                for (index, condition) in skill.conditions.iter().enumerate() {
                    if condition.broken_by.iter().any(|by| by == topic) {
                        self.holding.remove(&(skill.name.clone(), index));
                    }
                }
            }
        }
        self.invalidate_changes(now_us);
        let mut runs = Vec::new();
        let minute = self.local_minute(now_us);
        let moment = self.local_moment(now_us);
        // What a run held until its trigger stops holding is given back
        // when it does.
        let ends = |watch: &mut Watch, skill: &Skill, runs: &mut Vec<Run>| {
            if std::mem::take(&mut watch.open) {
                runs.push(Run {
                    generation: None,
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
                if trigger.when == When::Changes
                    && !Changes::selected_inputs_valid(skill, trigger, &value)
                {
                    continue;
                }
                let yielding = trigger
                    .yields_to
                    .as_ref()
                    .map_or(Yield::No, |to| self.yield_state(to, now_us));
                let watch = self.watches.entry((skill.name.clone(), index)).or_default();
                if let Some(why) = out_of_window(trigger, moment.as_ref()) {
                    if trigger.once_per_day
                        && let Some(since) = watch.since_us
                    {
                        let held_us = watch.last_held_us.unwrap_or(now_us).saturating_sub(since);
                        runs.push(watch_reset(skill, trigger, held_us, why));
                    }
                    ends(watch, skill, &mut runs);
                    // Time outside the window cannot count toward a duration
                    // or leave an old presence edge waiting for tomorrow.
                    *watch = Watch::default();
                    continue;
                }
                if let Some((list, key)) = &trigger.each {
                    // Each item on its own: one that holds where, in the
                    // trigger's last message, it did not, or was not in the
                    // list. The first message since the program started
                    // only says what is there: what was there then is not
                    // news. After a reload the value kept then is the last.
                    let onboarding = skill.permissions.grants("onboarding")
                        && skill.steps.iter().any(|(_, step)| {
                            matches!(step, Step::Call(Call::SayFrom(Source::Onboarding, _)))
                        });
                    // Only this source may deliver its initial snapshot: the
                    // runner checks the token and owns bounded retry/restart.
                    // Malformed or oversized lists never produce an ask.
                    let valid = !onboarding
                        || serde_json::from_value::<lamp_msgs::OnboardingPending>(value.clone())
                            .is_ok_and(|pending| pending.check().is_ok());
                    if !valid {
                        // Invalid input cannot rearm an already-consumed token.
                        continue;
                    }
                    let items = items_of(trigger, list, key, &value);
                    let came: Vec<String> = match &watch.items {
                        None if onboarding => items
                            .iter()
                            .filter(|(_, holds)| **holds)
                            .map(|(name, _)| name.clone())
                            .collect(),
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
                            let unmet = Self::unmet(&self.latest, &self.holding, skill, now_us);
                            dropped(
                                &mut self.dropped,
                                watch,
                                &skill.name,
                                "while",
                                now_us,
                                || format!("an item came, but its while did not hold: {unmet}"),
                            );
                            continue;
                        }
                        let held = format!("{key} {name} of {list} on {topic} came to hold");
                        let run = decide(
                            skill,
                            skill
                                .scheduled_run(Some(trigger))
                                .then_some(&self.generation),
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
                // A rise compares the field's readings: a message without
                // one (the sensor not knowing it yet) is passed over, as if
                // it never came, and breaks nothing (#1684).
                let mut rose = true;
                if let Some(rise) = &trigger.rise {
                    let Some(reading) = at(&value, &rise.field)
                        .as_f64()
                        .filter(|got| got.is_finite())
                    else {
                        continue;
                    };
                    rose = watch.readings.rose(reading, now_us, rise);
                }
                if let Some(line) = &trigger.rearm_line {
                    if !line.test.holds(&value) {
                        watch.past_line_us = None;
                    } else if now_us - *watch.past_line_us.get_or_insert(now_us) >= line.for_us {
                        watch.disarmed = false;
                    }
                }
                let holds = trigger.test.holds(&value);
                if !(rose && holds && chooses(skill, &value)) {
                    if watch
                        .last_held_us
                        .is_none_or(|held| now_us - held >= trigger.gap_us)
                    {
                        if trigger.once_per_day
                            && let Some(since) = watch.since_us
                        {
                            let what = match (&trigger.rise, holds) {
                                (Some(rise), _) if !rose => {
                                    format!("{} no longer rising", rise.field)
                                }
                                (_, false) => trigger.test.lapsed(),
                                _ => "not a choice the skill takes".into(),
                            };
                            let held_us =
                                watch.last_held_us.unwrap_or(now_us).saturating_sub(since);
                            runs.push(watch_reset(
                                skill,
                                trigger,
                                held_us,
                                format!("stopped holding: {topic} {what}"),
                            ));
                        }
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
                    // Held again only after a silence past what it tolerates:
                    // what it held is lost, and said so for a daily one.
                    if trigger.once_per_day
                        && let (Some(since), Some(held)) = (watch.since_us, watch.last_held_us)
                    {
                        runs.push(watch_reset(
                            skill,
                            trigger,
                            held.saturating_sub(since),
                            format!(
                                "went silent: nothing held it on {topic} for {} s, past its {} s",
                                (now_us - held) / 1_000_000,
                                tolerated / 1_000_000
                            ),
                        ));
                    }
                    ends(watch, skill, &mut runs);
                    watch.since_us = Some(now_us);
                    watch.away_us = now_us - watch.visited_us.unwrap_or(started);
                    watch.spent = false;
                    watch.waiting_us = None;
                    watch.yielded_us = None;
                }
                watch.last_held_us = Some(now_us);
                let held_us = now_us - watch.since_us.unwrap_or(now_us);
                // Brought forward while its sooner's field is past its line.
                let for_us = trigger
                    .sooner
                    .as_ref()
                    .filter(|sooner| {
                        self.sooner
                            && Self::fresh(&self.latest, &sooner.topic, now_us)
                                .is_some_and(|value| sooner.test.holds(value))
                    })
                    .map_or(trigger.for_us, |sooner| sooner.for_us);
                if held_us < for_us {
                    continue;
                }
                watch.visited_us = Some(now_us);
                // Once each time it holds: a chance held back now waits for
                // the next time.
                let once = trigger.when == When::Appears || trigger.rearm;
                if once && watch.spent {
                    continue;
                }
                if watch.disarmed {
                    if let Some(line) = &trigger.rearm_line {
                        dropped(
                            &mut self.dropped,
                            watch,
                            &skill.name,
                            "rearm",
                            now_us,
                            || {
                                format!(
                                    "not rearmed: {} for {} s first",
                                    line.test.shows(),
                                    line.for_us / 1_000_000
                                )
                            },
                        );
                    }
                    continue;
                }
                watch.spent = once;
                if trigger.rearm_line.is_some() {
                    watch.disarmed = true;
                    watch.past_line_us = None;
                }
                if trigger.when == When::Appears && watch.away_us < trigger.absent_for_us {
                    let (away, absent) = (watch.away_us, trigger.absent_for_us);
                    dropped(
                        &mut self.dropped,
                        watch,
                        &skill.name,
                        "away",
                        now_us,
                        || {
                            format!(
                                "away {} s, under its {} s",
                                away / 1_000_000,
                                absent / 1_000_000
                            )
                        },
                    );
                    continue;
                }
                if let Some(ran) = watch
                    .ran_us
                    .filter(|ran| now_us - ran < trigger.cooldown_us)
                {
                    let rest = trigger.cooldown_us - (now_us - ran);
                    dropped(
                        &mut self.dropped,
                        watch,
                        &skill.name,
                        "cooldown",
                        now_us,
                        || format!("in its cooldown, {} left", left(rest)),
                    );
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
                    dropped(
                        &mut self.dropped,
                        watch,
                        &skill.name,
                        "quiet",
                        now_us,
                        || "quiet hours".into(),
                    );
                    continue;
                }
                // Snoozed or turned off by a person (#1685): the chance is
                // spent, so nothing waits to come when it ends.
                if self.off.contains(&skill.name) {
                    dropped(&mut self.dropped, watch, &skill.name, "off", now_us, || {
                        "off".into()
                    });
                    continue;
                }
                if let Some(until) = self
                    .snoozed
                    .get(&skill.name)
                    .filter(|until| now_us < **until)
                {
                    let rest = until - now_us;
                    dropped(
                        &mut self.dropped,
                        watch,
                        &skill.name,
                        "snooze",
                        now_us,
                        || format!("snoozed, {} left", left(rest)),
                    );
                    continue;
                }
                // The nudges' limits (#1685). Past the day's most, or resting
                // after runs nobody noticed, the chance is spent. Outside its
                // active hours, or with its group's hour spent or another
                // of the group waiting longer for it, the chance is kept as
                // in the quiet hours: taken at the trigger's first message
                // once they allow it, while it still holds.
                let day = day_of(self.clock, now_us);
                if let Some(most) = skill.most_a_day.filter(|most| {
                    self.day_runs
                        .get(&skill.name)
                        .is_some_and(|(on, count)| *on == day && count >= most)
                }) {
                    dropped(
                        &mut self.dropped,
                        watch,
                        &skill.name,
                        "most",
                        now_us,
                        || format!("its {most} a day already run"),
                    );
                    continue;
                }
                if let Some(nudge) = self
                    .nudges
                    .get(&skill.name)
                    .filter(|nudge| now_us < nudge.resting_until_us)
                {
                    let rest = nudge.resting_until_us - now_us;
                    dropped(
                        &mut self.dropped,
                        watch,
                        &skill.name,
                        "resting",
                        now_us,
                        || format!("resting after runs nobody noticed, {} left", left(rest)),
                    );
                    continue;
                }
                // After a night the lamp rested, the other skill's daily
                // greeting comes first (the first builder's ruling on
                // #1672): the chance is kept, as the quiet hours keep one,
                // while that trigger holds unbroken since this one first
                // waited, until it is judged. Its greeting given to this
                // face is this run's, done. Otherwise this chance is taken
                // as before, and claims that trigger's day (`claims`).
                let mut claims = None;
                if let Some((other, name)) = &trigger.yields_to {
                    let greeted = matches!(yielding, Yield::Fired(at_us)
                        if watch.since_us.is_some_and(|since| since <= at_us));
                    let waits = match yielding {
                        Yield::Pending { since_us, .. } => {
                            watch.yielded_us.is_none_or(|first| since_us <= first)
                        }
                        _ => false,
                    };
                    claims = match yielding {
                        Yield::Open(day) => Some((other, name, day)),
                        Yield::Pending { day, .. } if !waits => Some((other, name, day)),
                        _ => None,
                    };
                    let why = if waits {
                        watch.spent = false;
                        watch.waiting_us = None;
                        let first = watch.yielded_us.is_none();
                        watch.yielded_us.get_or_insert(now_us);
                        first.then(|| format!("waits: the {name} greeting comes first"))
                    } else {
                        greeted.then(|| {
                            format!("done: {other}'s {name} greeting was this face's hello")
                        })
                    };
                    if let Some(why) = why {
                        runs.push(Run {
                            generation: None,
                            skill: skill.name.clone(),
                            events: vec![event(
                                &skill.name,
                                SkillStage::Answered,
                                why,
                                Some(false),
                            )],
                            calls: Vec::new(),
                            ask: None,
                            queue: false,
                            from: None,
                        });
                    }
                    if greeted || waits {
                        continue;
                    }
                }
                let outside = skill
                    .active
                    .is_some_and(|active| minute.is_none_or(|minute| !within(minute, active)));
                let hour_spent = skill.budget.as_ref().is_some_and(|(group, _)| {
                    let limit = self
                        .skills
                        .iter()
                        .filter_map(|other| other.budget.as_ref())
                        .filter(|(named, _)| named == group)
                        .map(|(_, per_hour)| *per_hour)
                        .min()
                        .unwrap_or(u32::MAX);
                    self.group_runs.get(group).is_some_and(|began| {
                        began
                            .iter()
                            .filter(|at| now_us.saturating_sub(**at) < HOUR_US)
                            .count() as u64
                            >= u64::from(limit)
                    })
                });
                // First come, first served: with the hour free, a member
                // that has waited for it longer, and still waits, goes first,
                // so the first of the group in folder order never takes
                // every hour (#1685's wellbeing skills).
                let behind = skill.budget.as_ref().is_some_and(|(group, _)| {
                    self.budget_waits.get(group).is_some_and(|waits| {
                        let mine = waits.get(&skill.name).map_or(now_us, |(first, _)| *first);
                        waits.iter().any(|(name, (first, last))| {
                            *name != skill.name
                                && now_us.saturating_sub(*last) < BUDGET_WAIT_LIVE_US
                                && *first < mine
                        })
                    })
                });
                if (hour_spent || behind)
                    && !outside
                    && let Some((group, _)) = &skill.budget
                {
                    let wait = self
                        .budget_waits
                        .entry(group.clone())
                        .or_default()
                        .entry(skill.name.clone())
                        .or_insert((now_us, now_us));
                    wait.1 = now_us;
                }
                if outside || hour_spent || behind {
                    if trigger.when != When::Appears {
                        watch.spent = false;
                        watch.disarmed = false;
                        watch.waiting_us = None;
                    }
                    let group = skill
                        .budget
                        .as_ref()
                        .map_or("", |(group, _)| group.as_str());
                    // Each budget reason is its own gate, so a different
                    // one within 60 s is still said.
                    let (gate, why) = if outside {
                        ("active", "outside its active hours".to_owned())
                    } else if hour_spent {
                        ("budget-hour", format!("its {group} group's hour is spent"))
                    } else {
                        ("budget-first", format!("another {group} skill waits first"))
                    };
                    dropped(&mut self.dropped, watch, &skill.name, gate, now_us, || why);
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
                    } else if once && (trigger.once_per_day || trigger.wait_us > 0) {
                        if trigger.once_per_day
                            && let (Some(name), Some(day)) = (
                                &trigger.name,
                                moment.as_ref().and_then(|moment| {
                                    trigger
                                        .during
                                        .as_ref()?
                                        .occurrence_day(moment.day, moment.minute)
                                }),
                            )
                        {
                            self.judged
                                .insert((skill.name.clone(), name.clone()), (day, None));
                        }
                        // A chance let go says so, once: on lamp-06c1
                        // bedtime matured 2 s before the voice had rested
                        // its 30 s, and nothing said why it never came
                        // (#1672). Welcome back's arrivals after a say that
                        // expired were let go in silence, so any trigger's
                        // drop says it too, with the condition that held it
                        // (#2861); a daily one's line is as it was. Without a
                        // `wait` a chance held back is spent, as before.
                        let mut why = format!(
                            "{} chance dropped: it held, but the skill's conditions did not hold within its wait of {} s; it comes again only once the trigger ends and holds again",
                            trigger.name.as_deref().unwrap_or("trigger"),
                            trigger.wait_us / 1_000_000
                        );
                        if !trigger.once_per_day {
                            why = format!(
                                "{why} (held by {})",
                                Self::held_back(&self.latest, &self.holding, skill, now_us)
                            );
                        }
                        runs.push(Run {
                            generation: None,
                            skill: skill.name.clone(),
                            events: vec![event(
                                &skill.name,
                                SkillStage::Answered,
                                why,
                                Some(false),
                            )],
                            calls: Vec::new(),
                            ask: None,
                            queue: false,
                            from: None,
                        });
                    }
                    // Without a `wait`, a chance a condition holds back is
                    // spent here and says so (#2602). One let go past a
                    // `wait` is said by the `chance dropped` line above,
                    // which #3007 gives every trigger with a `wait`, not
                    // here: it must not be said twice.
                    if trigger.wait_us == 0 && !(once && trigger.once_per_day) {
                        let unmet = Self::unmet(&self.latest, &self.holding, skill, now_us);
                        dropped(
                            &mut self.dropped,
                            watch,
                            &skill.name,
                            "while",
                            now_us,
                            || format!("its while did not hold: {unmet}"),
                        );
                    }
                    continue;
                }
                watch.waiting_us = None;
                if trigger.when == When::Changes
                    && (Self::fresh(&self.latest, topic, now_us).is_none()
                        || !watch.changes.observe(&trigger.changes, &value))
                {
                    continue;
                }
                let acts = self.skip_ask
                    || self.ask_policy
                    || skill.kind == SkillKind::Addon
                    || skill.permissions.grants("sleep");
                // Spoken to since the window opened today (the first
                // builder's ruling on bedtime-morning): no greeting, and a
                // daily trigger's day is spent, said once.
                let spoken = trigger.unless_heard
                    && trigger
                        .during
                        .as_ref()
                        .and_then(|window| {
                            opened_us_ago(self.clock, self.daily_clock_trusted, window, now_us)
                        })
                        .is_some_and(|opened_us| {
                            self.heard
                                .at_us
                                .is_some_and(|at| now_us.saturating_sub(at) < opened_us)
                        });
                if spoken && !(trigger.once_per_day && acts) {
                    let opens = trigger
                        .during
                        .as_ref()
                        .map_or_else(String::new, daily::Window::opens);
                    dropped(
                        &mut self.dropped,
                        watch,
                        &skill.name,
                        "spoken",
                        now_us,
                        || format!("spoken to since {opens}"),
                    );
                    continue;
                }
                if trigger.once_per_day && acts {
                    if self.daily_faulted {
                        dropped(
                            &mut self.dropped,
                            watch,
                            &skill.name,
                            "daily",
                            now_us,
                            || "its daily journal is faulted".into(),
                        );
                        continue;
                    }
                    let day = moment.as_ref().and_then(|moment| {
                        trigger
                            .during
                            .as_ref()?
                            .occurrence_day(moment.day, moment.minute)
                    });
                    let claim = match (&mut self.daily, day, trigger.name.as_deref()) {
                        (Some(journal), Some(day), Some(name)) => {
                            journal.claim(&skill.name, name, day)
                        }
                        _ => Err(std::io::Error::other(
                            "daily trigger needs a writable journal and a known local clock",
                        )),
                    };
                    // What a `yields_to` reads: judged today, and when it
                    // took its chance.
                    if let (Ok(true), Some(day), Some(name)) = (&claim, day, &trigger.name) {
                        self.judged.insert(
                            (skill.name.clone(), name.clone()),
                            (day, (!spoken).then_some(now_us)),
                        );
                    }
                    match claim {
                        Ok(true) if spoken => {
                            runs.push(Run {
                                generation: None,
                                skill: skill.name.clone(),
                                events: vec![event(
                                    &skill.name,
                                    SkillStage::Answered,
                                    format!(
                                        "{} chance let go: spoken to since {}",
                                        trigger.name.as_deref().unwrap_or("trigger"),
                                        trigger
                                            .during
                                            .as_ref()
                                            .map_or_else(String::new, daily::Window::opens)
                                    ),
                                    Some(false),
                                )],
                                calls: Vec::new(),
                                ask: None,
                                queue: false,
                                from: None,
                            });
                            continue;
                        }
                        Ok(true) => {}
                        Ok(false) => {
                            dropped(
                                &mut self.dropped,
                                watch,
                                &skill.name,
                                "daily",
                                now_us,
                                || "its day already claimed".into(),
                            );
                            continue;
                        }
                        Err(error) => {
                            self.daily_faulted = true;
                            runs.push(Run {
                                generation: Some(self.generation.clone()),
                                skill: skill.name.clone(),
                                events: vec![event(
                                    &skill.name,
                                    SkillStage::Answered,
                                    format!("daily chance withheld: {error}"),
                                    Some(false),
                                )],
                                calls: Vec::new(),
                                ask: None,
                                queue: false,
                                from: Some((index, None)),
                            });
                            continue;
                        }
                    }
                }
                // This hello is the morning's moment (the first builder on
                // #2757): the trigger it yields to has that day claimed, as
                // its own greeting would, so a later face is not given it.
                if acts && let Some((other, name, day)) = claims {
                    let claimed = match &mut self.daily {
                        Some(journal) => journal.claim(other, name, day),
                        None => Err(std::io::Error::other("no daily journal")),
                    };
                    let why = match claimed {
                        Ok(true) => {
                            self.judged
                                .insert((other.clone(), name.clone()), (day, None));
                            Some(format!(
                                "{name} chance let go: {} already greeted this morning",
                                skill.name
                            ))
                        }
                        Ok(false) => None,
                        Err(error) => {
                            self.daily_faulted = true;
                            Some(format!("daily chance withheld: {error}"))
                        }
                    };
                    if let Some(why) = why {
                        runs.push(Run {
                            generation: None,
                            skill: other.clone(),
                            events: vec![event(other, SkillStage::Answered, why, Some(false))],
                            calls: Vec::new(),
                            ask: None,
                            queue: false,
                            from: None,
                        });
                    }
                }
                let held = match trigger.when {
                    When::Appears => format!(
                        "it appeared on {topic} after {} s without",
                        watch.away_us / 1_000_000
                    ),
                    When::Present => format!("{topic} has held for {} s", held_us / 1_000_000),
                    When::Changes => format!("{topic} selected values changed or became eligible"),
                };
                let mut question = question(skill);
                if question["act"] == "hello" {
                    question["away_min"] = json!(watch.away_us / 60_000_000);
                    question["there_s"] = json!(held_us / 1_000_000);
                }
                if acts {
                    watch.ran_us = Some(now_us);
                    watch.said.clear();
                    // Counted as the cooldown is: when the run is started,
                    // or asked of the Policy (#1685).
                    if skill.most_a_day.is_some() {
                        let count = self.day_runs.entry(skill.name.clone()).or_insert((day, 0));
                        if count.0 != day {
                            *count = (day, 0);
                        }
                        count.1 += 1;
                    }
                    if let Some((group, _)) = &skill.budget {
                        let began = self.group_runs.entry(group.clone()).or_default();
                        began.retain(|at| now_us.saturating_sub(*at) < HOUR_US);
                        began.push_back(now_us);
                        self.group_ran.insert(skill.name.clone(), now_us);
                        if let Some(waits) = self.budget_waits.get_mut(group) {
                            waits.remove(&skill.name);
                            waits.retain(|_, (_, last)| {
                                now_us.saturating_sub(*last) < BUDGET_WAIT_LIVE_US
                            });
                        }
                    }
                    // Asked of the Policy, a run is watched for once it is
                    // made ([`Engine::made`]): a no shows nothing to ignore.
                    if !self.ask_policy {
                        nudged(&mut self.nudges, &self.latest, skill, now_us);
                    }
                    // Its end is told even if the Policy says no: with no
                    // run holding anything, the program lets it pass.
                    watch.open = skill.holds_until_ends();
                }
                let run = decide(
                    skill,
                    skill
                        .scheduled_run(Some(trigger))
                        .then_some(&self.generation),
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
        self.tool_with_requested_audio(name, args, now_us, false)
    }

    /// The runtime's explicit opt-in; the private spoken context is still
    /// required by the offer authority before the result can leave Skills.
    pub fn tool_with_requested_audio(
        &mut self,
        name: &str,
        args: &Value,
        now_us: u64,
        permitted: bool,
    ) -> Result<Run, String> {
        // A search's `found` is the program's to answer, never a run: here
        // (the skill folders' recorded tests) it is refused, so it is never
        // taken as a look at the desk (#2262).
        if self
            .skills
            .iter()
            .any(|skill| skill.tool.as_deref() == Some(name) && skill.search_found(args).is_some())
        {
            return Err(FOUND_BEFORE_A_LOOK.into());
        }
        // Before a record is told, only the names the eyes still say said
        // yes are kept: a no, a forget or a reset lets the rest go (#1997).
        if self.naming
            && let Some(path) = self.known_file.as_deref()
        {
            self.visits
                .keep_names(&known::tellable_in(&known::tellable_of(path)));
        }
        let sharing: Vec<String> = self
            .skills
            .iter()
            .filter(|skill| skill.stop.as_ref().is_some_and(|stop| stop.tool == name))
            .map(|skill| skill.name.clone())
            .collect();
        if !sharing.is_empty() {
            return self.stop(&sharing, name, args, now_us);
        }
        let skill = self
            .skills
            .iter()
            .find(|skill| skill.tool.as_deref() == Some(name))
            .ok_or("No skill of mine does that just now.")?;
        if skill.requested_audio().is_some() && !permitted {
            return Err("Requested audio is disabled or has no spoken context.".into());
        }
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
            generation: skill.scheduled_run(None).then(|| self.generation.clone()),
            skill: skill.name.clone(),
            events: vec![event(
                &skill.name,
                SkillStage::Triggered,
                format!("the model called {name} with {args}"),
                None,
            )],
            calls: self.told_history(
                skill,
                calls(&self.latest, skill, None, now_us, args, None),
                now_us,
            ),
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
    generation: Option<&Arc<()>>,
    held: String,
    (skip_ask, ask_policy): (bool, bool),
    question: Value,
    made: impl FnOnce() -> Vec<Call>,
) -> Run {
    // An add-on asks nobody here: what it proposes goes to its judge, which
    // keeps or drops each proposal (`docs/addons.md`). Nor does the night's
    // sleep (#940): it is the creature's own, as the built-in one was.
    let unasked = match (skill.kind, skill.permissions.grants("sleep")) {
        (SkillKind::Addon, _) => Some("an add-on: its proposals go to the judge on addon/proposal"),
        (_, true) => Some("the lamp's own sleep: asked of nobody"),
        _ => None,
    };
    if let Some(why) = unasked {
        let events = vec![
            event(&skill.name, SkillStage::Triggered, held, None),
            event(&skill.name, SkillStage::Answered, why.into(), Some(true)),
        ];
        return Run {
            generation: generation.cloned(),
            skill: skill.name.clone(),
            events,
            calls: made(),
            ask: None,
            queue: false,
            from: None,
        };
    }
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
        generation: generation.cloned(),
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

/// Whether a trigger's message chooses something, for a skill whose
/// `[choice]` is read from it (`field`, #1669 (e)): only a value in
/// `one_of` lets it hold, so a message that chooses nothing neither runs
/// the skill, nor asks the Policy, nor starts its cooldown (the reviewer on
/// #2012: a departure from the middle used up Watch you leave's ten
/// minutes). Every other skill's trigger is as its test says.
fn chooses(skill: &Skill, value: &Value) -> bool {
    skill
        .choice
        .as_ref()
        .and_then(|choice| Some((choice.field.as_deref()?, choice)))
        .is_none_or(|(field, choice)| {
            value[field]
                .as_str()
                .is_some_and(|chosen| choice.one_of.iter().any(|one| one == chosen))
        })
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
        .and_then(|choice| match (&choice.field, trigger) {
            // The trigger's own message, as it fired (#1669 (e)).
            (Some(field), Some(trigger)) => latest
                .get(&trigger.topic)
                .and_then(|(value, _, _)| value[field.as_str()].as_str()),
            (Some(_), None) => None,
            (None, _) => args[&choice.name].as_str().or(choice.default.as_deref()),
        });
    let twice = skill
        .flag
        .as_ref()
        .is_some_and(|flag| args[&flag.name] == true);
    skill
        .steps
        .iter()
        .filter(|(on, _)| match on {
            None => true,
            Some(Only::Trigger(on)) => Some(on) == fired,
            Some(Only::Choice(values)) => {
                chosen.is_some_and(|chosen| values.iter().any(|value| value == chosen))
            }
        })
        .flat_map(|(_, step)| match step {
            Step::Call(Call::Gaze { .. }) => face
                .map(|face| Call::Gaze { face: Some(face) })
                .into_iter()
                .collect(),
            // An item's run names its task to the voice, by a numeric key,
            // and only on a `from = "tasks"` ask: the voice refuses `task`
            // on any other (#1237).
            Step::Call(Call::SayFrom(source, _)) => {
                let task = matches!(*source, Source::Tasks | Source::Onboarding)
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
/// each skill's tool, described with the skill's description. Skills from
/// [`load_all`] have a name each ([`load_each`]); in any other list a name an
/// earlier skill took is left out, as [`Engine::tool`] runs the first.
///
/// Beside `tools`, `body` names the tools whose skill asks for the body
/// (`[permissions] body`), when any does: a text from the Autonomous app
/// never runs one, since a lamp that moves for a text nobody in the room sent
/// looks wrong (#1673, the first builder, 19 Oct). A key of the file's own,
/// not of each tool, which the voice's function declarations refuse.
/// `core` names the tools of core skills ([`SkillKind::Core`]), when any:
/// at a session's bound of functions the voice never cuts one of them, and
/// cuts others first (the owner's rule, on #2614).
/// `searches` gives, by tool, a search's note and walk, when any says them
/// (`[search]`'s `note`, `walk` and `looked`, and its choice's `default`):
/// the second voice route's note and walk read them (docs/second-voice.md).
/// The lists include enabled skills only, even when given the inventory.
pub fn tools_file(skills: &[Skill]) -> Value {
    tools_file_with(
        skills,
        requested_audio::from_environment(),
        watch::words_v2(std::env::var(watch::WORDS_ENVIRONMENT).ok().as_deref()),
    )
}

/// Explicit switch form, including deterministic tests. Package validation
/// uses the loader and does not grant execution or advertisement.
/// Body and core metadata follow the same enabled/media filters and first
/// emitted name as the tools; an omitted definition cannot add a body grant
/// or core priority.
pub fn tools_file_with_requested_audio(skills: &[Skill], enabled: bool) -> Value {
    tools_file_with(skills, enabled, false)
}

/// [`tools_file_with_requested_audio`] with guard mode's words too:
/// `guard_words_v2` gives `keep_watch` [`watch::WORDS_V2_DESCRIPTION`] and
/// [`watch::WORDS_V2_TURN`] in place of its `SKILL.md` description and its
/// `[choice]` `about` (#2827, `LAMP_GUARD_WORDS_V2`); off, every tool is
/// as its skill says, byte for byte.
pub fn tools_file_with(skills: &[Skill], enabled: bool, guard_words_v2: bool) -> Value {
    let mut tools: Vec<Value> = Vec::new();
    let mut moving: Vec<&str> = Vec::new();
    let mut core: Vec<&str> = Vec::new();
    let mut searches = serde_json::Map::new();
    for skill in skills.iter().filter(|skill| skill.enabled) {
        if skill.requested_audio().is_some() && !enabled {
            continue;
        }
        if let Some(name) = &skill.tool
            && !tools.iter().any(|tool| tool["name"] == *name)
        {
            if skill.permissions.body {
                moving.push(name);
            }
            if skill.kind == SkillKind::Core {
                core.push(name);
            }
            if let Some(search) = &skill.search
                && let Some(note) = &search.note
            {
                let default = skill
                    .choice
                    .as_ref()
                    .and_then(|choice| choice.default.clone());
                searches.insert(
                    name.clone(),
                    json!({"note": note, "walk": search.walk, "looked": search.looked, "default": default}),
                );
            }
            let reworded = guard_words_v2 && skill.name == watch::GUARD;
            let description = if reworded {
                watch::WORDS_V2_DESCRIPTION
            } else {
                skill.description.as_str()
            };
            let mut tool = json!({"name": name, "description": description});
            let mut properties = serde_json::Map::new();
            if let Some(choice) = &skill.choice {
                let about = if reworded {
                    watch::WORDS_V2_TURN
                } else {
                    choice.about.as_str()
                };
                properties.insert(
                    choice.name.clone(),
                    json!({"type": "string", "enum": choice.one_of, "description": about}),
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
        // One function a skill to snooze it or turn it off (#1685): counted
        // as any skill's against a session's 32, and reached through
        // `use_skill` with `LAMP_USE_SKILL=on`.
        if let Some(stop) = &skill.stop
            && !tools.iter().any(|tool| tool["name"] == *stop.tool)
        {
            let mut properties = serde_json::Map::new();
            properties.insert(
                STOP_CHOICE.into(),
                json!({
                    "type": "string",
                    "enum": STOP_WAYS,
                    "description": "snooze: none until tomorrow morning; off: none until they ask for them again; on: as before again"
                }),
            );
            // Shared by a group's skills: one function, and a yes-or-no for
            // all of them (#1685).
            let shared = skills
                .iter()
                .filter(|other| {
                    other
                        .stop
                        .as_ref()
                        .is_some_and(|theirs| theirs.tool == stop.tool)
                })
                .count()
                > 1;
            if shared {
                properties.insert(
                    STOP_ALL.into(),
                    json!({
                        "type": "boolean",
                        "description": "true for every one of them; left out for the one that came last"
                    }),
                );
            }
            tools.push(json!({
                "name": stop.tool,
                "description": stop.about,
                "parameters": {"type": "object", "properties": properties}
            }));
        }
    }
    // "Never mind" is declared whenever the unplug skill is loaded, so a Live
    // session that asked for the unplug already has it; with no latch in
    // force the call is refused (`unplug::may_stay`). It moves the body.
    if skills.iter().any(Skill::prepares_unplug)
        && !tools.iter().any(|tool| tool["name"] == unplug::STAY_TOOL)
    {
        tools.push(unplug::stay_tool());
        moving.push(unplug::STAY_TOOL);
    }
    let mut file = json!({ "tools": tools });
    if !moving.is_empty() {
        file["body"] = json!(moving);
    }
    if !core.is_empty() {
        file["core"] = json!(core);
    }
    if !searches.is_empty() {
        file["searches"] = Value::Object(searches);
    }
    file
}

/// What the model is told when its call ran these calls. With a look, to
/// say only that it will look, and `"look": true`, by which the voice takes
/// its full look itself, whatever the model says (#941): the still comes in
/// the model's next turn. With a say, the note, for its answer.
pub fn told(calls: &[Call]) -> Value {
    if calls.contains(&Call::PrepareUnplug) {
        return json!({
            "ready": false,
            "next": "Say only that you are preparing to fold and they must wait. Do not say it is safe to unplug yet. The skill will speak again only after checking the body."
        });
    }
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

/// The day a case's `clock` is on: midnight UTC, 1 January 2026.
const REPLAY_DAY_MS: u64 = 1_767_225_600_000;

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
    // A name the skill's tool is not, nor its `[stop]` function's, would
    // make nothing and pass as a quiet case: it is refused.
    let stop = skill.stop.as_ref().map(|stop| stop.tool.clone());
    if let Some(tool) = tools.iter().find(|tool| {
        skill.tool.as_deref() != Some(tool.name.as_str())
            && stop.as_deref() != Some(tool.name.as_str())
    }) {
        return Err(format!(
            "call_tool {:?}: this skill's tool is {:?}, its [stop] function {:?}",
            tool.name,
            skill.tool.as_deref().unwrap_or("none"),
            stop.as_deref().unwrap_or("none")
        ));
    }
    let mut engine = Engine::new(vec![skill], true);
    let messages = read_recordings(recording)?;
    let first_us = messages.first().map_or(0, |message| message.header.at_us);
    // On a real date (1 January 2026, UTC), since a trigger's `during`
    // needs a wall clock that was set; quiet hours and `[active]` read only
    // the minute of the day, and `most_a_day` only where a day turns.
    let clock_ms = clock
        .map(|clock| minute_of_day(clock).map(|minute| REPLAY_DAY_MS + u64::from(minute) * 60_000))
        .transpose()?;
    if let Some(clock_ms) = clock_ms {
        engine.clock(0, first_us, clock_ms);
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
            // A `[stop]` call makes nothing: its words are the model's
            // answer (#1685).
            if let Ok(run) = engine.tool(name, &args, first_us + at)
                && stop.as_deref() != Some(name)
            {
                keep(made, run.calls, at);
            }
        }
    };
    for message in messages {
        let at_us = message.header.at_us;
        // The clock told again at every message, as a live host tells it
        // (`clock_in_with_trust`), so a trigger's `during` window, which
        // needs a local time told within 2 minutes, is replayed over hours.
        if let Some(clock_ms) = clock_ms {
            engine.clock(0, at_us, clock_ms + (at_us - first_us) / 1_000);
        }
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

/// In a watch's case, the eyes' known file from `at` on: the name of the
/// known person at the lamp, or with no `name`, nobody known there.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnownAt {
    pub at: String,
    pub name: Option<String>,
}

/// What a watch skill does in one case ([`watch`]): its recordings, the
/// model's calls of its tool and the known faces, each in the order of its
/// time (at one time, the messages first, then what the eyes know, then the
/// call), the eyes' known file looked at after each, as the program looks at
/// it as it goes. Its mark is `light`, the mark given back `ends`, a text
/// `text`, a turn of the head `body.gaze` and its words aloud `say`. With
/// `clock`, the case's first message is at that local time. Arrivals by
/// sound are taken, as the program takes them with `LAMP_GUARD_SOUNDS=on`.
pub fn replay_watch(
    dir: &Path,
    recording: &Path,
    tools: &[ToolCall],
    clock: Option<&str>,
    known: &[KnownAt],
) -> Result<Vec<Made>, String> {
    let skill = load(dir)?;
    if skill.watch.is_none() {
        return Err("[[known]] is a watch's: this skill has no [watch]".into());
    }
    let mut watches = watch::Watches::default();
    watches.sounds = true;
    watches.replace(std::slice::from_ref(&skill));
    let messages = read_recordings(recording)?;
    let first_us = messages.first().map_or(0, |message| message.header.at_us);
    let start = clock.map(minute_of_day).transpose()?;
    let minute =
        |at_us: u64| start.map(|start| ((u64::from(start) + at_us / 60_000_000) % 1_440) as u16);
    // Each event at its time from the first message, in the order above.
    enum Event<'a> {
        Message(&'a Recorded),
        Known(Option<&'a str>),
        Call(&'a ToolCall),
    }
    let mut events: Vec<(u64, u8, Event)> = Vec::new();
    for message in &messages {
        events.push((message.header.at_us - first_us, 0, Event::Message(message)));
    }
    for entry in known {
        events.push((
            duration_us(&entry.at)?,
            1,
            Event::Known(entry.name.as_deref()),
        ));
    }
    for tool in tools {
        if skill.tool.as_deref() != Some(tool.name.as_str()) {
            return Err(format!(
                "call_tool {:?}: this skill's tool is {:?}",
                tool.name,
                skill.tool.as_deref().unwrap_or("none")
            ));
        }
        events.push((duration_us(&tool.at)?, 2, Event::Call(tool)));
    }
    events.sort_by_key(|(at, rank, _)| (*at, *rank));
    let mut made = Vec::new();
    let keep = |made: &mut Vec<Made>, outs: Vec<watch::Out>, at_us: u64| {
        for out in outs {
            let call = match out {
                watch::Out::Mark(call) => call,
                watch::Out::Unmark => Call::Ends,
                watch::Out::Text(text) | watch::Out::Named { text, .. } => Call::Text(text),
                watch::Out::Gaze(call) => call,
                watch::Out::Say(words) => Call::Words(words),
                watch::Out::Said(_) => continue,
            };
            made.push(Made { at_us, call });
        }
    };
    let mut known_now: Option<&str> = None;
    for (at, _, event) in events {
        let outs = match event {
            // What the ring says of the mark, as the program takes it.
            Event::Message(message) => match ring_said(message) {
                Some((source, shown, by)) => match source.strip_prefix("skill:") {
                    Some(name) if name == skill.name => watches
                        .ring_said(name, shown, by.as_deref(), at)
                        .into_iter()
                        .map(|(_, out)| out)
                        .collect(),
                    _ => Vec::new(),
                },
                None => watches
                    .observe(&message.header.topic, &message.payload, at, minute(at))
                    .into_iter()
                    .map(|(_, out)| out)
                    .collect(),
            },
            Event::Known(name) => {
                known_now = name;
                Vec::new()
            }
            Event::Call(tool) => {
                let args = if tool.args.is_empty() {
                    Value::Null
                } else {
                    serde_json::to_value(&tool.args).map_err(|error| error.to_string())?
                };
                match watches.tool(&tool.name, &args, at, known_now, true) {
                    Some(Ok((_, _, outs))) => outs,
                    _ => Vec::new(),
                }
            }
        };
        keep(&mut made, outs, at);
        let looked = watches.known(known_now, at, minute(at));
        keep(
            &mut made,
            looked.into_iter().map(|(_, out)| out).collect(),
            at,
        );
    }
    Ok(made)
}

/// What a recorded message of the ring says of a request's light: its
/// claim's source, whether it is shown (`light/reached`) or not
/// (`light/outranked`), and by what.
fn ring_said(message: &Recorded) -> Option<(String, bool, Option<String>)> {
    match message.header.topic.as_str() {
        lamp_msgs::LightReached::TOPIC => {
            let reached: lamp_msgs::LightReached = serde_json::from_slice(&message.payload).ok()?;
            Some((reached.source, true, None))
        }
        lamp_msgs::LightOutranked::TOPIC => {
            let outranked: lamp_msgs::LightOutranked =
                serde_json::from_slice(&message.payload).ok()?;
            Some((outranked.source, false, Some(outranked.by)))
        }
        _ => None,
    }
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
    /// A watch's: the face the eyes know at the lamp, as time goes.
    #[serde(default)]
    known: Vec<KnownAt>,
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
    let watches = load(dir)?.watch.is_some();
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
        let made = if watches {
            replay_watch(
                dir,
                case,
                &expect.call_tool,
                expect.clock.as_deref(),
                &expect.known,
            )?
        } else if !expect.known.is_empty() {
            return Err(wrong(
                "[[known]] is a watch's: this skill has no [watch]".into(),
            ));
        } else {
            replay_with(dir, case, &expect.call_tool, expect.clock.as_deref())?
        };
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

    #[test]
    fn a_change_trigger_requires_bounded_paths_and_compatible_options() {
        let read = |options: &str| {
            toml::from_str::<TriggerFile>(&format!(
                "topic = \"schedule/due\"\nfield = \"pending\"\nabove = 0.5\n{options}"
            ))
            .map_err(|error| error.to_string())
            .and_then(trigger)
        };
        let selected = "when = \"changes\"\nchanges = [\"pending\", \"oldest.id\"]\n";
        let valid = read(selected).unwrap();
        assert_eq!(valid.when, When::Changes);
        assert_eq!(valid.changes, ["pending", "oldest.id"]);
        let most = serde_json::to_string(
            &(0..MOST_CHANGE_FIELDS)
                .map(|i| format!("p{i}"))
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert!(read(&format!("when = \"changes\"\nchanges = {most}\n")).is_ok());
        for fields in [
            "[]".to_owned(),
            "[\"\"]".to_owned(),
            "[\"oldest..id\"]".to_owned(),
            "[\"line\\nfeed\"]".to_owned(),
            "[\"pending\", \"pending\"]".to_owned(),
            format!("[\"{}\"]", "p".repeat(MOST_CHANGE_PATH_BYTES + 1)),
            serde_json::to_string(
                &(0..=MOST_CHANGE_FIELDS)
                    .map(|i| format!("p{i}"))
                    .collect::<Vec<_>>(),
            )
            .unwrap(),
        ] {
            assert!(
                read(&format!("when = \"changes\"\nchanges = {fields}\n")).is_err(),
                "{fields}"
            );
        }
        assert!(read("when = \"changes\"\n").is_err());
        for mode in ["appears", "present"] {
            assert!(read(&format!("when = \"{mode}\"\nchanges = [\"pending\"]\n")).is_err());
            let unchanged = read(&format!("when = \"{mode}\"\n")).unwrap();
            assert!(unchanged.changes.is_empty());
        }
        for extra in [
            "each = \"items\"\nkey = \"id\"",
            "key = \"id\"",
            "near = 0.14",
            "present = true",
            "absent_for = \"0s\"",
            "for = \"0s\"",
            "gap = \"0s\"",
            "rearm = \"ends\"",
            "cooldown = \"0s\"",
            "wait = \"0s\"",
            "rise = { by = 1, within = \"1s\" }",
            "during = { from = \"08:00\", to = \"09:00\" }",
            "once_per_day = true",
            "unless_heard = true",
            "yields_to = { skill = \"hello\", trigger = \"morning\" }",
        ] {
            assert!(read(&format!("{selected}{extra}\n")).is_err(), "{extra}");
        }
        // A face trigger without a field must not silently become a change
        // trigger whose test differs from the scalar selection it names.
        let faces: TriggerFile =
            toml::from_str(&format!("topic = \"humans/faces/tracked\"\n{selected}")).unwrap();
        assert!(trigger(faces).is_err());
    }

    #[test]
    fn a_change_trigger_requires_a_scalar_comparison() {
        let read = |field: &str, wanted: &str, selected: &str, mode: &str| {
            let selection = if mode == "changes" {
                format!("changes = [\"{selected}\"]\n")
            } else {
                String::new()
            };
            let file: TriggerFile = toml::from_str(&format!(
                "topic = \"air/indoor\"\nfield = {field:?}\nequals = {wanted}\nwhen = {mode:?}\n{selection}"
            ))
            .unwrap();
            trigger(file)
        };
        // Scalar predicates retain exact selections and independent fields.
        for (field, wanted, selected) in [
            ("count", "1", "count"),
            ("ready", "true", "count"),
            ("state", "\"ready\"", "count"),
            ("object.ready", "true", "object.count"),
        ] {
            read(field, wanted, selected, "changes").unwrap();
        }
        // Existing modes retain compound comparisons.
        for mode in ["present", "appears"] {
            read("object", "{ count = 1 }", "object.count", mode).unwrap();
            read("values", "[1]", "count", mode).unwrap();
        }
        for (field, wanted, selected) in [
            ("object", "{ count = 1 }", "object.count"),
            ("values", "[1]", "count"),
        ] {
            let error = read(field, wanted, selected, "changes")
                .expect_err("Changes must not accept a compound comparison");
            assert!(error.contains("needs a scalar comparison"), "{error}");
        }
    }

    /// #1669: a trigger's field may step into an object with a dot, as
    /// `ears/classes` keeps its scores (`scores.music`); a plain name is
    /// read as before, and a step that is not there is null, never a match.
    #[test]
    fn a_dotted_field_reads_inside_an_object() {
        let classes = json!({"status": "scored", "scores": {"music": 0.8, "laughter": 0.1}});
        assert!(Test::Above("scores.music".into(), 0.5).holds(&classes));
        assert!(!Test::Above("scores.laughter".into(), 0.5).holds(&classes));
        assert!(Test::Equals("status".into(), json!("scored")).holds(&classes));
        assert!(!Test::Number("scores.speech".into()).holds(&classes));
        assert!(!Test::Above("status.music".into(), 0.0).holds(&classes));
        let unavailable = json!({"status": "unavailable", "reason": "privacy"});
        assert!(!Test::Above("scores.music".into(), 0.0).holds(&unavailable));
    }

    /// #940: with the switch, a gesture or a move says who asks; without
    /// it, the datagram is as before.
    /// #1684 (a): a rise compares with the lowest reading of its while
    /// before the newest, never with the newest itself, and forgets what
    /// is older than the while.
    #[test]
    fn a_rise_is_over_the_lowest_reading_of_its_while_before() {
        let rise = Rise {
            field: "pm2_5".into(),
            by: 100.0,
            within_us: 180_000_000,
        };
        let mut readings = Readings::default();
        let s = |seconds: u64| seconds * 1_000_000;
        assert!(!readings.rose(500.0, s(0), &rise), "nothing before it");
        assert!(!readings.rose(90.0, s(5), &rise));
        assert!(!readings.rose(150.0, s(10), &rise));
        assert!(readings.rose(190.0, s(15), &rise), "100 over 90");
        assert!(readings.risen);
        assert!(
            readings.rose(400.0, s(185), &rise),
            "90 at 5 s is still in it"
        );
        assert!(
            !readings.rose(240.0, s(190), &rise),
            "now the lowest is 150"
        );
        assert!(!readings.risen);
        assert!(readings.kept.len() <= 3, "{:?}", readings.kept);
        // A climb every second keeps every reading, at most an hour's.
        let mut climbing = Readings::default();
        let long = Rise {
            within_us: 3_600_000_000,
            ..rise
        };
        for at in 0..5_000 {
            climbing.rose(at as f64, s(at), &long);
        }
        assert_eq!(climbing.kept.len(), MOST_RISE_READINGS);
    }

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
                    // A say of either kind reads its words' topic.
                    "[permissions]\ntopics = [\"schedule/due\", \"tasks/status\"]\nspeak = true\n\
                     [trigger]\ntopic = \"schedule/due\"\nfield = \"pending\"\nabove = 0.5\n\
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
