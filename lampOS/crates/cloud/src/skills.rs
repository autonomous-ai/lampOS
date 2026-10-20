//! The app's skills page (#1722, the first builder's 17:21 on mobile's
//! design): `skills.list`, every skill folder on the lamp as the running
//! `lamp-skills` loads it, each with its group, whether it can be removed or
//! turned off, and what it may use in plain words; and `skills.remove`, an
//! installed or made-here skill taken off on the person's tap, through
//! `lamp-skills`' own remove ([`remove`]), never a core one. Nothing here
//! installs or changes a skill. Off unless `LAMP_SKILLS_PAGE=on`, with
//! `LAMP_CLOUD=on`; without it each kind is answered as any this lamp
//! lacks, `not on this lamp: skills.list` (`docs/cloud.md`, "The skills
//! page").
//!
//! The skills are read where `lamp-skills` reads them, its folder
//! (`/var/lib/lampOS/skills`, the product unit's `--dir`), with its own
//! loader ([`lamp_skills::load_each`]), so the page says what `lamp-skills
//! list` says: no new socket, topic or copy of the schema.
use lamp_skills::{Permissions, SkillKind};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// `on` answers `skills.list` and `skills.remove`, with `LAMP_CLOUD=on`. Anything else: the
/// kind is one this lamp lacks.
pub const ENVIRONMENT: &str = "LAMP_SKILLS_PAGE";

/// The reply's own version. Adding a field keeps 1; removing, renaming or
/// retyping one is 2.
pub const VERSION: u64 = 1;

/// The most skills one reply lists; the rest are counted in `left_out`, so
/// a reply stays one small MQTT message.
pub const MOST_SKILLS: usize = 64;
/// The most bytes of a title, a description and a reason a skill does not
/// load, each cut at a character's edge.
pub const TITLE_MOST: usize = 80;
pub const DESCRIPTION_MOST: usize = 1_024;
pub const WHY_MOST: usize = 300;
/// The most bytes read of a `SKILL.md` or a `skill.toml`, and of the off file.
const FILE_MOST: u64 = 64 * 1_024;

/// What a person makes on this lamp by talking to it says so in its
/// `SKILL.md` front matter, `made: here`: an owner's skill, its own group
/// ("Made on this lamp", the first builder's 17:21). Only an installable
/// skill can be one; a core skill that says it is still core.
pub const MADE_HERE: &str = "here";

/// The privacy switch's line, beside what a camera or microphone skill may
/// use (the first builder's 17:21: "since that's what a person will ask").
pub const PRIVACY: &str = "the privacy switch stops it";

/// Whether the switch is on: both `LAMP_CLOUD` and `LAMP_SKILLS_PAGE` are `on`.
pub fn gates(cloud: Option<&str>, page: Option<&str>) -> bool {
    cloud == Some("on") && page == Some("on")
}

/// [`gates`] as the environment says.
pub fn on_from_environment() -> bool {
    gates(
        std::env::var("LAMP_CLOUD").ok().as_deref(),
        std::env::var(ENVIRONMENT).ok().as_deref(),
    )
}

/// Where the skills are read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Places {
    /// `lamp-skills`' folder, core and installable skills side by side.
    pub folder: PathBuf,
    /// The skills a person turned off by voice, a name a line, kept by
    /// `lamp-skills --state-dir` (#1886); a snooze is kept only in its
    /// memory, so it is never known here.
    pub off: PathBuf,
    /// The manage helper's socket, root's, which removes a skill for the
    /// page ([`lamp_skills::package::MANAGE_SOCKET`]): `lamp-cloud` never
    /// writes the folder itself.
    pub manage: PathBuf,
}

impl Places {
    /// The product's: the unit's `--dir`, and its `--state-dir`'s off file
    /// (`deploy/product/lamp-skills.service`, the folder #1732 shares).
    pub fn product() -> Self {
        Self {
            folder: "/var/lib/lampOS/skills".into(),
            off: "/var/lib/lampos-skills-state/off".into(),
            manage: lamp_skills::package::MANAGE_SOCKET.into(),
        }
    }
}

/// A skill's group on the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    /// Comes with the lamp, always installed, never removed.
    Core,
    /// Put on by its owner, from a folder or a package.
    Installed,
    /// Made on this lamp by its person ([`MADE_HERE`]).
    MadeHere,
}

/// One thing a skill may use: a name the app keys on, and its line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Use {
    pub name: &'static str,
    pub says: &'static str,
    /// [`PRIVACY`] for what the camera or a microphone gives, else null.
    pub privacy: Option<&'static str>,
}

/// What a skill may use, by machine name, in the order the page shows it.
pub const USES: [Use; 14] = [
    use_of("speak", "speaks, or plays a short sound", false),
    use_of("light", "changes the light", false),
    use_of("body", "moves the lamp", false),
    use_of("camera", "uses the camera when it runs", true),
    use_of(
        "faces",
        "notices faces in front of it, with the camera",
        true,
    ),
    use_of(
        "presence",
        "notices whether someone is at the desk, with the camera",
        true,
    ),
    use_of("hearing", "hears what is said to it", true),
    use_of("room_sound", "hears how loud the room is", true),
    use_of("touch", "feels a touch on the lamp", false),
    use_of("air", "reads the air sensor", false),
    use_of("reminders", "knows when a reminder is due", false),
    use_of(
        "tasks",
        "knows how the work you handed over is going",
        false,
    ),
    use_of("memory", "reads what you asked it to remember", false),
    use_of("lamp_state", "knows what the lamp itself is doing", false),
];

/// The lamp's own functions by name: `[permissions] actions`, which no
/// skill names yet.
pub const ACTIONS: Use = use_of("actions", "uses the lamp's own controls", false);

const fn use_of(name: &'static str, says: &'static str, private: bool) -> Use {
    Use {
        name,
        says,
        privacy: if private { Some(PRIVACY) } else { None },
    }
}

/// The use a topic a skill reads gives, by name: the camera's, the
/// microphones', the sensors' and the parts', and `lamp_state` for any other.
fn topic_use(topic: &str) -> &'static str {
    match topic {
        "humans/faces/tracked" => "faces",
        "room/presence" | "room/activity" => "presence",
        "voice/heard" | "name/heard" => "hearing",
        "ears/level" | "ears/sudden" => "room_sound",
        "controls/touch" | "controls/gesture" => "touch",
        "air/state" => "air",
        "schedule/due" => "reminders",
        "tasks/status" => "tasks",
        "memory/rules" => "memory",
        _ => "lamp_state",
    }
}

/// What `permissions` give, each once, in [`USES`]' order, then
/// [`ACTIONS`].
pub fn may_use(permissions: &Permissions) -> Vec<Use> {
    let mut named = BTreeSet::new();
    for (name, given) in [
        ("speak", permissions.speak),
        ("light", permissions.light),
        ("body", permissions.body),
        ("camera", permissions.look),
    ] {
        if given {
            named.insert(name);
        }
    }
    for topic in &permissions.topics {
        named.insert(topic_use(topic));
    }
    let mut uses: Vec<Use> = USES
        .iter()
        .copied()
        .filter(|known| named.contains(known.name))
        .collect();
    if !permissions.actions.is_empty() {
        uses.push(ACTIONS);
    }
    uses
}

/// One row of the page.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Listed {
    /// The folder's name, which is the skill's.
    pub name: String,
    /// `SKILL.md`'s heading, else the name.
    pub title: String,
    /// `SKILL.md`'s description; empty when it has none.
    pub description: String,
    pub group: Group,
    /// Its `skill.toml`'s version, if it says one.
    pub version: Option<String>,
    /// False for a core skill: it comes with the lamp.
    pub can_remove: bool,
    /// It has a `[stop]` (#1837) and loads: a person can turn it off.
    pub can_turn_off: bool,
    /// With a `[stop]`: true when it is turned off, false when it is not
    /// (it may be snoozed until the morning, which is not kept where this
    /// reads); null without one, or when the off file can't be read.
    pub off: Option<bool>,
    /// Whether the running `lamp-skills` loads it.
    pub loads: bool,
    /// Why not, in the lamp's words, when it does not; else null.
    pub why: Option<String>,
    pub may_use: Vec<Use>,
}

/// `skills.list`'s reply: `{"version", "skills", "left_out"}`, the skills
/// by group (core, installed, made here) and by name in each.
pub fn list(places: &Places) -> Result<Value, String> {
    fs::read_dir(&places.folder).map_err(|_| "the lamp's skills can't be read now".to_owned())?;
    let off = read_off(&places.off);
    let mut rows: Vec<Listed> = lamp_skills::load_each(&places.folder)
        .into_iter()
        .map(|(folder, loaded)| row(&folder, loaded, off.as_ref()))
        .collect();
    rows.sort_by(|a, b| (a.group, &a.name).cmp(&(b.group, &b.name)));
    let left_out = rows.len().saturating_sub(MOST_SKILLS);
    rows.truncate(MOST_SKILLS);
    Ok(json!({"version": VERSION, "skills": rows, "left_out": left_out}))
}

/// What `skills.remove` says of a `name` that is not a skill's: the name
/// is not said back, so nothing the app sent is echoed.
pub const NOT_A_NAME: &str =
    "name must be a skill's name: 1 to 64 lowercase letters, digits and hyphens";

/// `skills.remove {"name"}` (#1722): an installed or made-here skill taken
/// off the lamp, on the person's tap in the app. `lamp-cloud` talks to the
/// network, so it never writes the skills folder (the first builder's 18:33):
/// it asks the manage helper over its local socket ([`ask_helper`]), root's
/// `lamp-skills asked`, which checks the request again itself and runs
/// `lamp-skills`' own remove ([`lamp_skills::package::remove`]): renamed
/// aside under the folder's lock, gone at once for the running program, then
/// deleted. The reply is the page as it now is, [`list`]'s, read after the
/// remove. A core skill is refused, and so is a name that is not a skill's
/// (no `/`, `..`, NUL or more than 64 bytes get past
/// [`lamp_skills::package::skill_name_ok`]) or one not on the lamp, here
/// before the helper is asked and by the helper again.
pub fn remove(places: &Places, data: &Value) -> Result<Value, String> {
    let name = data
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| lamp_skills::package::skill_name_ok(name))
        .ok_or(NOT_A_NAME)?;
    fs::read_dir(&places.folder).map_err(|_| "the lamp's skills can't be read now".to_owned())?;
    let (folder, loaded) = lamp_skills::load_each(&places.folder)
        .into_iter()
        .find(|(folder, _)| folder.file_name() == Some(std::ffi::OsStr::new(name)))
        .ok_or_else(|| format!("no skill {name} on this lamp"))?;
    if row(&folder, loaded, None).group == Group::Core {
        return Err(format!(
            "{name} is a core skill: it comes with the lamp, so it can't be removed"
        ));
    }
    match ask_helper(&places.manage, name) {
        Ok(said) => eprintln!("lamp-cloud: skills.remove from the app: {said}"),
        Err(Asked::Refused(why)) => {
            eprintln!("lamp-cloud: skills.remove {name} from the app refused: {why}");
            return Err(cut(&one_line(&why), WHY_MOST));
        }
        Err(Asked::Unanswered(error)) => {
            eprintln!(
                "lamp-cloud: skills.remove {name}: the manage helper did not answer: {error}"
            );
            return Err(NOT_CHANGED.to_owned());
        }
    }
    list(places)
}

/// What the app is told when the manage helper can't be reached or does
/// not answer: the helper's own words when another run holds the folder.
pub const NOT_CHANGED: &str = lamp_skills::package::BUSY;

/// The longest `skills.remove` waits for the manage helper's answer: a
/// remove takes milliseconds, but it waits for the folder's lock while an
/// install by hand runs. The helper waits for that lock at most
/// [`lamp_skills::package::ASKED_LOCK_WAIT`], under this, then refuses with
/// nothing done, so no remove is finished after `lamp-cloud` gave up.
pub const ASK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const _: () = assert!(lamp_skills::package::ASKED_LOCK_WAIT.as_millis() < ASK_TIMEOUT.as_millis());

/// Why the manage helper did not remove a skill.
enum Asked {
    /// It answered no, and why.
    Refused(String),
    /// It could not be asked, or its answer did not come or was not one.
    Unanswered(String),
}

/// `{"remove": NAME}` asked of the manage helper at `socket`, one line each
/// way ([`lamp_skills::package::asked`]): what it did, or why not. The
/// helper is root's and owns the folder; `lamp-cloud` writes nothing there.
fn ask_helper(socket: &Path, name: &str) -> Result<String, Asked> {
    use std::io::{BufRead, Write};
    let unanswered = |error: std::io::Error| Asked::Unanswered(error.to_string());
    let stream = std::os::unix::net::UnixStream::connect(socket).map_err(unanswered)?;
    stream
        .set_read_timeout(Some(ASK_TIMEOUT))
        .map_err(unanswered)?;
    stream
        .set_write_timeout(Some(ASK_TIMEOUT))
        .map_err(unanswered)?;
    writeln!(&stream, "{}", json!({ "remove": name })).map_err(unanswered)?;
    let mut line = String::new();
    std::io::BufReader::new((&stream).take(lamp_skills::package::ASKED_MOST))
        .read_line(&mut line)
        .map_err(unanswered)?;
    let answer: Value = serde_json::from_str(line.trim_end())
        .map_err(|_| Asked::Unanswered(format!("not an answer: {:?}", cut(&line, 200))))?;
    match (answer["done"].as_str(), answer["refused"].as_str()) {
        (Some(said), None) => Ok(said.to_owned()),
        (None, Some(why)) => Err(Asked::Refused(why.to_owned())),
        _ => Err(Asked::Unanswered(format!("not an answer: {answer}"))),
    }
}

/// A folder as the page shows it, loaded or not.
fn row(
    folder: &Path,
    loaded: Result<lamp_skills::Skill, String>,
    off: Option<&BTreeSet<String>>,
) -> Listed {
    let name = folder
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let card = read_small(&folder.join("SKILL.md")).unwrap_or_default();
    let toml = read_small(&folder.join("skill.toml"))
        .and_then(|text| text.parse::<toml::Table>().ok())
        .unwrap_or_default();
    let has_stop = toml.contains_key("stop");
    let (kind, version, permissions, description, why) = match loaded {
        Ok(skill) => (
            skill.kind,
            skill.version,
            skill.permissions,
            skill.description,
            None,
        ),
        // What a folder that does not load says of itself, read on its own,
        // as `lamp-skills list` does.
        Err(why) => (
            SkillKind::read(toml.get("kind").and_then(toml::Value::as_str)).unwrap_or_default(),
            toml.get("version")
                .and_then(toml::Value::as_str)
                .map(str::to_owned),
            toml.get("permissions")
                .cloned()
                .and_then(|table| table.try_into::<Permissions>().ok())
                .unwrap_or_default(),
            front_field(&card, "description").unwrap_or_default(),
            Some(cut(&one_line(&why), WHY_MOST)),
        ),
    };
    let loads = why.is_none();
    let group = match kind {
        SkillKind::Core => Group::Core,
        SkillKind::Installable if front_field(&card, "made").as_deref() == Some(MADE_HERE) => {
            Group::MadeHere
        }
        SkillKind::Installable => Group::Installed,
    };
    let can_turn_off = has_stop && loads;
    Listed {
        title: heading(&card).unwrap_or_else(|| name.clone()),
        description: cut(&description, DESCRIPTION_MOST),
        group,
        version: version.map(|version| cut(&version, 32)),
        can_remove: kind != SkillKind::Core,
        can_turn_off,
        off: can_turn_off
            .then(|| off.map(|off| off.contains(&name)))
            .flatten(),
        loads,
        why,
        may_use: may_use(&permissions),
        name,
    }
}

/// A file of at most [`FILE_MOST`] bytes, or nothing.
fn read_small(path: &Path) -> Option<String> {
    let mut text = String::new();
    fs::File::open(path)
        .ok()?
        .take(FILE_MOST)
        .read_to_string(&mut text)
        .ok()?;
    Some(text)
}

/// The off file's names; an empty set when there is none (nothing is off),
/// and nothing when it can't be read.
fn read_off(path: &Path) -> Option<BTreeSet<String>> {
    match fs::metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(BTreeSet::new()),
        Err(_) => None,
        Ok(_) => read_small(path).map(|text| {
            text.lines()
                .map(str::trim)
                .filter(|line| lamp_skills::package::skill_name_ok(line))
                .map(str::to_owned)
                .collect()
        }),
    }
}

/// A field of `SKILL.md`'s front matter, as `lamp-skills` reads its name
/// and description.
fn front_field(card: &str, key: &str) -> Option<String> {
    let front = card.strip_prefix("---\n")?.split_once("\n---")?.0;
    front
        .lines()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix(':'))
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

/// `SKILL.md`'s first `# ` heading after its front matter.
fn heading(card: &str) -> Option<String> {
    let body = card
        .strip_prefix("---\n")
        .and_then(|rest| rest.split_once("\n---"))
        .map_or(card, |(_, body)| body);
    body.lines()
        .find_map(|line| line.strip_prefix("# "))
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(|title| cut(title, TITLE_MOST))
}

/// `text` on one line: each run of white space one space.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `text` cut to at most `most` bytes, at a character's edge.
fn cut(text: &str, most: usize) -> String {
    if text.len() <= most {
        return text.to_owned();
    }
    let mut end = most;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// A skills folder of its own, and an off file beside it.
    pub fn places(name: &str) -> Places {
        let root = PathBuf::from(format!("/tmp/lcsk-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("skills")).unwrap();
        Places {
            folder: root.join("skills"),
            off: root.join("off"),
            manage: root.join("m.sock"),
        }
    }

    /// The manage helper on `places.manage`, a real Unix socket on a short
    /// `/tmp` path: each connection answered by `lamp_skills::package::asked`
    /// over the folder, as systemd's `Accept=yes` hands one to `lamp-skills
    /// asked`. Only the process starting differs.
    pub fn helper(places: &Places) {
        let _ = fs::remove_file(&places.manage);
        let listener = std::os::unix::net::UnixListener::bind(&places.manage).unwrap();
        let folder = places.folder.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let _ = lamp_skills::package::asked(&folder, &stream, &stream);
            }
        });
    }

    /// A skill that loads: a tool started only when asked, with `toml` for
    /// its kind, version, permissions and anything more, and `front` added
    /// to its front matter.
    pub fn skill(places: &Places, name: &str, title: &str, front: &str, toml: &str) {
        let dir = places.folder.join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("SKILL.md"),
            format!(
                "---\nname: {name}\ndescription: What {name} does.\n{front}---\n\n# {title}\n\nMore.\n"
            ),
        )
        .unwrap();
        fs::write(
            dir.join("skill.toml"),
            format!(
                "tool = \"{}\"\n{toml}\n\n[[do]]\nbody = {{ gesture = \"stretching\" }}\n",
                name.replace('-', "_")
            ),
        )
        .unwrap();
    }

    fn listed(places: &Places) -> Vec<Value> {
        list(places).unwrap()["skills"]
            .as_array()
            .unwrap()
            .to_owned()
    }

    #[test]
    fn the_reply_lists_each_skill_with_its_group_and_what_it_may_use() {
        let places = places("shape");
        skill(
            &places,
            "stretch",
            "Stretch break",
            "",
            "kind = \"core\"\nversion = \"1.0.0\"\n[permissions]\nbody = true",
        );
        let reply = list(&places).unwrap();
        assert_eq!(reply["version"], 1);
        assert_eq!(reply["left_out"], 0);
        assert_eq!(
            reply["skills"],
            json!([{
                "name": "stretch",
                "title": "Stretch break",
                "description": "What stretch does.",
                "group": "core",
                "version": "1.0.0",
                "can_remove": false,
                "can_turn_off": false,
                "off": null,
                "loads": true,
                "why": null,
                "may_use": [{"name": "body", "says": "moves the lamp", "privacy": null}],
            }])
        );
    }

    #[test]
    fn a_core_skill_can_never_be_removed_and_an_installed_one_can() {
        let places = places("remove");
        skill(
            &places,
            "dance",
            "Dance",
            "made: here\n",
            "kind = \"core\"\n[permissions]\nbody = true",
        );
        skill(
            &places,
            "tutor",
            "Desk tutor",
            "",
            "kind = \"installable\"\n[permissions]\nbody = true",
        );
        let rows = listed(&places);
        assert!(rows.iter().all(|row| row["loads"] == true), "{rows:?}");
        // A core skill that says it was made here is still core.
        assert_eq!(
            (&rows[0]["name"], &rows[0]["group"], &rows[0]["can_remove"]),
            (&json!("dance"), &json!("core"), &json!(false))
        );
        assert_eq!(
            (&rows[1]["name"], &rows[1]["group"], &rows[1]["can_remove"]),
            (&json!("tutor"), &json!("installed"), &json!(true))
        );
    }

    #[test]
    fn a_skill_made_on_this_lamp_has_its_own_group_and_can_be_removed() {
        let places = places("made");
        skill(
            &places,
            "plant-water",
            "Water the plant",
            "made: here\n",
            "[permissions]\nbody = true",
        );
        skill(
            &places,
            "a-core",
            "A core one",
            "",
            "kind = \"core\"\n[permissions]\nbody = true",
        );
        skill(
            &places,
            "z-added",
            "Added",
            "made: elsewhere\n",
            "[permissions]\nbody = true",
        );
        let rows = listed(&places);
        assert!(rows.iter().all(|row| row["loads"] == true), "{rows:?}");
        let groups: Vec<(&Value, &Value)> = rows
            .iter()
            .map(|row| (&row["name"], &row["group"]))
            .collect();
        assert_eq!(
            groups,
            [
                (&json!("a-core"), &json!("core")),
                (&json!("z-added"), &json!("installed")),
                (&json!("plant-water"), &json!("made_here")),
            ]
        );
        assert_eq!(rows[2]["can_remove"], true);
        assert_eq!(rows[2]["title"], "Water the plant");
    }

    #[test]
    fn what_a_skill_may_use_is_said_in_plain_words_with_the_privacy_switch() {
        let permissions = Permissions {
            topics: vec![
                "humans/faces/tracked".into(),
                "voice/state".into(),
                "voice/heard".into(),
                "air/state".into(),
            ],
            speak: true,
            light: true,
            look: true,
            ..Permissions::default()
        };
        let said: Vec<(&str, &str, Option<&str>)> = may_use(&permissions)
            .iter()
            .map(|used| (used.name, used.says, used.privacy))
            .collect();
        assert_eq!(
            said,
            [
                ("speak", "speaks, or plays a short sound", None),
                ("light", "changes the light", None),
                (
                    "camera",
                    "uses the camera when it runs",
                    Some("the privacy switch stops it")
                ),
                (
                    "faces",
                    "notices faces in front of it, with the camera",
                    Some("the privacy switch stops it")
                ),
                (
                    "hearing",
                    "hears what is said to it",
                    Some("the privacy switch stops it")
                ),
                ("air", "reads the air sensor", None),
                ("lamp_state", "knows what the lamp itself is doing", None),
            ]
        );
        assert!(may_use(&Permissions::default()).is_empty());
        // Every name is the page's key, once.
        let names: BTreeSet<&str> = USES.iter().map(|used| used.name).collect();
        assert_eq!(names.len(), USES.len());
    }

    #[test]
    fn a_skill_that_does_not_load_is_listed_with_why_and_what_it_says_of_itself() {
        let places = places("broken");
        skill(
            &places,
            "tutor",
            "Desk tutor",
            "",
            "version = \"2.0.0\"\n[permissions]\nlook = true",
        );
        // Its step needs the body, which it does not give.
        fs::write(
            places.folder.join("tutor/skill.toml"),
            "version = \"2.0.0\"\ntool = \"tutor\"\n[permissions]\nlook = true\n[[do]]\nbody = { gesture = \"stretching\" }\n",
        )
        .unwrap();
        let rows = listed(&places);
        assert_eq!(rows[0]["loads"], false);
        assert!(
            rows[0]["why"].as_str().unwrap().contains("body"),
            "{}",
            rows[0]
        );
        assert_eq!(rows[0]["version"], "2.0.0");
        assert_eq!(rows[0]["group"], "installed");
        assert_eq!(rows[0]["description"], "What tutor does.");
        assert_eq!(rows[0]["may_use"][0]["name"], "camera");
    }

    /// #1837's `[stop]` is not on `lamp-voice` yet, so a skill with one does
    /// not load here and is offered no switch. Once one loads, the off file
    /// says whether it is off.
    #[test]
    fn only_a_skill_with_a_stop_that_loads_can_be_turned_off() {
        let places = places("stop");
        let with_stop = "[permissions]\nbody = true\n[stop]\ntool = \"stop_sip\"";
        skill(&places, "sip", "Sip", "", with_stop);
        skill(&places, "plain", "Plain", "", "[permissions]\nbody = true");
        let rows = listed(&places);
        let sip = rows.iter().find(|row| row["name"] == "sip").unwrap();
        assert_eq!(
            (&sip["loads"], &sip["can_turn_off"], &sip["off"]),
            (&json!(false), &json!(false), &Value::Null)
        );
        let plain = rows.iter().find(|row| row["name"] == "plain").unwrap();
        assert_eq!(
            (&plain["can_turn_off"], &plain["off"]),
            (&json!(false), &Value::Null)
        );
        // As it will be once a `[stop]` loads: a loaded skill in the folder
        // whose `skill.toml` has one.
        let loaded = lamp_skills::load(&places.folder.join("plain")).unwrap();
        let folder = places.folder.join("sip");
        fs::write(&places.off, "sip\nnot a name\n").unwrap();
        let off = read_off(&places.off);
        assert_eq!(off, Some(["sip".to_owned()].into()));
        let stopped = row(&folder, Ok(loaded.clone()), off.as_ref());
        assert_eq!((stopped.can_turn_off, stopped.off), (true, Some(true)));
        let none = read_off(&places.off.with_file_name("none"));
        assert_eq!(none, Some(BTreeSet::new()));
        let on = row(&folder, Ok(loaded.clone()), none.as_ref());
        assert_eq!((on.can_turn_off, on.off), (true, Some(false)));
        let unknown = row(&folder, Ok(loaded), None);
        assert_eq!((unknown.can_turn_off, unknown.off), (true, None));
    }

    #[test]
    fn the_reply_is_bounded() {
        let places = places("bounded");
        for at in 0..(MOST_SKILLS + 2) {
            skill(
                &places,
                &format!("s{at:03}"),
                &"T".repeat(200),
                "",
                "[permissions]\nbody = true",
            );
        }
        let reply = list(&places).unwrap();
        assert_eq!(reply["skills"].as_array().unwrap().len(), MOST_SKILLS);
        assert_eq!(reply["left_out"], 2);
        assert_eq!(
            reply["skills"][0]["title"].as_str().unwrap().len(),
            TITLE_MOST
        );
        assert_eq!(cut("éé", 3), "é");
    }

    #[test]
    fn a_skills_folder_that_cannot_be_read_is_said_plainly() {
        let mut places = places("none");
        places.folder = places.folder.join("missing");
        assert_eq!(
            list(&places),
            Err("the lamp's skills can't be read now".to_owned())
        );
    }

    /// A core, a made-here and an installed skill, side by side, with the
    /// manage helper.
    fn three(name: &str) -> Places {
        let places = places(name);
        helper(&places);
        skill(
            &places,
            "dance",
            "Dance",
            "",
            "kind = \"core\"\n[permissions]\nbody = true",
        );
        skill(
            &places,
            "plant-water",
            "Water the plant",
            "made: here\n",
            "[permissions]\nbody = true",
        );
        skill(
            &places,
            "tutor",
            "Desk tutor",
            "",
            "kind = \"installable\"\n[permissions]\nbody = true",
        );
        places
    }

    fn names(reply: &Value) -> Vec<&str> {
        reply["skills"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["name"].as_str().unwrap())
            .collect()
    }

    /// What is in the folder, hidden folders among them: a remove leaves
    /// nothing beside the skills.
    fn folder_holds(places: &Places) -> Vec<String> {
        let mut found: Vec<String> = fs::read_dir(&places.folder)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        found.sort();
        found
    }

    #[test]
    fn a_core_skill_is_never_removed() {
        let places = three("rm-core");
        assert_eq!(
            remove(&places, &json!({"name": "dance"})),
            Err("dance is a core skill: it comes with the lamp, so it can't be removed".to_owned())
        );
        assert!(places.folder.join("dance/skill.toml").exists());
        assert_eq!(folder_holds(&places), ["dance", "plant-water", "tutor"]);
    }

    #[test]
    fn a_skill_made_here_is_removed_and_the_page_is_given_as_it_now_is() {
        let places = three("rm-made");
        let reply = remove(&places, &json!({"name": "plant-water"})).unwrap();
        assert_eq!(reply, list(&places).unwrap());
        assert_eq!(names(&reply), ["dance", "tutor"]);
        assert_eq!(reply["version"], VERSION);
        assert!(!places.folder.join("plant-water").exists());
        assert_eq!(folder_holds(&places), ["dance", "tutor"]);
        // Asked again, it is not there.
        assert_eq!(
            remove(&places, &json!({"name": "plant-water"})),
            Err("no skill plant-water on this lamp".to_owned())
        );
    }

    #[test]
    fn an_installed_skill_is_removed() {
        let places = three("rm-installed");
        let reply = remove(&places, &json!({"name": "tutor"})).unwrap();
        assert_eq!(names(&reply), ["dance", "plant-water"]);
        assert_eq!(folder_holds(&places), ["dance", "plant-water"]);
    }

    /// One that does not load is still the person's to take off.
    #[test]
    fn an_installed_skill_that_does_not_load_is_removed_too() {
        let places = three("rm-broken");
        fs::write(places.folder.join("tutor/skill.toml"), "not toml [").unwrap();
        assert_eq!(list(&places).unwrap()["skills"][1]["loads"], false);
        let reply = remove(&places, &json!({"name": "tutor"})).unwrap();
        assert_eq!(names(&reply), ["dance", "plant-water"]);
    }

    #[test]
    fn an_unknown_name_and_every_path_trick_are_refused_and_nothing_is_touched() {
        let places = three("rm-tricks");
        // A folder beside the skills folder, which `../victim` would name.
        let victim = places.folder.with_file_name("victim");
        fs::create_dir_all(&victim).unwrap();
        assert_eq!(
            remove(&places, &json!({"name": "juggle"})),
            Err("no skill juggle on this lamp".to_owned())
        );
        let long = "a".repeat(65);
        for data in [
            json!({"name": ".."}),
            json!({"name": "."}),
            json!({"name": "../victim"}),
            json!({"name": "tutor/../dance"}),
            json!({"name": "/var/lib/lampOS/skills/tutor"}),
            json!({"name": "tutor\0"}),
            json!({"name": "\0"}),
            json!({"name": ".removed-tutor-1"}),
            json!({"name": "Tutor"}),
            json!({"name": "desk_tutor"}),
            json!({"name": "tutor "}),
            json!({"name": ""}),
            json!({"name": long}),
            json!({"name": 7}),
            json!({"name": null}),
            json!({"name": ["tutor"]}),
            json!({}),
            json!("tutor"),
            Value::Null,
        ] {
            assert_eq!(remove(&places, &data), Err(NOT_A_NAME.to_owned()), "{data}");
        }
        assert!(victim.exists());
        assert_eq!(folder_holds(&places), ["dance", "plant-water", "tutor"]);
        // 64 bytes is a name, which this lamp does not have.
        let most = "a".repeat(64);
        assert_eq!(
            remove(&places, &json!({"name": most})),
            Err(format!("no skill {most} on this lamp"))
        );
    }

    /// A link named as a skill is `lamp-skills`' to refuse: what it points
    /// at is never removed.
    #[test]
    fn a_link_named_as_a_skill_is_left_as_it_is() {
        let places = three("rm-link");
        let elsewhere = places.folder.with_file_name("elsewhere");
        fs::create_dir_all(&elsewhere).unwrap();
        fs::write(elsewhere.join("keep"), "kept").unwrap();
        std::os::unix::fs::symlink(&elsewhere, places.folder.join("linked")).unwrap();
        let why = remove(&places, &json!({"name": "linked"})).unwrap_err();
        assert!(why.contains("is not a skill's folder"), "{why}");
        assert!(elsewhere.join("keep").exists());
        assert!(places.folder.join("linked").exists());
    }

    /// A folder `lamp-cloud` can't change (the product's, root's, under
    /// its unit's `ProtectSystem=strict`) refuses in `lamp-skills`' words,
    /// and the skill stays whole.
    #[test]
    fn a_folder_that_cannot_be_changed_leaves_the_skill_whole() {
        use std::os::unix::fs::PermissionsExt;
        let places = three("rm-readonly");
        fs::set_permissions(&places.folder, fs::Permissions::from_mode(0o555)).unwrap();
        let refused = remove(&places, &json!({"name": "tutor"}));
        fs::set_permissions(&places.folder, fs::Permissions::from_mode(0o755)).unwrap();
        let why = refused.unwrap_err();
        assert!(why.starts_with("not removed: "), "{why}");
        assert!(why.len() <= WHY_MOST);
        assert!(places.folder.join("tutor/skill.toml").exists());
        assert_eq!(folder_holds(&places), ["dance", "plant-water", "tutor"]);
    }

    /// Without the helper, nothing is removed and the app is told so in
    /// plain words; `lamp-cloud` never falls back to writing the folder.
    #[test]
    fn without_the_manage_helper_nothing_is_removed() {
        let places = three("rm-nohelper");
        fs::remove_file(&places.manage).unwrap();
        assert_eq!(
            remove(&places, &json!({"name": "tutor"})),
            Err(NOT_CHANGED.to_owned())
        );
        // One that answers nothing a helper would.
        let listener = std::os::unix::net::UnixListener::bind(&places.manage).unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                use std::io::Write;
                let _ = writeln!(&stream, "{{\"removed\": true}}");
            }
        });
        assert_eq!(
            remove(&places, &json!({"name": "tutor"})),
            Err(NOT_CHANGED.to_owned())
        );
        assert_eq!(folder_holds(&places), ["dance", "plant-water", "tutor"]);
    }

    /// While another run holds the skills folder past the helper's wait (an
    /// install by hand), the helper refuses with nothing done before
    /// `lamp-cloud` stops waiting, and never removes the skill afterwards
    /// (the reviewer's P3 1 on #1890).
    #[test]
    fn a_remove_held_past_the_helpers_wait_is_refused_and_never_done_later() {
        let places = three("rm-held");
        let held = fs::File::open(&places.folder).unwrap();
        held.lock().unwrap();
        let asked = std::time::Instant::now();
        assert_eq!(
            remove(&places, &json!({"name": "tutor"})),
            Err(NOT_CHANGED.to_owned())
        );
        let waited = asked.elapsed();
        assert!(
            waited >= lamp_skills::package::ASKED_LOCK_WAIT && waited < ASK_TIMEOUT,
            "{waited:?}"
        );
        held.unlock().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert_eq!(folder_holds(&places), ["dance", "plant-water", "tutor"]);
        // Given back, the next ask removes it.
        let page = remove(&places, &json!({"name": "tutor"})).unwrap();
        assert_eq!(folder_holds(&places), ["dance", "plant-water"]);
        assert_eq!(page, list(&places).unwrap());
    }

    #[test]
    fn the_switch_is_on_only_with_the_cloud() {
        assert!(gates(Some("on"), Some("on")));
        for (cloud, page) in [
            (None, Some("on")),
            (Some("on"), None),
            (Some("on"), Some("1")),
            (Some("off"), Some("on")),
            (Some("on"), Some("")),
        ] {
            assert!(!gates(cloud, page), "{cloud:?} {page:?}");
        }
    }
}
