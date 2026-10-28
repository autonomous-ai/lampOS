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
//! then says what it would do, what it asks to use, and asks [`ASK_TO_PUT_ON`]
//! (the lead, 20 Oct 08:37 on #1711: a spoken yes bound to the SHA-256, an
//! exception to the "only signed packages" ruling of 19 Oct 18:33 for a
//! skill made here alone). The ask is kept beside the package ([`offered`]);
//! on the person's spoken yes within [`PUT_ON_WITHIN`] the model calls
//! [`PUT_ON`], and [`put_on`] asks `lamp-skills`' manage helper for that
//! hash alone, which installs only those bytes, held again to the cap.
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
pub const ASK_TO_PUT_ON: &str = "Shall I put it on the lamp?";
/// The function the model calls on the person's spoken yes to [`ASK_TO_PUT_ON`].
pub const PUT_ON: &str = "put_on_made_skill";
/// A yes counts this long after the lamp asked, as a hand-over's does.
pub const PUT_ON_WITHIN: Duration = crate::agent_tasks::CONFIRM_WITHIN;
/// The manage helper's socket, `lamp_skills::package::MANAGE_SOCKET`
/// (lamp-skills' tests hold the two the same): root's, which puts a made
/// skill on by its SHA-256 alone.
pub const MANAGE_SOCKET: &str = "/run/lampos-skills-manage/manage.sock";
/// The longest the helper is waited for: its own 4 s for the skills
/// folder, and the install, while the person waits for the answer.
const PUT_ON_WAIT: Duration = Duration::from_secs(8);
/// The longest `lamp-skills pack` may take.
const PACK_WITHIN: Duration = Duration::from_secs(20);
/// The most of either file taken from an answer.
const MOST_FILE_BYTES: usize = 8 * 1024;
/// The most of a description said.
const MOST_DESCRIPTION_CHARS: usize = 200;
/// The file that keeps what was made, in [`DIRECTORY`].
pub const INDEX: &str = "made.json";

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
         kind = \"installable\", version = \"1.0.0\", no name key; [permissions] \
         with only light = true and/or body = true, topics only voice/state and room/presence; \
         one [trigger] on \
         voice/state (field \"state\", equals \"resting\") or room/presence (field \"someone\", \
         equals true or false), with for, cooldown, rearm = \"ends\"; a time of day as the \
         trigger's during = {{ from = \"22:00\", to = \"22:30\" }}; \
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
             confirm_agent_task. The skill is made, checked and kept, and put on the lamp only \
             if the person then says yes to putting it on ({PUT_ON})."
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

/// What a repair's brief says before the check's lines (#2813 piece 2).
pub const REPAIR_MARK: &str = "Your last answer did not pass the lamp's check, which said: ";
/// What it asks after them.
const REPAIR_ASK: &str = " Answer again with both blocks, fixed. ";
/// The check's lines a repair carries, at most, in bytes: with the longest
/// request the brief stays within the tasks part's 2,000 (the first
/// builder on #3126, 23 Oct 13:07).
pub const MOST_REPAIR_BYTES: usize = 600;

/// The brief for a repair (#2813 piece 2): when the lamp's check refuses
/// a made skill, one new hand-over to the same agent, whose new session
/// has none of the first, so it carries the whole [`brief`] again with
/// the check's `lines` ([`check_lines`]) before the request. Those lines
/// are the agent's own skill read back to it, never the conversation.
pub fn repair_brief(request: &str, lines: &str) -> Result<String, &'static str> {
    let brief = brief(request)?;
    let (head, request) = brief
        .rsplit_once(REQUEST_MARK)
        .expect("a brief ends with its request");
    Ok(format!(
        "{head}{REPAIR_MARK}{}{REPAIR_ASK}{REQUEST_MARK}{request}",
        cut_bytes(lines, MOST_REPAIR_BYTES)
    ))
}

/// Whether a [`brief`] is a repair's: its answer is never repaired again.
/// Read before the request, so a person's words never make one.
pub fn is_repair(task: &str) -> bool {
    is_brief(task)
        && task
            .rsplit_once(REQUEST_MARK)
            .is_some_and(|(head, _)| head.contains(REPAIR_MARK))
}

/// Why a made skill is not kept, in plain words the lamp can say; and when
/// the lamp's check refused it, the check's lines for a repair
/// ([`check_lines`]), never said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refused(pub String, pub Option<String>);

impl Refused {
    fn new(why: impl Into<String>) -> Self {
        Self(why.into(), None)
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
    // Asked of the Policy as a nudge, never as a hello, news or a reminder
    // (the reviewer on #2011): the act `lamp-skills` passes on.
    if let Some(ask) = table.get("ask")
        && ask.get("policy").and_then(|policy| policy.as_str()) != Some("nudge")
    {
        return Err(Refused::new(
            "it asks the Policy as something other than a nudge",
        ));
    }
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
    /// The product's, with the switch on in this process's environment:
    /// the coordinator's, which systemd gives it. A worker's is cleared, so
    /// a worker is told by its flag ([`Self::when`]).
    pub fn from_environment() -> Option<Self> {
        Self::when(std::env::var(ENVIRONMENT).ok().as_deref() == Some("on"))
    }

    /// The product's, if `on` (`agent_tasks::Place::make_skill`).
    pub fn when(on: bool) -> Option<Self> {
        on.then(|| Self {
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
    drop_unfinished(&keeping.directory);
    if let Some(made) = kept(&keeping.directory, from) {
        return Ok(made);
    }
    if cut {
        return Err(Refused::new("its answer was cut off"));
    }
    let files = files(answer)?;
    let (version, uses) = capped(&files.toml)?;
    let (name, description, marked) = card(&files.card)?;
    // A folder of this call's own: two keeps at once in one process (two
    // readers of the same news, as #3662's test runs them) never share
    // its check's words.
    static MAKING: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let work = keeping.directory.join(format!(
        ".making-{}-{}",
        std::process::id(),
        MAKING.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
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
    let packed = made.and_then(|()| pack(&keeping.program, &folder, &out, &name));
    let result = packed.and_then(|()| {
        let package = format!("{name}-{version}.tar");
        let bytes = fs::read(out.join(&package)).map_err(unkept)?;
        let sha256 = hex(&Sha256::digest(&bytes));
        let kept_as = keeping
            .directory
            .join(format!("{name}-{version}-{}.tar", &sha256[..12]));
        // Written whole beside it, and moved in only once the index names
        // it: a crash leaves a `.new` the next keep deletes, never a
        // package nothing names (the reviewer on #2011).
        let new = keeping
            .directory
            .join(format!(".{name}-{version}-{}.tar.new", &sha256[..12]));
        fs::write(&new, &bytes).map_err(unkept)?;
        let made = Made {
            name,
            version,
            sha256,
            description: cut_chars(&description, MOST_DESCRIPTION_CHARS),
            uses,
            package: kept_as,
        };
        remember(&keeping.directory, from, &made).map_err(unkept)?;
        fs::rename(&new, &made.package).map_err(unkept)?;
        Ok(made)
    });
    let _ = fs::remove_dir_all(&work);
    result
}

/// `lamp-skills pack --out OUT FOLDER`: the checks a load makes, its
/// permissions among them, and the package read back as an install would.
/// Its words go to a file beside OUT, in the private folder, so a long
/// answer never blocks it.
fn pack(program: &Path, folder: &Path, out: &Path, name: &str) -> Result<(), Refused> {
    let said = out.with_file_name("check.err");
    let stderr = fs::File::create(&said).map_err(|why| {
        eprintln!("lamp-voice: a made skill was not checked: {why}");
        Refused::new("the lamp couldn't check it")
    })?;
    let mut child = Command::new(program)
        .arg("pack")
        .arg("--out")
        .arg(out)
        .arg(folder)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
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
    let mut words = Vec::new();
    if let Ok(file) = fs::File::open(&said) {
        let _ = file.take(MOST_CHECK_BYTES).read_to_end(&mut words);
    }
    eprintln!(
        "lamp-voice: a made skill did not pass lamp-skills' check (exit {}): {}",
        status
            .code()
            .map_or("none".to_owned(), |code| code.to_string()),
        check_reason(&String::from_utf8_lossy(&words), folder, name)
    );
    Err(Refused(
        "it doesn't pass the lamp's check".into(),
        Some(check_lines(&String::from_utf8_lossy(&words), folder)),
    ))
}

/// The check's `said` for a repair's brief: every line, trimmed and joined
/// with " / ", the skill's folder (the lamp's own path) replaced, cut to
/// [`MOST_REPAIR_BYTES`]. Unlike [`check_reason`] its quotes are kept: they
/// are the agent's own words going back to it.
fn check_lines(said: &str, folder: &Path) -> String {
    let said = said.replace(&*folder.to_string_lossy(), "<folder>");
    let lines: Vec<&str> = said
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    cut_bytes(&lines.join(" / "), MOST_REPAIR_BYTES).to_owned()
}

/// The check's answer read, at most.
const MOST_CHECK_BYTES: u64 = 8 * 1024;
/// The reason logged, at most, in characters.
const MOST_REASON_CHARS: usize = 200;

/// The check's first line, for the journal (Mobile's #2813 rerun on
/// lamp-0c89, 23 Oct, gave only `exit 1`). lamp-skills' words can quote the
/// skill's, which come from the agent and the person's request (the
/// reviewer on #2011), so anything in quotes, the skill's own name and its
/// folder are left out, and the line is cut short.
fn check_reason(said: &str, folder: &Path, name: &str) -> String {
    let mut lines = said.lines().map(str::trim).filter(|line| !line.is_empty());
    let Some(line) = lines.next() else {
        return "it said nothing".into();
    };
    let line = line.strip_prefix("lamp-skills: ").unwrap_or(line);
    // A TOML parse error says where on its first line and why after the
    // quoted source ("unknown field `name`", Mobile's #2813 rerun): the
    // first line that is not the source or its marks is added.
    let because = if line.contains("TOML parse error") {
        lines.find(|line| !line.starts_with('|') && !line.starts_with(|c: char| c.is_ascii_digit()))
    } else {
        None
    };
    let line = match because {
        Some(because) => format!("{line}: {because}"),
        None => line.to_owned(),
    };
    let line = line.replace(&*folder.to_string_lossy(), "<folder>");
    let chars: Vec<char> = line.chars().collect();
    let word = |at: Option<&char>| at.is_some_and(|c| c.is_alphanumeric());
    let mut kept = String::new();
    let mut open: Option<char> = None;
    for (at, &c) in chars.iter().enumerate() {
        let before = at.checked_sub(1).and_then(|at| chars.get(at));
        // An apostrophe in a word ("skill's", "don't") neither opens nor
        // closes a quote.
        let in_word = word(before) && word(chars.get(at + 1));
        // A quote escaped inside a value printed with `{:?}` ("say \"hi\"")
        // neither closes nor opens one (the reviewer-lead's P3 on #3225).
        let escaped = before == Some(&'\\');
        match open {
            Some(quote)
                if !in_word && !escaped && c == if quote == '“' { '”' } else { quote } =>
            {
                kept.push('…');
                kept.push(c);
                open = None;
            }
            Some(_) => {}
            None if !word(before) && matches!(c, '"' | '\'' | '`' | '“') => {
                kept.push(c);
                open = Some(c);
            }
            None => kept.push(c),
        }
    }
    // A quote never closed hides the rest of the line.
    if open.is_some() {
        kept.push('…');
    }
    let kept = if name.is_empty() {
        kept
    } else {
        kept.replace(name, "<skill>")
    };
    cut_chars(&kept, MOST_REASON_CHARS)
}

/// The folder, made 0700 if it is not there; one already there is made
/// 0700 too.
pub(crate) fn ensure_private(directory: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir_all(directory)?;
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
}

/// A package a crash left half moved in (`.NAME.tar.new`), deleted.
/// How old a half-moved package must be to be a crash's: older than any
/// keep takes from writing it to moving it in, its check included. Two
/// readers keep the same answers, so a younger one may be another keep's,
/// named in the index a moment before it is moved in (Fourteen's P2 on
/// #3662).
const UNFINISHED_AFTER: Duration = Duration::from_secs(3 * PACK_WITHIN.as_secs());

fn drop_unfinished(directory: &Path) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let old = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > UNFINISHED_AFTER);
        if old
            && name
                .to_str()
                .is_some_and(|name| name.starts_with('.') && name.ends_with(".tar.new"))
        {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// The repairs (#2813 piece 2), by the news key of the answer each
/// repairs: claimed before its hand-over, then sent or not sent. So an
/// answer is repaired at most once, by whichever reader claims it first
/// (the coordinator's and the texts worker's read the same answers: the
/// reviewer-lead's P2 on #3662), and across the voice's restarts too.
pub const REPAIRS: &str = "repairs.json";
/// The lock every writer of [`REPAIRS`] holds across its read and write.
const REPAIRS_LOCK: &str = ".repairs.lock";
/// A repair is remembered this long: longer than an answer is read for
/// (the task news' 12 h), so none still read is ever let go.
const REPAIRS_FOR_MS: u64 = 24 * 3_600_000;

fn repairs(directory: &Path) -> Vec<Value> {
    fs::read(directory.join(REPAIRS))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<Value>>(&bytes).ok())
        .unwrap_or_default()
}

/// Read the repairs, let `change` decide, and write them back if it says
/// so, all under [`REPAIRS_LOCK`]; those older than [`REPAIRS_FOR_MS`] are
/// let go on each write.
fn repairs_transact<R>(
    directory: &Path,
    change: impl FnOnce(&mut Vec<Value>) -> (bool, R),
) -> io::Result<R> {
    ensure_private(directory)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(directory.join(REPAIRS_LOCK))?;
    rustix::fs::flock(&lock, rustix::fs::FlockOperation::LockExclusive)?;
    let mut entries = repairs(directory);
    let (write, told) = change(&mut entries);
    if write {
        let now = now_ms();
        entries.retain(|entry| {
            now.saturating_sub(entry["at_unix_ms"].as_u64().unwrap_or(0)) < REPAIRS_FOR_MS
        });
        let new = directory.join(format!(".{REPAIRS}.{}.new", std::process::id()));
        fs::write(
            &new,
            serde_json::to_vec(&entries).map_err(io::Error::other)?,
        )?;
        fs::rename(&new, directory.join(REPAIRS))?;
    }
    Ok(told)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
}

/// Whether the answer `key` has a repair, claimed or done: its news waits
/// for the repair's own.
pub fn repair_of(directory: &Path, key: &str) -> bool {
    repairs(directory).iter().any(|entry| entry["key"] == key)
}

/// Claim the repair of the answer `key`, under the lock: true only for the
/// one claim that finds none, so only that reader hands it over. A claim
/// that can't be written is none.
pub fn claim_repair(directory: &Path, key: &str) -> bool {
    repairs_transact(directory, |entries| {
        if entries.iter().any(|entry| entry["key"] == key) {
            return (false, false);
        }
        entries.push(json!({"key": key, "at_unix_ms": now_ms()}));
        (true, true)
    })
    .unwrap_or_else(|error| {
        eprintln!("lamp-voice: a made skill's repair was not claimed: {error}");
        false
    })
}

/// The claimed repair of the answer `key` was handed over as the task
/// `to` (`store:id`), or, with none, not sent. The task is kept so the
/// app's ask can follow the repair on a later read, should the voice stop
/// before it does (Fourteen's P3 on #3662).
pub fn repaired_to(directory: &Path, key: &str, to: Option<&str>) -> io::Result<()> {
    repairs_transact(directory, |entries| {
        for entry in entries.iter_mut().filter(|entry| entry["key"] == key) {
            entry["sent"] = json!(to.is_some());
            if let Some(to) = to {
                entry["task"] = json!(to);
            }
        }
        (true, ())
    })
}

/// The task the repair of the answer `key` was handed over as, if it was.
pub fn repair_task(directory: &Path, key: &str) -> Option<String> {
    repairs(directory)
        .into_iter()
        .find(|entry| entry["key"] == key)
        .and_then(|entry| entry["task"].as_str().map(str::to_owned))
}

/// How long a claim may stand with no outcome: far past the hand-over's
/// own 5 s wait. Older, the voice stopped between the claim and the
/// hand-over's end, and nothing can say whether it was sent.
pub const CLAIM_STALE_MS: u64 = 120_000;

/// Whether the claimed repair of the answer `key` was not sent, as at
/// `now_unix_ms`: marked so, or claimed and left with no outcome past
/// [`CLAIM_STALE_MS`]. Then the answer is told not kept, as with no repair,
/// once (the reviewer-lead's P3 on #3662).
pub fn repair_not_sent_at(directory: &Path, key: &str, now_unix_ms: u64) -> bool {
    repairs(directory).iter().any(|entry| {
        entry["key"] == key
            && (entry["sent"] == false
                || entry["sent"].is_null()
                    && now_unix_ms.saturating_sub(entry["at_unix_ms"].as_u64().unwrap_or(0))
                        > CLAIM_STALE_MS)
    })
}

/// [`repair_not_sent_at`] now.
pub fn repair_not_sent(directory: &Path, key: &str) -> bool {
    repair_not_sent_at(directory, key, now_ms())
}

/// What is kept, oldest first.
/// The made skills' index as kept, for the app's asks
/// (`crate::app_make`): read only.
pub fn entries(directory: &Path) -> Vec<Value> {
    index(directory)
}

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
    write_index(directory, &entries)
}

/// The index written whole beside it and moved over it.
fn write_index(directory: &Path, entries: &[Value]) -> io::Result<()> {
    let new = directory.join(format!(".{INDEX}.new"));
    let mut file = fs::File::create(&new)?;
    file.write_all(&serde_json::to_vec(entries).map_err(io::Error::other)?)?;
    file.sync_all()?;
    fs::rename(&new, directory.join(INDEX))
}

/// What the model is told of a made skill, or of one not kept: the news
/// of the hand-over, in place of its answer, which is files and never
/// read out.
pub fn note(
    words: &crate::skill_words::NewsWords,
    who: &str,
    when: &str,
    computer: &str,
    made: &Result<Made, Refused>,
) -> String {
    use crate::skill_words::fill;
    let said = match made {
        Ok(made) => fill(
            &words.made,
            &[
                ("who", who),
                ("when", when),
                ("name", &made.name),
                ("description", &made.description),
                ("uses", &made.uses.spoken()),
                ("ask", ASK_TO_PUT_ON),
                ("put_on", PUT_ON),
            ],
        ),
        Err(Refused(why, _)) => fill(
            &words.not_kept,
            &[
                ("who", who),
                ("when", when),
                ("why", why),
                ("computer", computer),
            ],
        ),
    };
    crate::task_news::unprompted(&said)
}

/// Keep, beside the package whose SHA-256 is `sha256`, that the lamp asked
/// [`ASK_TO_PUT_ON`] at `now_unix_ms`: what a yes is held to ([`put_on`]).
/// Only the newest ask waits; another skill asked about later takes its
/// place.
pub fn offered(directory: &Path, sha256: &str, now_unix_ms: u64) -> io::Result<()> {
    let mut entries = index(directory);
    let mut found = false;
    for entry in &mut entries {
        if entry["sha256"] == sha256 {
            entry["offered_at_unix_ms"] = json!(now_unix_ms);
            found = true;
        }
    }
    if found {
        write_index(directory, &entries)?;
    }
    Ok(())
}

/// The lamp has just asked [`ASK_TO_PUT_ON`] about the made skill from the
/// news `key` (a task's answer, kept by [`keep`] with it as `from`): heard
/// whole at the lamp, or given in a text. Kept as asked at `now_unix_ms`,
/// unless it is on already. The news is not dated when it is read, since
/// the lamp tells it later, at a good moment (the reviewer's P2 on #2424).
pub fn asked(directory: &Path, key: &str, now_unix_ms: u64) -> io::Result<()> {
    let made = index(directory)
        .into_iter()
        .find(|entry| entry["from"] == key && entry["on"] != true);
    match made.as_ref().and_then(|entry| entry["sha256"].as_str()) {
        Some(sha256) => offered(directory, sha256, now_unix_ms),
        None => Ok(()),
    }
}

/// Whether the made skill from the news `key` is on the lamp already: its
/// news then has nothing left to ask, and isn't told again (Reviewer-three's
/// P3 on #4240: news played through, and so told again, asked to put on a
/// skill a yes had already put on).
pub fn is_on(directory: &Path, key: &str) -> bool {
    index(directory)
        .iter()
        .any(|entry| entry["from"] == key && entry["on"] == true)
}

/// The made skill the lamp asked about within [`PUT_ON_WITHIN`] before
/// `now_unix_ms` and not yet on: its name and SHA-256. The newest ask
/// alone: once it is on, or out of its time, none. lamp-skills' manage
/// helper holds `install_made` to it too, by its own clock, so a kept
/// SHA-256 is never put on later by any path.
pub fn waiting(directory: &Path, now_unix_ms: u64) -> Option<(String, String)> {
    let within = PUT_ON_WITHIN.as_millis() as u64;
    // The newest ask first, whatever became of it: once it is on, or with
    // the clock before it, nothing waits, and an older ask never stands in
    // for it (reviewer-two's P2 on #2874).
    let (at, entry) = index(directory)
        .into_iter()
        .filter_map(|entry| Some((entry["offered_at_unix_ms"].as_u64()?, entry)))
        .max_by_key(|(at, _)| *at)?;
    (entry["on"] != true && at <= now_unix_ms && now_unix_ms - at <= within).then(|| {
        (
            entry["name"].as_str().unwrap_or_default().to_owned(),
            entry["sha256"].as_str().unwrap_or_default().to_owned(),
        )
    })
}

/// The note a request is given while a made skill waits for its yes (the
/// second route keeps its exchanges' words, not their calls, as
/// `agent_tasks::pending_note` says): without it a yes is a yes to nothing.
pub fn waiting_note(directory: &Path, now_unix_ms: u64) -> Option<String> {
    waiting(directory, now_unix_ms).map(|(name, _)| {
        format!(
            "(For you to know, not to answer: you have asked the person whether to put the skill \"{name}\" on the lamp. If they say yes now, call {PUT_ON}; if they say no, don't, and it stays off.)"
        )
    })
}

/// The function the model calls on the person's yes to [`ASK_TO_PUT_ON`].
pub fn put_on_tool() -> lamp_gemini::Tool {
    lamp_gemini::Tool {
        name: PUT_ON.into(),
        description: format!(
            "Put on the lamp the skill it just made, which you asked about (\"{ASK_TO_PUT_ON}\"). \
             Call it only when the person has just said yes to that question out loud."
        ),
        parameters: None,
        silent: false,
    }
}

/// On the person's yes ([`PUT_ON`]), put on the made skill waiting for it,
/// by its SHA-256 alone, through the manage helper at `socket`, which
/// installs only those bytes, held again to the cap. Only a yes said out
/// loud counts, and only a plain one (`said`: the words heard in the line,
/// `None` for one not heard). The lead's decision is a spoken yes, so a
/// typed one, or one the lamp wrote, puts nothing on, and neither does a
/// "no, not yet" the model took for a yes (the reviewer's P3 on #2424, as
/// the held forget's plain yes). What the model is told.
pub fn put_on(directory: &Path, socket: &Path, said: Option<&str>, now_unix_ms: u64) -> Value {
    put_on_within(directory, socket, said, now_unix_ms, PUT_ON_WAIT)
}

/// [`put_on`], waiting at most `wait` for the helper.
pub fn put_on_within(
    directory: &Path,
    socket: &Path,
    said: Option<&str>,
    now_unix_ms: u64,
    wait: Duration,
) -> Value {
    let Some(said) = said else {
        return json!({
            "error": "only a yes said out loud puts a made skill on the lamp",
            "next": "Tell them in a few words to say yes out loud at the lamp to put it on."
        });
    };
    if !crate::known_faces::plain_agreement(said) {
        return json!({
            "error": "what they said was not a plain yes",
            "next": "Ask them once more, in a few words, whether to put it on the lamp."
        });
    }
    let Some((name, sha256)) = waiting(directory, now_unix_ms) else {
        return json!({
            "error": "no skill made here is waiting for a yes",
            "next": "Tell them in a few words that there is nothing waiting to be put on."
        });
    };
    // A helper not there (no socket, or nobody listening) never puts it on:
    // only one asked and silent may still (the reviewer's P3 on #2424).
    let Some(answer) = crate::tools::reached(socket, &json!({ "install_made": sha256 }), wait)
    else {
        eprintln!("lamp-voice: a made skill: no skills helper to put it on: {name}");
        return json!({
            "error": "it couldn't be put on",
            "next": "Tell them in one short sentence that it couldn't be put on the lamp, with no technical words."
        });
    };
    match answer.as_ref().map(|answer| answer.get("done").is_some()) {
        // No answer in time: the helper may still put it on, so the person
        // is told that, not that it failed.
        None => {
            eprintln!("lamp-voice: a made skill: the skills helper didn't answer in time: {name}");
            json!({
                "error": "the lamp didn't say in time whether it is on",
                "next": "Tell them in a few words that it is being put on, and that they can ask in a minute whether it is on."
            })
        }
        Some(true) => {
            let mut entries = index(directory);
            for entry in &mut entries {
                if entry["sha256"] == sha256.as_str() {
                    entry["on"] = json!(true);
                }
            }
            if let Err(error) = write_index(directory, &entries) {
                eprintln!("lamp-voice: a made skill is on, but not marked so: {error}");
            }
            eprintln!("lamp-voice: a made skill is on the lamp: {name}, sha256 {sha256}");
            json!({
                "put_on": name,
                "next": "Tell them in a few words that it is on the lamp now."
            })
        }
        Some(false) => {
            let why = answer
                .as_ref()
                .and_then(|answer| answer["refused"].as_str())
                .unwrap_or("");
            eprintln!("lamp-voice: a made skill was not put on: {name}: {why}");
            // One they put on another way has its name (#2414): said, not
            // replaced.
            if why.contains("put on another way") {
                return json!({
                    "error": format!("a skill called {name} is already on the lamp, put on another way"),
                    "next": format!("Tell them in one short sentence that a skill called {name} is already on the lamp, so this one wasn't put on in its place.")
                });
            }
            json!({
                "error": "it couldn't be put on",
                "next": "Tell them in one short sentence that it couldn't be put on the lamp, with no technical words."
            })
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// `text`'s first `most` bytes, cut on a character.
fn cut_bytes(text: &str, most: usize) -> &str {
    let mut at = most.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    &text[..at]
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
during = { from = "22:00", to = "22:30" }

[ask]
policy = "nudge"

[[do]]
light = { dim = 1 }
"#;

    const CARD: &str = "---\nname: evening-dim\ndescription: Dims the light a shade at 10 pm.\n---\n\n# Evening dim\n\nAt ten it dims.\n";

    fn answer(toml: &str, card: &str) -> String {
        format!("Here it is.\n\n```skill.toml\n{toml}```\n\n```markdown SKILL.md\n{card}```\n")
    }

    /// The brief and the longest request fit in 1,000 bytes, half the tasks
    /// part's 2,000, so a repair has room for the check's lines;
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
        // A time of day is the trigger's own window (#2813's rerun on
        // lamp-0c89: a 23.5 h [quiet] around 22:00, as the brief once said).
        assert!(
            asked.contains("during = { from = \"22:00\", to = \"22:30\" }"),
            "{asked}"
        );
        assert!(!asked.contains("from its end to its start"), "{asked}");
        // The name is SKILL.md's alone: a `name =` in skill.toml fails
        // lamp-skills' check as an unknown field (Mobile on lamp-0c89, #2813).
        assert!(asked.contains("no name key"), "{asked}");
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
        let reminder = TOML.replace("policy = \"nudge\"", "policy = \"reminder\"");
        assert!(
            capped(&reminder)
                .unwrap_err()
                .0
                .contains("other than a nudge")
        );
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
    fn the_note_says_what_it_would_do_and_asks_to_put_it_on() {
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
        let told = note(
            &crate::skill_words::NewsWords::built_in(),
            "Claude Code",
            "just",
            "your computer",
            &Ok(made),
        );
        assert!(told.contains("\"evening-dim\": Dims the light a shade at 10 pm."));
        assert!(told.contains("It would use its light and whether it is talking."));
        assert!(told.contains(ASK_TO_PUT_ON));
        assert!(!told.contains(&"ab".repeat(32)), "no hash for the model");
        let refused = note(
            &crate::skill_words::NewsWords::built_in(),
            "Claude Code",
            "just",
            "your computer",
            &Err(Refused::new("it asks to read more than a made skill may")),
        );
        assert!(refused.contains("can't keep it: it asks to read more"));
        assert!(!refused.contains(ASK_TO_PUT_ON));
    }

    /// The keeping is the product's only with the switch: a worker's from
    /// its place's flag (`when`), the coordinator's from its environment.
    #[test]
    fn the_keeping_is_the_products_only_with_the_switch() {
        assert_eq!(
            Keeping::when(true),
            Some(Keeping {
                directory: PathBuf::from(DIRECTORY),
                program: PathBuf::from(PROGRAM),
            })
        );
        assert_eq!(Keeping::when(false), None);
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

    /// #2813 piece 2 (the first builder, 23 Oct 13:07): a repair's brief
    /// is the brief again with the check's lines, cut to 600 bytes, before
    /// the request, so it still fits the tasks part's 2,000 bytes with the
    /// longest request; it is a brief, its request is the person's, and it
    /// is a repair; a first brief is not, even with the mark in its request.
    #[test]
    fn a_repairs_brief_carries_the_checks_lines_and_still_fits() {
        let request = "x".repeat(MOST_REQUEST_BYTES);
        let lines = "y".repeat(5_000);
        let repair = repair_brief(&request, &lines).unwrap();
        assert!(
            repair.len() <= crate::agent_tasks::MOST_TASK_BYTES,
            "{}",
            repair.len()
        );
        assert!(repair.contains(&"y".repeat(MOST_REPAIR_BYTES)));
        assert!(!repair.contains(&"y".repeat(MOST_REPAIR_BYTES + 1)));
        assert!(is_brief(&repair) && is_repair(&repair));
        assert_eq!(request_of(&repair), request);
        let first = brief(&request).unwrap();
        assert!(is_brief(&first) && !is_repair(&first));
        let quoted = brief(&format!("dim it, {REPAIR_MARK}")).unwrap();
        assert!(
            !is_repair(&quoted),
            "the person's words never make a repair"
        );
        // Cut on a character, never inside one.
        let wide = repair_brief("dim it", &"é".repeat(400)).unwrap();
        assert!(wide.contains(&"é".repeat(MOST_REPAIR_BYTES / 2)));
        assert!(!wide.contains(&"é".repeat(MOST_REPAIR_BYTES / 2 + 1)));
        assert_eq!(repair_brief("", "x"), Err("Say what the skill should do."));
    }

    /// A made skill the check refuses comes back with the check's lines for
    /// a repair, its folder replaced, every line kept; a refusal that is no
    /// check's carries none.
    #[test]
    fn a_failed_check_gives_its_lines_with_the_folder_replaced() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("made-check-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let program = dir.join("pack");
        fs::write(
            &program,
            "#!/bin/sh\necho \"lamp-skills: not packed: $4/skill.toml: TOML parse error at line 3\" >&2\necho '  |' >&2\necho 'unknown field `name`' >&2\nexit 1\n",
        )
        .unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        let keeping = Keeping {
            directory: dir.join("made"),
            program,
        };
        let refused = keep(&answer(TOML, CARD), false, "s:1:2", &keeping).unwrap_err();
        assert_eq!(refused.0, "it doesn't pass the lamp's check");
        let lines = refused.1.expect("the check's lines");
        assert_eq!(
            lines,
            "lamp-skills: not packed: <folder>/skill.toml: TOML parse error at line 3 / | / unknown field `name`"
        );
        assert!(!lines.contains(".making-"), "{lines}");
        assert_eq!(
            check_lines(&"z".repeat(900), Path::new("/f")).len(),
            MOST_REPAIR_BYTES
        );
        let cut = keep(&answer(TOML, CARD), true, "s:1:3", &keeping).unwrap_err();
        assert!(cut.1.is_none(), "{cut:?}");
        let _ = fs::remove_dir_all(&dir);
    }

    /// The check's first line goes to the journal with the skill's words
    /// left out: quoted text, its name and its folder (Mobile's #2813 rerun:
    /// only `exit 1` was logged).
    #[test]
    fn the_checks_reason_is_logged_without_the_skills_words() {
        let folder = Path::new("/var/lib/made/.making-7/skill/evening-dim");
        let reason = |said: &str| check_reason(said, folder, "evening-dim");
        assert_eq!(
            reason(
                "lamp-skills: not packed: /var/lib/made/.making-7/skill/evening-dim/skill.toml: \
                 [[do]] light: unknown key \"glow\" (expected colour, level, for or dim)\n\
                 more lines\n"
            ),
            "not packed: <folder>/skill.toml: [[do]] light: unknown key \"…\" (expected colour, level, for or dim)"
        );
        // Its name, an apostrophe in a word, single quotes, backticks and
        // curly quotes.
        assert_eq!(
            reason("not packed: evening-dim's skill.toml says no version: a package carries one"),
            "not packed: <skill>'s skill.toml says no version: a package carries one"
        );
        assert_eq!(
            reason("gesture 'wave to the cat' isn't one of `nod`, “curious”"),
            "gesture '…' isn't one of `…`, “…”"
        );
        // An apostrophe inside a quote doesn't end it early.
        assert_eq!(reason("said 'don't wake me' at 7"), "said '…' at 7");
        // A TOML parse error gives its why after the quoted source, which is
        // left out.
        assert_eq!(
            reason(
                "lamp-skills: not packed: skill.toml: TOML parse error at line 1, column 1\n  |\n\
                 1 | name = \"evening-dim\"\n  | ^^^^\nunknown field `name`, expected one of `kind`\n"
            ),
            "not packed: skill.toml: TOML parse error at line 1, column 1: unknown field `…`, expected one of `…`"
        );
        // Nor does one escaped in a `{:?}` value.
        assert_eq!(
            reason(r#"unknown say "call \"home\" now" in [[do]]"#),
            r#"unknown say "…" in [[do]]"#
        );
        // A quote never closed hides the rest.
        assert_eq!(reason("bad value \"call home at"), "bad value \"…");
        assert_eq!(reason("\n  \n"), "it said nothing");
        let long = reason(&"word ".repeat(100));
        assert!(long.chars().count() <= MOST_REASON_CHARS + 1, "{long}");
        assert!(long.ends_with('…'));
    }

    /// A check that says a lot and fails is answered "doesn't pass" at
    /// once: its words go to a file, so a full pipe never holds it.
    #[test]
    fn a_check_that_says_a_lot_still_ends() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("made-loud");
        let program = dir.join("lamp-skills");
        fs::write(
            &program,
            "#!/bin/sh\nhead -c 200000 /dev/zero | tr '\\0' x >&2\nexit 1\n",
        )
        .unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        let keeping = Keeping {
            directory: dir.join("made"),
            program,
        };
        let began = Instant::now();
        let kept = keep(&answer(TOML, CARD), false, "s:1:4", &keeping);
        assert_eq!(kept.unwrap_err().0, "it doesn't pass the lamp's check");
        assert!(began.elapsed() < PACK_WITHIN, "{:?}", began.elapsed());
        assert!(index(&keeping.directory).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    /// A fresh folder of the test's own, for its process.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// An index of two made skills, as `keep` writes it.
    fn two_kept(directory: &Path) {
        fs::create_dir_all(directory).unwrap();
        write_index(
            directory,
            &[
                json!({"name": "evening-dim", "sha256": "a".repeat(64), "package": "evening-dim.tar", "from": "s1:7:1"}),
                json!({"name": "morning-glow", "sha256": "b".repeat(64), "package": "morning-glow.tar", "from": "s1:8:1"}),
            ],
        )
        .unwrap();
    }

    /// A helper that answers `answer` to one line, and gives back what it was
    /// asked.
    fn helper(socket: &Path, answer: &'static str) -> std::thread::JoinHandle<String> {
        use std::io::{BufRead, BufReader};
        let listener = std::os::unix::net::UnixListener::bind(socket).unwrap();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            writeln!(&stream, "{answer}").unwrap();
            line
        })
    }

    /// The ask waits [`PUT_ON_WITHIN`], the newest ask alone, and the note
    /// says which skill a yes would put on.
    /// A package left half moved in by a crash is deleted, but never one
    /// another keep is moving in right now: only one older than any keep
    /// takes (Fourteen's P2 on #3662: two readers keep the same answers).
    #[test]
    fn only_a_package_left_long_ago_is_dropped_as_unfinished() {
        let dir = std::env::temp_dir().join(format!("made-unfinished-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let fresh = dir.join(".evening-dim-1.0.0-abc.tar.new");
        let left = dir.join(".evening-dim-1.0.0-def.tar.new");
        fs::write(&fresh, b"x").unwrap();
        fs::write(&left, b"x").unwrap();
        fs::File::options()
            .write(true)
            .open(&left)
            .unwrap()
            .set_modified(SystemTime::now() - UNFINISHED_AFTER - Duration::from_secs(1))
            .unwrap();
        drop_unfinished(&dir);
        assert!(fresh.exists(), "another keep's package, moving in now");
        assert!(!left.exists(), "a crash's, long ago");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_the_newest_ask_waits_and_only_for_two_minutes() {
        let dir = scratch("mk-wait");
        let made = dir.join("made");
        two_kept(&made);
        assert_eq!(waiting_note(&made, 1_000), None);
        offered(&made, &"a".repeat(64), 1_000).unwrap();
        // Asked by its news' key, as a told one is.
        asked(&made, "s1:8:1", 5_000).unwrap();
        let within = PUT_ON_WITHIN.as_millis() as u64;
        assert_eq!(
            waiting(&made, 6_000),
            Some(("morning-glow".to_owned(), "b".repeat(64)))
        );
        let note = waiting_note(&made, 6_000).unwrap();
        assert!(
            note.contains("\"morning-glow\"") && note.contains(PUT_ON),
            "{note}"
        );
        assert_eq!(
            waiting(&made, 5_000 + within),
            Some(("morning-glow".to_owned(), "b".repeat(64)))
        );
        assert_eq!(waiting(&made, 5_001 + within), None);
        // An ask for a SHA-256 nothing kept changes nothing.
        offered(&made, &"c".repeat(64), 7_000).unwrap();
        assert_eq!(waiting(&made, 7_000).unwrap().0, "morning-glow");
    }

    /// On a spoken yes, the helper is asked for the waiting skill's SHA-256
    /// alone, and once it is on, nothing waits.
    #[test]
    fn a_spoken_yes_asks_the_helper_for_that_sha256_and_nothing_else() {
        let dir = scratch("mk-yes");
        let made = dir.join("made");
        two_kept(&made);
        offered(&made, &"a".repeat(64), 1_000).unwrap();
        let socket = dir.join("manage.sock");
        let asked = helper(&socket, r#"{"done": "installed evening-dim"}"#);
        let told = put_on(&made, &socket, Some("Yes."), 2_000);
        assert_eq!(told["put_on"], "evening-dim", "{told}");
        let asked: Value = serde_json::from_str(&asked.join().unwrap()).unwrap();
        assert_eq!(asked, json!({"install_made": "a".repeat(64)}));
        assert_eq!(waiting(&made, 2_000), None, "on, so nothing waits");
        let again = put_on(&made, &socket, Some("Yes."), 2_000);
        assert_eq!(
            again["error"], "no skill made here is waiting for a yes",
            "{again}"
        );
    }

    /// A yes not said out loud, or no ask waiting, asks the helper nothing;
    /// a helper's refusal leaves the skill off and still waiting.
    #[test]
    fn no_spoken_yes_or_no_ask_puts_nothing_on_and_a_refusal_is_told() {
        let dir = scratch("mk-no");
        let made = dir.join("made");
        two_kept(&made);
        let socket = dir.join("nobody.sock");
        let told = put_on(&made, &socket, Some("Yes."), 2_000);
        assert_eq!(
            told["error"], "no skill made here is waiting for a yes",
            "{told}"
        );
        offered(&made, &"a".repeat(64), 1_000).unwrap();
        let typed = put_on(&made, &socket, None, 2_000);
        assert_eq!(
            typed["error"], "only a yes said out loud puts a made skill on the lamp",
            "{typed}"
        );
        let not_yes = put_on(&made, &socket, Some("No, not yet."), 2_000);
        assert_eq!(
            not_yes["error"], "what they said was not a plain yes",
            "{not_yes}"
        );
        // No helper there: it is not being put on (the reviewer's P3 on #2424).
        let nobody = put_on(&made, &socket, Some("Yes."), 2_000);
        assert_eq!(nobody["error"], "it couldn't be put on", "{nobody}");
        // A helper asked and silent may still put it on.
        let socket = dir.join("silent.sock");
        let _listening = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let silent = put_on_within(
            &made,
            &socket,
            Some("Yes."),
            2_000,
            Duration::from_millis(200),
        );
        assert_eq!(
            silent["error"], "the lamp didn't say in time whether it is on",
            "{silent}"
        );
        let socket = dir.join("manage.sock");
        let asked = helper(
            &socket,
            r#"{"refused": "not installed: its SHA-256 has changed"}"#,
        );
        let refused = put_on(&made, &socket, Some("Yes."), 2_000);
        asked.join().unwrap();
        assert_eq!(refused["error"], "it couldn't be put on", "{refused}");
        assert_eq!(waiting(&made, 2_000).unwrap().0, "evening-dim");
        let socket = dir.join("in-the-way.sock");
        let asked = helper(
            &socket,
            r#"{"refused": "not installed: evening-dim is already on the lamp, put on another way, and a skill made here doesn't take its place"}"#,
        );
        let in_the_way = put_on(&made, &socket, Some("Yes."), 2_000);
        asked.join().unwrap();
        assert_eq!(
            in_the_way["error"],
            "a skill called evening-dim is already on the lamp, put on another way",
            "{in_the_way}"
        );
    }
}
