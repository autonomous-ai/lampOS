//! A skill made by voice (#1685, "An owner makes their own skill"), off
//! unless `LAMP_MAKE_SKILL=on`: "Lamp, make me a skill that dims the light
//! at 10 pm". The model calls [`MAKE`] with the person's request. The voice
//! prepares a hand-over with the tasks part as for any work
//! (`crate::agent_tasks`): the [`brief`], fixed words that carry the skill
//! format and what a made skill may use, then the request and nothing else
//! of the conversation. It reads the request back and hands it over only on
//! the person's yes.
//!
//! When the agent's answer comes back (the hand-over's own channel,
//! Harness's answer), its two files are taken from it ([`files`]), held to
//! what a made skill may use ([`capped`]), marked `made: here`, and checked
//! and packed by `lamp-skills pack` (#1698), the checker an install uses.
//! The package is kept in the voice's own folder ([`DIRECTORY`], 0700, at
//! most [`KEPT_MOST`]), with its SHA-256 and what it asks to use. The lamp
//! then says what it would do, what it asks to use, and [`NEEDS_A_YES`]:
//! nothing is installed. Putting a made skill on the lamp needs a decision
//! still to be taken (an unsigned package made by an agent; the
//! "only signed packages" ruling of 19 Oct 18:33), and the SHA-256 kept is
//! what such a yes would be bound to, so that what was said is what goes on.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime},
};

/// Where the coordinator reads the switch. `on` only.
pub const ENVIRONMENT: &str = "LAMP_MAKE_SKILL";
/// The function the model calls for a skill to be made.
pub const MAKE: &str = "make_skill";
/// The most of the person's request carried in the brief: the brief and it
/// fit the tasks part's 1,000 bytes.
pub const MOST_REQUEST_BYTES: usize = 180;
/// At most this many made skills are kept; the oldest is let go.
pub const KEPT_MOST: usize = 5;
/// Where the product keeps them: the voice's own, 0700.
pub const DIRECTORY: &str = "/var/lib/lampOS/voice/made-skills";
/// The program that checks and packs them: `lamp-skills`, as the product
/// installs it.
pub const PROGRAM: &str = "/opt/lampOS/skills/current/lamp-skills";
/// The topics a made skill may read: whether the voice is talking, and
/// whether someone is at the lamp. Nothing it reads is about a person's
/// words, face or picture.
pub const TOPICS: [&str; 2] = ["voice/state", "room/presence"];
/// What the lamp says of a skill it made, word for word.
pub const NEEDS_A_YES: &str = "I've made it; putting it on needs a yes from the team first.";
/// The longest `lamp-skills pack` may take.
const PACK_WITHIN: Duration = Duration::from_secs(20);
/// The most of either file taken from an answer.
const MOST_FILE_BYTES: usize = 8 * 1024;
/// The most of a description said.
const MOST_DESCRIPTION_CHARS: usize = 200;
/// The file that keeps what was made, in [`DIRECTORY`].
const INDEX: &str = "made.json";

/// The brief's first words: a task that begins with them is a skill to make.
pub const BRIEF_HEAD: &str = "Write one lamp skill (lampOS lamp-skills)";
/// What comes before the person's request in the brief.
const REQUEST_MARK: &str = "Request: ";

/// The brief: the format of a skill a lamp can make, what it may use, and
/// the person's request, within the tasks part's 1,000 bytes. Nothing of
/// the conversation but the request: the model passes it, in the person's
/// words.
pub fn brief(request: &str) -> Result<String, &'static str> {
    let request = request.split_whitespace().collect::<Vec<_>>().join(" ");
    if request.is_empty() {
        return Err("Say what the skill should do.");
    }
    if request.len() > MOST_REQUEST_BYTES {
        return Err("That is too long to ask for as one skill: say it more shortly.");
    }
    Ok(format!(
        "{BRIEF_HEAD} for the request at the end. Reply with exactly two fenced blocks, \
         ```skill.toml and ```SKILL.md, and nothing else; write no files. skill.toml: \
         kind = \"installable\", version = \"1.0.0\"; [permissions] with only light = true \
         and/or body = true, topics only voice/state and room/presence; one [trigger] on \
         voice/state (field \"state\", equals \"resting\") or room/presence (field \"someone\", \
         equals true or false), with for, cooldown, rearm = \"ends\"; a time of day as [quiet] \
         from its end to its start (22:00 to 22:30: from \"22:30\", to \"22:00\"); \
         [ask] policy = \"nudge\"; [[do]] steps: light = {{ colour, level 1-5, for }} or \
         {{ dim = 1 }}, body = {{ gesture = \"nod\" or \"curious\" }}. SKILL.md: front matter \
         name (lowercase, hyphens) and description (one sentence: what it does, when). \
         {REQUEST_MARK}{request}"
    ))
}

/// The function the model is given with the switch: `who` makes it, as the
/// hand-over's work goes to them.
pub fn tool(who: &str) -> lamp_gemini::Tool {
    lamp_gemini::Tool {
        name: MAKE.into(),
        description: format!(
            "Ask {who} to make a new skill for the lamp: something the lamp then does by itself \
             with its light or its head, at a time of day, when someone comes or goes, or when \
             it is resting (\"make me a skill that dims the light at 10 pm\"). Call it only when \
             the person asks for a skill to be made. Say it back and ask; on their yes, call \
             confirm_agent_task. The skill is made, checked and kept, and is not put on the lamp."
        ),
        parameters: Some(json!({
            "type": "object",
            "properties": {
                "request": {
                    "type": "string",
                    "description": "What the skill should do and when, in the person's own words, nothing else."
                }
            },
            "required": ["request"]
        })),
        silent: false,
    }
}

/// What the model is told after a skill to make was prepared.
pub fn prepared_next(who: &str) -> String {
    format!(
        "Say back in one short sentence the skill they asked for, saying {who} will make it, \
         and ask whether to have it made. Only if the person says yes, call confirm_agent_task."
    )
}

/// Whether a task's words are a [`brief`]: its answer is a skill to keep.
pub fn is_brief(task: &str) -> bool {
    task.starts_with(BRIEF_HEAD)
}

/// The person's request in a [`brief`], for saying it back.
pub fn request_of(task: &str) -> &str {
    task.rsplit_once(REQUEST_MARK)
        .map_or("", |(_, request)| request)
}

/// Why a made skill is not kept, in plain words the lamp can say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused(pub String);

impl Refused {
    fn new(why: impl Into<String>) -> Self {
        Self(why.into())
    }
}

/// The two files of a made skill, as the agent wrote them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Files {
    pub toml: String,
    pub card: String,
}

/// The two files of the agent's answer: exactly one fenced block named
/// `skill.toml` and one named `SKILL.md` (its info string's last word),
/// and no other fenced block; each at most [`MOST_FILE_BYTES`].
pub fn files(answer: &str) -> Result<Files, Refused> {
    let mut toml = None;
    let mut card = None;
    let mut lines = answer.lines();
    while let Some(line) = lines.next() {
        let Some(info) = line.trim_start().strip_prefix("```") else {
            continue;
        };
        let name = info.split_whitespace().last().unwrap_or_default();
        let mut body = String::new();
        let mut closed = false;
        for line in lines.by_ref() {
            if line.trim_start().starts_with("```") {
                closed = true;
                break;
            }
            body.push_str(line);
            body.push('\n');
        }
        if !closed {
            return Err(Refused::new("its answer was cut off"));
        }
        if body.len() > MOST_FILE_BYTES {
            return Err(Refused::new("one of its files is too long"));
        }
        let slot = match name {
            "skill.toml" => &mut toml,
            "SKILL.md" => &mut card,
            _ => return Err(Refused::new("it wrote a file a made skill can't have")),
        };
        if slot.replace(body).is_some() {
            return Err(Refused::new("it wrote one of its files twice"));
        }
    }
    match (toml, card) {
        (Some(toml), Some(card)) => Ok(Files { toml, card }),
        _ => Err(Refused::new("it didn't write both of a skill's files")),
    }
}

/// What a made skill asks to use: within [`capped`]'s bounds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Uses {
    pub light: bool,
    pub body: bool,
    pub topics: Vec<String>,
}

impl Uses {
    /// In plain words: "its light and its head", "whether someone is at the
    /// desk".
    pub fn spoken(&self) -> String {
        let mut parts = Vec::new();
        if self.light {
            parts.push("its light".to_owned());
        }
        if self.body {
            parts.push("its head and arm".to_owned());
        }
        for topic in &self.topics {
            parts.push(
                match topic.as_str() {
                    "voice/state" => "whether it is talking",
                    "room/presence" => "whether someone is at the desk",
                    other => other,
                }
                .to_owned(),
            );
        }
        match parts.as_slice() {
            [] => "nothing".to_owned(),
            [one] => one.clone(),
            [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
        }
    }

    fn json(&self) -> Value {
        json!({"light": self.light, "body": self.body, "topics": self.topics})
    }
}

/// A made skill's `skill.toml` held to what one may use (the coordinator's
/// cap, 19 Oct): installable, with a version; `[permissions]` with only
/// `light`, `body` and `topics` among [`TOPICS`]; no function for the
/// model (`tool`). Speaking, messages, the look and every other topic are
/// refused; widening this is a decision still to be taken. What the skill
/// does within it is checked by `lamp-skills pack` after.
pub fn capped(text: &str) -> Result<(String, Uses), Refused> {
    let table: toml::Table =
        toml::from_str(text).map_err(|_| Refused::new("its skill.toml can't be read"))?;
    if table.contains_key("tool") {
        return Err(Refused::new(
            "it gives the model a function, which a made skill can't",
        ));
    }
    match table.get("kind").map(|kind| kind.as_str()) {
        None | Some(Some("installable")) => {}
        _ => return Err(Refused::new("a made skill can only be installable")),
    }
    let Some(version) = table.get("version").and_then(|version| version.as_str()) else {
        return Err(Refused::new("it has no version"));
    };
    let mut uses = Uses::default();
    if let Some(permissions) = table.get("permissions") {
        let Some(permissions) = permissions.as_table() else {
            return Err(Refused::new("its permissions can't be read"));
        };
        for (key, value) in permissions {
            match (key.as_str(), value) {
                ("light", toml::Value::Boolean(on)) => uses.light = *on,
                ("body", toml::Value::Boolean(on)) => uses.body = *on,
                ("topics", toml::Value::Array(topics)) => {
                    for topic in topics {
                        match topic.as_str() {
                            Some(topic) if TOPICS.contains(&topic) => {
                                if !uses.topics.iter().any(|kept| kept == topic) {
                                    uses.topics.push(topic.to_owned());
                                }
                            }
                            _ => {
                                return Err(Refused::new(
                                    "it asks to read more than a made skill may",
                                ));
                            }
                        }
                    }
                }
                ("actions", toml::Value::Array(actions)) if actions.is_empty() => {}
                ("speak" | "look" | "messages" | "known_faces" | "actions", _) => {
                    return Err(Refused::new(
                        "it asks to use more than the light and the head, which a made skill can't",
                    ));
                }
                _ => return Err(Refused::new("its permissions can't be read")),
            }
        }
    }
    Ok((version.to_owned(), uses))
}

/// A `SKILL.md`'s name and description, and the card marked `made: here`
/// (`lamp-cloud`'s "Made on this lamp" group).
fn card(text: &str) -> Result<(String, String, String), Refused> {
    let unread = || Refused::new("its SKILL.md has no name and description");
    let rest = text.strip_prefix("---\n").ok_or_else(unread)?;
    let (front, body) = rest.split_once("\n---").ok_or_else(unread)?;
    let field = |key: &str| {
        front.lines().find_map(|line| {
            line.strip_prefix(key)
                .and_then(|rest| rest.strip_prefix(':'))
                .map(|value| value.trim().trim_matches('"').to_owned())
        })
    };
    let name = field("name").ok_or_else(unread)?;
    let description = field("description").ok_or_else(unread)?;
    if !(1..=64).contains(&name.len())
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(Refused::new("its name isn't a skill's name"));
    }
    if description.is_empty() {
        return Err(unread());
    }
    let front: Vec<&str> = front
        .lines()
        .filter(|line| !line.starts_with("made:"))
        .collect();
    let marked = format!("---\n{}\nmade: here\n---{body}", front.join("\n"));
    Ok((name, description, marked))
}

/// Where made skills are kept, and the program that checks them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keeping {
    pub directory: PathBuf,
    pub program: PathBuf,
}

impl Keeping {
    /// The product's, with the switch on.
    pub fn from_environment() -> Option<Self> {
        (std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")).then(|| Self {
            directory: PathBuf::from(DIRECTORY),
            program: PathBuf::from(PROGRAM),
        })
    }
}

/// A made skill, checked and kept.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Made {
    pub name: String,
    pub version: String,
    /// The package's SHA-256, lowercase hex: what a yes to put it on would
    /// be bound to.
    pub sha256: String,
    pub description: String,
    pub uses: Uses,
    pub package: PathBuf,
}

/// Take a made skill from the agent's `answer` (`cut`: the answer was cut
/// before it came here), hold it to the cap, check and pack it with
/// `lamp-skills pack`, and keep the package, at most [`KEPT_MOST`]. The
/// answer of the hand-over `from` (its news key) is kept once: asked again,
/// the kept one is given back.
pub fn keep(answer: &str, cut: bool, from: &str, keeping: &Keeping) -> Result<Made, Refused> {
    let unkept = |why: io::Error| {
        eprintln!("lamp-voice: a made skill was not kept: {why}");
        Refused::new("the lamp couldn't keep it")
    };
    ensure_private(&keeping.directory).map_err(unkept)?;
    if let Some(made) = kept(&keeping.directory, from) {
        return Ok(made);
    }
    if cut {
        return Err(Refused::new("its answer was cut off"));
    }
    let files = files(answer)?;
    let (version, uses) = capped(&files.toml)?;
    let (name, description, marked) = card(&files.card)?;
    let work = keeping
        .directory
        .join(format!(".making-{}", std::process::id()));
    let _ = fs::remove_dir_all(&work);
    let folder = work.join("skill").join(&name);
    let out = work.join("out");
    let made = (|| {
        fs::create_dir_all(&folder)?;
        fs::create_dir_all(&out)?;
        fs::write(folder.join("skill.toml"), &files.toml)?;
        fs::write(folder.join("SKILL.md"), &marked)?;
        Ok::<_, io::Error>(())
    })()
    .map_err(unkept);
    let packed = made.and_then(|()| pack(&keeping.program, &folder, &out));
    let result = packed.and_then(|()| {
        let package = format!("{name}-{version}.tar");
        let bytes = fs::read(out.join(&package)).map_err(unkept)?;
        let sha256 = hex(&Sha256::digest(&bytes));
        let kept_as = keeping
            .directory
            .join(format!("{name}-{version}-{}.tar", &sha256[..12]));
        fs::write(&kept_as, &bytes).map_err(unkept)?;
        let made = Made {
            name,
            version,
            sha256,
            description: cut_chars(&description, MOST_DESCRIPTION_CHARS),
            uses,
            package: kept_as,
        };
        remember(&keeping.directory, from, &made).map_err(unkept)?;
        Ok(made)
    });
    let _ = fs::remove_dir_all(&work);
    result
}

/// `lamp-skills pack --out OUT FOLDER`: the checks a load makes, its
/// permissions among them, and the package read back as an install would.
fn pack(program: &Path, folder: &Path, out: &Path) -> Result<(), Refused> {
    let mut child = Command::new(program)
        .arg("pack")
        .arg("--out")
        .arg(out)
        .arg(folder)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|why| {
            eprintln!("lamp-voice: a made skill was not checked: {why}");
            Refused::new("the lamp couldn't check it")
        })?;
    let began = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if began.elapsed() < PACK_WITHIN => {
                std::thread::sleep(Duration::from_millis(50));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Refused::new("the lamp couldn't check it in time"));
            }
        }
    };
    if status.success() {
        return Ok(());
    }
    // Why, for the journal: lamp-skills' own words, never the skill's.
    let mut why = String::new();
    if let Some(mut stderr) = child.stderr.take() {
        let _ = stderr.by_ref().take(2048).read_to_string(&mut why);
    }
    eprintln!(
        "lamp-voice: a made skill did not pass lamp-skills' check: {}",
        why.lines().next().unwrap_or("no reason given")
    );
    Err(Refused::new("it doesn't pass the lamp's check"))
}

/// The folder, made 0700 if it is not there; one already there is made
/// 0700 too.
fn ensure_private(directory: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir_all(directory)?;
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
}

/// What is kept, oldest first.
fn index(directory: &Path) -> Vec<Value> {
    fs::read(directory.join(INDEX))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<Value>>(&bytes).ok())
        .unwrap_or_default()
}

/// The made skill kept from the hand-over `from`, if it is still kept.
fn kept(directory: &Path, from: &str) -> Option<Made> {
    index(directory)
        .into_iter()
        .find(|entry| entry["from"] == from)
        .and_then(|entry| {
            let package = directory.join(entry["package"].as_str()?);
            package.is_file().then(|| Made {
                name: entry["name"].as_str().unwrap_or_default().to_owned(),
                version: entry["version"].as_str().unwrap_or_default().to_owned(),
                sha256: entry["sha256"].as_str().unwrap_or_default().to_owned(),
                description: entry["description"].as_str().unwrap_or_default().to_owned(),
                uses: Uses {
                    light: entry["uses"]["light"] == true,
                    body: entry["uses"]["body"] == true,
                    topics: entry["uses"]["topics"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|topic| topic.as_str().map(str::to_owned))
                        .collect(),
                },
                package,
            })
        })
}

/// Keep `made` in the index, from the hand-over `from`, and let the oldest
/// go past [`KEPT_MOST`], their packages with them. Written whole beside
/// it and moved over it.
fn remember(directory: &Path, from: &str, made: &Made) -> io::Result<()> {
    let mut entries = index(directory);
    entries.push(json!({
        "from": from,
        "name": made.name,
        "version": made.version,
        "sha256": made.sha256,
        "description": made.description,
        "uses": made.uses.json(),
        "package": made.package.file_name().and_then(|name| name.to_str()).unwrap_or_default(),
        "at_unix_ms": SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |since| since.as_millis() as u64),
    }));
    while entries.len() > KEPT_MOST {
        let oldest = entries.remove(0);
        if let Some(package) = oldest["package"]
            .as_str()
            .filter(|name| !name.is_empty() && !name.contains('/') && !name.starts_with('.'))
        {
            let _ = fs::remove_file(directory.join(package));
        }
    }
    let new = directory.join(format!(".{INDEX}.new"));
    let mut file = fs::File::create(&new)?;
    file.write_all(&serde_json::to_vec(&entries).map_err(io::Error::other)?)?;
    file.sync_all()?;
    fs::rename(&new, directory.join(INDEX))
}

/// What the model is told of a made skill, or of one not kept: the news
/// of the hand-over, in place of its answer, which is files and never
/// read out.
pub fn note(who: &str, when: &str, computer: &str, made: &Result<Made, Refused>) -> String {
    let opening = "(Nobody has spoken. News from work you handed over for the person.";
    match made {
        Ok(made) => format!(
            "{opening} {who} {when} made the skill they asked for, \"{}\": {} It would use {}. \
             Tell them in one or two short sentences what it would do and what it would use, then say exactly: \
             \"{NEEDS_A_YES}\" It is not on the lamp. No question, no file names.)",
            made.name,
            made.description,
            made.uses.spoken()
        ),
        Err(Refused(why)) => format!(
            "{opening} {who} {when} made a skill for them, but the lamp can't keep it: {why}. \
             Tell them so in one short sentence, and that the rest is on {computer}. No question.)"
        ),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn cut_chars(text: &str, most: usize) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match line.char_indices().nth(most) {
        Some((at, _)) => format!("{}…", &line[..at]),
        None => line,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOML: &str = r#"kind = "installable"
version = "1.0.0"
needs = ["light"]

[permissions]
topics = ["voice/state"]
light = true

[trigger]
topic = "voice/state"
field = "state"
equals = "resting"
cooldown = "12h"

[quiet]
from = "22:30"
to = "22:00"

[ask]
policy = "nudge"

[[do]]
light = { dim = 1 }
"#;

    const CARD: &str = "---\nname: evening-dim\ndescription: Dims the light a shade at 10 pm.\n---\n\n# Evening dim\n\nAt ten it dims.\n";

    fn answer(toml: &str, card: &str) -> String {
        format!("Here it is.\n\n```skill.toml\n{toml}```\n\n```markdown SKILL.md\n{card}```\n")
    }

    /// The brief and the longest request fit the tasks part's 1,000 bytes;
    /// it carries the request and nothing else of the conversation, and a
    /// longer request is refused, never cut.
    #[test]
    fn the_brief_fits_the_hand_over_and_carries_only_the_request() {
        let request = "x".repeat(MOST_REQUEST_BYTES);
        let longest = brief(&request).unwrap();
        assert!(longest.len() <= 1_000, "{}", longest.len());
        assert!(is_brief(&longest));
        assert_eq!(request_of(&longest), request);
        let asked = brief("make me a skill  that dims\nthe light at 10 pm").unwrap();
        assert!(asked.ends_with("Request: make me a skill that dims the light at 10 pm"));
        assert!(brief(&"x".repeat(MOST_REQUEST_BYTES + 1)).is_err());
        assert!(brief("  ").is_err());
        assert!(!is_brief("Write a poem about lamps"));
    }

    /// The answer's two files, each its own fenced block; another file, one
    /// twice, one missing, one left open or one too long refuses it.
    #[test]
    fn an_answer_gives_exactly_its_two_files() {
        let files = files(&answer(TOML, CARD)).unwrap();
        assert_eq!((files.toml.as_str(), files.card.as_str()), (TOML, CARD));
        let extra = format!("{}```moves/x.json\n{{}}\n```\n", answer(TOML, CARD));
        assert!(files_err(&extra).contains("can't have"));
        let twice = format!("{}```skill.toml\n{TOML}```\n", answer(TOML, CARD));
        assert!(files_err(&twice).contains("twice"));
        assert!(files_err(&format!("```skill.toml\n{TOML}```\n")).contains("both"));
        assert!(files_err(&format!("```skill.toml\n{TOML}")).contains("cut off"));
        let long = format!(
            "```skill.toml\n{}\n```\n```SKILL.md\n{CARD}```\n",
            "#".repeat(9_000)
        );
        assert!(files_err(&long).contains("too long"));
    }

    fn files_err(answer: &str) -> String {
        files(answer).unwrap_err().0
    }

    /// The coordinator's cap (19 Oct): the light and the body, and only
    /// voice/state and room/presence; speaking, messages, the look, any
    /// other topic, a function for the model, a core kind and a missing
    /// version are each refused.
    #[test]
    fn a_made_skill_may_use_only_the_light_and_the_body() {
        let (version, uses) = capped(TOML).unwrap();
        assert_eq!(version, "1.0.0");
        assert_eq!(
            uses,
            Uses {
                light: true,
                body: false,
                topics: vec!["voice/state".into()],
            }
        );
        assert_eq!(uses.spoken(), "its light and whether it is talking");
        for (wider, why) in [
            ("speak = true", "more than the light"),
            ("look = true", "more than the light"),
            ("messages = true", "more than the light"),
            ("known_faces = true", "more than the light"),
            ("actions = [\"volume_up\"]", "more than the light"),
            ("frobs = true", "can't be read"),
        ] {
            let text = TOML.replace("light = true\n", &format!("light = true\n{wider}\n"));
            assert!(capped(&text).unwrap_err().0.contains(why), "{wider}");
        }
        let faces = TOML.replace(
            "[\"voice/state\"]",
            "[\"voice/state\", \"humans/faces/tracked\"]",
        );
        assert!(capped(&faces).unwrap_err().0.contains("read more"));
        let heard = TOML.replace("[\"voice/state\"]", "[\"voice/heard\"]");
        assert!(capped(&heard).unwrap_err().0.contains("read more"));
        let tool = format!("tool = \"dim_now\"\n{TOML}");
        assert!(capped(&tool).unwrap_err().0.contains("function"));
        let core = TOML.replace("installable", "core");
        assert!(capped(&core).unwrap_err().0.contains("installable"));
        let unversioned = TOML.replace("version = \"1.0.0\"\n", "");
        assert!(capped(&unversioned).unwrap_err().0.contains("version"));
        assert!(capped("not toml [").is_err());
        let presence = TOML
            .replace("[\"voice/state\"]", "[\"room/presence\", \"voice/state\"]")
            .replace("light = true\n", "light = true\nbody = true\n");
        assert_eq!(
            capped(&presence).unwrap().1.spoken(),
            "its light, its head and arm, whether someone is at the desk and whether it is talking"
        );
    }

    /// The card's name and description, and `made: here` in its front
    /// matter, once; a card without them, or with a name that is not a
    /// skill's, is refused.
    #[test]
    fn the_card_is_marked_made_here() {
        let (name, description, marked) = card(CARD).unwrap();
        assert_eq!(name, "evening-dim");
        assert_eq!(description, "Dims the light a shade at 10 pm.");
        assert_eq!(marked.matches("made: here").count(), 1);
        assert!(marked.starts_with("---\nname: evening-dim\n"));
        assert!(marked.ends_with("At ten it dims.\n"));
        let again = card(&CARD.replace("name:", "made: there\nname:"))
            .unwrap()
            .2;
        assert_eq!(again.matches("made:").count(), 1);
        assert!(card("# No front matter\n").is_err());
        assert!(card(&CARD.replace("evening-dim", "Evening Dim")).is_err());
        assert!(
            card(&CARD.replace("description: Dims the light a shade at 10 pm.\n", "")).is_err()
        );
    }

    /// What the model is told: the skill's words, what it would use, and
    /// the line said word for word; or why it was not kept.
    #[test]
    fn the_note_says_what_it_would_do_and_that_it_needs_a_yes() {
        let made = Made {
            name: "evening-dim".into(),
            version: "1.0.0".into(),
            sha256: "ab".repeat(32),
            description: "Dims the light a shade at 10 pm.".into(),
            uses: Uses {
                light: true,
                body: false,
                topics: vec!["voice/state".into()],
            },
            package: PathBuf::from("/x"),
        };
        let told = note("Claude Code", "just", "your computer", &Ok(made));
        assert!(told.contains("\"evening-dim\": Dims the light a shade at 10 pm."));
        assert!(told.contains("It would use its light and whether it is talking."));
        assert!(told.contains(NEEDS_A_YES));
        assert!(!told.contains(&"ab".repeat(32)), "no hash for the model");
        let refused = note(
            "Claude Code",
            "just",
            "your computer",
            &Err(Refused::new("it asks to read more than a made skill may")),
        );
        assert!(refused.contains("can't keep it: it asks to read more"));
        assert!(!refused.contains(NEEDS_A_YES));
    }

    /// Without its program, nothing is kept and the lamp says it couldn't
    /// check it; the folder is made 0700.
    #[test]
    fn without_the_checker_nothing_is_kept() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("made-none-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let keeping = Keeping {
            directory: dir.clone(),
            program: dir.join("no-such-program"),
        };
        let kept = keep(&answer(TOML, CARD), false, "s:1:2", &keeping);
        assert_eq!(kept.unwrap_err().0, "the lamp couldn't check it");
        let mode = fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        assert!(index(&dir).is_empty());
        assert!(
            keep(&answer(TOML, CARD), true, "s:1:3", &keeping)
                .unwrap_err()
                .0
                .contains("cut off")
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
