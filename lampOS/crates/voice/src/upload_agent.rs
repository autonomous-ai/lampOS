//! A bare `SKILL.md` added from a file, run by the lamp's agent
//! (`LAMP_UPLOAD_TO_AGENT`, off unless `on`, set in no unit; the owner's
//! ask, 12 Nov: a skill uploaded from the app runs through the lamp's
//! Hermes, as V1's did). With it off, a bare `SKILL.md` is a written skill
//! as before ([`crate::upload_skill::written`], at most 900 bytes).
//!
//! On, a bare `SKILL.md` of at most [`MOST_BYTES`] becomes an agent skill
//! ([`files`]):
//! - **Its instructions are the file**, kept whole in its folder as
//!   [`INSTRUCTIONS`] (`scripts/SKILL.md`), never read by the lamp past its
//!   front matter, its trigger phrases and the words [`grants_read`] looks
//!   for.
//! - **A voice tool** (`tool = name`, `-` as `_`): its card's description,
//!   the tool's note, is the file's description and its trigger phrases
//!   ([`triggers`]), so the model calls it when the person says one. A call
//!   runs one `do.task` step, `task = { task, run = "scripts/SKILL.md" }`:
//!   lamp-skills hands it to lamp-tasks, which runs the lamp's Hermes held
//!   (its namespace, a fresh home per run, no `admin.sock`, the run's own
//!   one-run token for its grants), told to follow the file.
//! - **Its grants are read from what the file asks for** ([`grants_read`]),
//!   conservatively: `camera`, `light`, `speak` (its voice and the sounds
//!   it plays), `sound` (hearing kinds of sound), and `net` when it
//!   names a web API, a URL or HTTP (with `LAMP_TASK_NET` on; off, such a
//!   file is refused, [`NO_NET`]). Each is said back when it goes on.
//! - **On at once, or a spoken yes once** (the first builder's 19:14 line):
//!   it goes on as an uploaded skill does today (#5977, said aloud). With
//!   grants only of [`AT_ONCE`] (light, speak, body) its task's yes is
//!   recorded by lamp-control as it goes on ([`went_on`]); with anything
//!   else, `sound` (hearing kinds of sound), the microphone, the camera, a
//!   send or `net` ([`asks_yes`]), its first run asks the spoken yes through
//!   the task-yes path ([`first_run`], `crate::skill_task_yes`), once.
//! - **No secret is the lamp's** (the first builder's 19:44 ruling): a key
//!   or token is never said aloud, asked by voice or kept in the skill's
//!   folder. Its way in (the app, into root's store as a service's tokens,
//!   reaching the run as an environment file read once) is a follow-up, so
//!   for now a file carrying a value that looks like one is refused in
//!   plain words ([`carries_secret`], [`HAS_SECRET`]), the value never
//!   journaled. The run's environment has none (lamp-tasks clears it to the
//!   control socket, the run's token and the SDK's path).
use crate::made_skill::{self, Files, Keeping, Made, Refused};
use serde_json::Value;
use std::path::Path;

/// The switch, in lamp-cloud, lamp-voice, lamp-skills and its manage
/// helper. `on` only.
pub const ENVIRONMENT: &str = "LAMP_UPLOAD_TO_AGENT";
/// The most bytes of the file.
pub const MOST_BYTES: usize = 64 * 1_024;
/// The line an agent skill's card carries beside `made: here` and
/// [`made_skill::ADDED_FROM_FILE`].
pub const MARK: &str = "agent: file";
/// Where the file is kept in the skill's folder, the task's `run`.
pub const INSTRUCTIONS: &str = "scripts/SKILL.md";
/// The grants that go on at once, with no spoken yes (the first builder's
/// 19:44 and 20:16 lines): the same set as #6659's
/// `made_skill::CODE_AT_ONCE_GRANTS`, mirrored here until that lands.
/// `sound` is hearing kinds of sound (lamp-control's `Grant::Sound`), so it
/// keeps its yes; playing a sound is `speak`.
pub const AT_ONCE: [&str; 3] = ["light", "speak", "body"];
/// Where the script is said to come from in the say-back.
pub const FROM: &str = "a file added in the app";
/// What a file that names a web API is told with `LAMP_TASK_NET` off.
pub const NO_NET: &str =
    "it reaches the internet, which a skill's task can't on this lamp (LAMP_TASK_NET is off)";
/// What a file carrying a key or token is told: never the value.
pub const HAS_SECRET: &str = "it carries what looks like a key or a token; a skill added from a \
                              file can't hold one yet, so add it without the key or token";

/// Whether `text` carries a value that looks like a key or a token: a run
/// of at least 24 of `A-Z a-z 0-9 _ - . :` with at least four letters and
/// four digits, outside a link (no `/` in it). Never says which.
pub fn carries_secret(text: &str) -> bool {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':' | '/')))
        .filter(|run| !run.contains('/'))
        .any(|run| {
            let letters = run.chars().filter(char::is_ascii_alphabetic).count();
            let digits = run.chars().filter(char::is_ascii_digit).count();
            run.len() >= 24 && letters >= 4 && digits >= 4
        })
}

/// The most trigger phrases put in the tool's note.
pub const MOST_TRIGGERS: usize = 8;
/// The most of one trigger phrase, in characters.
const MOST_TRIGGER_CHARS: usize = 80;
/// The most of the tool's note: a tool's description is at most 1,024
/// bytes (lamp-skills' check), the card's quotes and label beside it.
const MOST_NOTE_BYTES: usize = 800;
/// The most of the task's words, from the file's description.
const MOST_TASK_BYTES: usize = 300;

thread_local! {
    /// The switch as a test of this thread sets it (other crates' tests
    /// too, so not only under `cfg(test)`).
    static ON_HERE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Whether the switch is on in this process.
pub fn on() -> bool {
    if let Some(on) = ON_HERE.with(std::cell::Cell::get) {
        return on;
    }
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// The switch for this test's thread alone: tests only.
#[doc(hidden)]
pub fn set_on_here(on: Option<bool>) {
    ON_HERE.with(|here| here.set(on));
}

/// Whether a `SKILL.md` is an agent skill's card ([`MARK`]).
pub fn is_agent_card(card: &str) -> bool {
    card.lines().any(|line| line.trim() == MARK)
}

/// The file's words, lowercased, split on anything but a letter or digit.
fn words(text: &str) -> std::collections::BTreeSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The grants `text` asks for, in [`crate::skill_task_yes::GRANTS`]'s
/// order and then `net`: only these five, by plain words, never more.
pub fn grants_read(text: &str) -> Vec<String> {
    let said = words(text);
    let any = |list: &[&str]| list.iter().any(|word| said.contains(*word));
    let lower = text.to_lowercase();
    let mut grants = Vec::new();
    if any(&[
        "light", "lights", "flash", "flashes", "flashing", "led", "glow",
    ]) {
        grants.push("light");
    }
    // Its voice and the sounds it plays are `speak`, lamp-control's.
    if any(&[
        "say", "says", "saying", "speak", "speaks", "aloud", "announce", "sound", "sounds", "beep",
        "chime",
    ]) {
        grants.push("speak");
    }
    // Hearing kinds of sound (`sound`): only a clap or a knock named.
    if any(&["clap", "claps", "clapping", "knock", "knocks", "knocking"]) {
        grants.push("sound");
    }
    if any(&[
        "camera", "photo", "photos", "picture", "pictures", "snapshot", "selfie",
    ]) {
        grants.push("camera");
    }
    if lower.contains("http://")
        || lower.contains("https://")
        || any(&["api", "apis", "url", "http", "https", "webhook", "endpoint"])
    {
        grants.push(crate::skill_task_yes::NET.0);
    }
    grants.into_iter().map(str::to_owned).collect()
}

/// Whether a skill with `grants` keeps its spoken yes (the camera, the
/// microphone, a send or `net`): anything beyond [`AT_ONCE`].
pub fn asks_yes(grants: &[String]) -> bool {
    grants
        .iter()
        .any(|grant| !AT_ONCE.contains(&grant.as_str()))
}

/// A phrase as it is put in the tool's note: one line, no quotes, no
/// control character, at most [`MOST_TRIGGER_CHARS`].
fn phrase(text: &str) -> Option<String> {
    let text: String = text
        .trim()
        .trim_matches(['"', '\'', '“', '”', '`', '*'])
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '"' | '\\' | '“' | '”'))
        .collect();
    let text = text.trim();
    (!text.is_empty() && text.chars().count() <= MOST_TRIGGER_CHARS).then(|| text.to_owned())
}

/// The quoted phrases in `line`, `"…"` or `“…”`.
fn quoted(line: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (open, close) in [('"', '"'), ('“', '”')] {
        let mut rest = line;
        while let Some((_, after)) = rest.split_once(open) {
            let Some((inside, next)) = after.split_once(close) else {
                break;
            };
            found.extend(phrase(inside));
            rest = next;
        }
    }
    found
}

/// The trigger phrases the file names: the quoted phrases on a line that
/// says "trigger" (its front matter's too), and the items of a list under a
/// heading that says "trigger"; once each, at most [`MOST_TRIGGERS`].
pub fn triggers(card: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut under = false;
    for line in card.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_lowercase();
        if trimmed.starts_with('#') {
            under = lower.contains("trigger");
            continue;
        }
        let item = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "));
        let mut here = Vec::new();
        if lower.contains("trigger") {
            here.extend(quoted(trimmed));
        }
        if under && let Some(item) = item {
            let inner = quoted(item);
            if inner.is_empty() {
                here.extend(phrase(item));
            } else {
                here.extend(inner);
            }
        }
        for phrase in here {
            if !found.iter().any(|kept| kept.eq_ignore_ascii_case(&phrase)) {
                found.push(phrase);
            }
        }
    }
    found.truncate(MOST_TRIGGERS);
    found
}

/// `text`'s first `most` bytes, cut on a character.
fn cut(text: &str, most: usize) -> &str {
    if text.len() <= most {
        return text;
    }
    let mut end = most;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// The tool's note: the file's description, then its trigger phrases.
pub fn note(description: &str, triggers: &[String]) -> String {
    let mut note = description.trim().to_owned();
    if !triggers.is_empty() {
        let said: Vec<String> = triggers
            .iter()
            .map(|phrase| format!("'{phrase}'"))
            .collect();
        if !note.ends_with(['.', '!', '?']) {
            note.push('.');
        }
        note.push_str(&format!(
            " Call it when the person says {}, or asks for it in other words.",
            said.join(", ")
        ));
    }
    cut(&note, MOST_NOTE_BYTES).to_owned()
}

/// A bare `SKILL.md` as an agent skill: its `skill.toml` (a voice tool
/// named for it, its grants and one `do.task` step that runs
/// [`INSTRUCTIONS`]), a card of the lamp's own words whose description is
/// [`note`], and the file whole as [`INSTRUCTIONS`]. Its grants with it.
pub fn files(card: &str) -> Result<(Files, Vec<String>), Refused> {
    let card = card.replace("\r\n", "\n");
    if card.len() > MOST_BYTES {
        return Err(Refused::new(format!(
            "its SKILL.md is longer than {} KiB",
            MOST_BYTES / 1_024
        )));
    }
    let unread = || Refused::new("its SKILL.md has no name and description");
    let name = made_skill::card_name(&card).ok_or_else(unread)?;
    let description = made_skill::written_description(&card).ok_or_else(unread)?;
    let tool = name.replace('-', "_");
    if !tool.starts_with(|c: char| c.is_ascii_lowercase()) || tool.len() > 48 {
        return Err(Refused::new(
            "its name can't be a function's: it begins with a letter and is at most 48 long",
        ));
    }
    if card
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(Refused::new("its SKILL.md isn't plain text"));
    }
    if carries_secret(&card) {
        eprintln!(
            "lamp-voice: an agent skill added from a file was refused: it carries a key or token"
        );
        return Err(Refused::new(HAS_SECRET));
    }
    let grants = grants_read(&card);
    if grants
        .iter()
        .any(|grant| grant == crate::skill_task_yes::NET.0)
        && !crate::skill_task_yes::net_on()
    {
        return Err(Refused::new(NO_NET));
    }
    let task: String = cut(&description, MOST_TASK_BYTES)
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let quoted_grants: Vec<String> = grants.iter().map(|grant| format!("\"{grant}\"")).collect();
    let toml = format!(
        "kind = \"installable\"\nversion = \"1.0.0\"\nneeds = [\"voice\"]\ntool = \"{tool}\"\n\
         grants = [{}]\n\n[[do]]\ntask = {{ task = {}, run = \"{INSTRUCTIONS}\" }}\n",
        quoted_grants.join(", "),
        toml_string(task.trim()),
    );
    let note = note(&description, &triggers(&card));
    let lamps_card = format!(
        "---\nname: {name}\ndescription: \"{}\"\n---\n\nAdded from a file in the app. The lamp's agent \
         follows its instructions, kept whole in {INSTRUCTIONS}.\n",
        note.replace(['"', '\\'], "'")
    );
    Ok((
        Files {
            toml,
            card: lamps_card,
            left_out: None,
            scripts: vec![(INSTRUCTIONS.to_owned(), card)],
        },
        grants,
    ))
}

/// `text` as a TOML basic string.
fn toml_string(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for c in text.chars() {
        match c {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

/// An agent skill's `skill.toml` held to what [`files`] writes, as it is
/// kept and again as it is put on (lamp-skills' manage helper,
/// `install_made`): an installable voice tool, no trigger, `[ask]`,
/// `[[while]]` or `[permissions]`, one `do.task` step alone whose `run` is
/// [`INSTRUCTIONS`], and grants each one [`grants_read`] may give, once.
/// Its version and what it uses.
pub fn capped(text: &str) -> Result<(String, made_skill::Uses), Refused> {
    let table: toml::Table =
        toml::from_str(text).map_err(|_| Refused::new("its skill.toml can't be read"))?;
    let allowed = ["kind", "version", "needs", "tool", "grants", "do"];
    if table.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(Refused::new(
            "an agent skill added from a file is a tool and one task, nothing more",
        ));
    }
    if table.get("kind").and_then(toml::Value::as_str) != Some("installable") {
        return Err(Refused::new("it isn't an installable skill"));
    }
    if !table.get("tool").is_some_and(toml::Value::is_str) {
        return Err(Refused::new(
            "it has no tool to be asked for by, so it could never run",
        ));
    }
    let steps = table
        .get("do")
        .and_then(toml::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let [step] = steps else {
        return Err(Refused::new("an agent skill has one step, its task"));
    };
    let task = step
        .as_table()
        .filter(|step| step.len() == 1)
        .and_then(|step| step.get("task"))
        .and_then(toml::Value::as_table)
        .ok_or_else(|| Refused::new("an agent skill's one step is its task"))?;
    if task.get("run").and_then(toml::Value::as_str) != Some(INSTRUCTIONS)
        || task.keys().any(|key| key != "task" && key != "run")
    {
        return Err(Refused::new("an agent skill's task runs its file alone"));
    }
    let mut uses = made_skill::Uses::default();
    let mut seen: Vec<&str> = Vec::new();
    for grant in table
        .get("grants")
        .and_then(toml::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let grant = grant.as_str().unwrap_or_default();
        let known = [
            "light",
            "speak",
            "sound",
            "camera",
            crate::skill_task_yes::NET.0,
        ];
        if !known.contains(&grant) || seen.contains(&grant) {
            return Err(Refused::new(
                "an agent skill's grants are the camera, the light, its voice, sounds and the internet",
            ));
        }
        if grant == crate::skill_task_yes::NET.0 && !crate::skill_task_yes::net_on() {
            return Err(Refused::new(NO_NET));
        }
        seen.push(grant);
        match grant {
            "light" => uses.light = true,
            "speak" => uses.speak = true,
            "camera" => uses.camera = true,
            _ => {}
        }
    }
    let version = table
        .get("version")
        .and_then(toml::Value::as_str)
        .unwrap_or("1.0.0")
        .to_owned();
    Ok((version, uses))
}

/// An agent skill's package files held again as it is put on (lamp-skills'
/// manage helper): exactly `skill.toml`, `SKILL.md` and [`INSTRUCTIONS`],
/// the file at most [`MOST_BYTES`] of plain text.
pub fn package_files_ok<'a>(
    files: impl IntoIterator<Item = (&'a str, &'a [u8])>,
) -> Result<(), Refused> {
    let mut instructions = false;
    for (path, bytes) in files {
        match path {
            "skill.toml" | "SKILL.md" => {}
            INSTRUCTIONS if bytes.len() <= MOST_BYTES && std::str::from_utf8(bytes).is_ok() => {
                instructions = true;
            }
            _ => {
                return Err(Refused::new(
                    "an agent skill carries its file and nothing else",
                ));
            }
        }
    }
    if instructions {
        Ok(())
    } else {
        Err(Refused::new("an agent skill carries its file"))
    }
}

/// What the lamp says once an agent skill is on: that it was added from a
/// file, that the lamp's agent runs it, its grants in plain words, and
/// whether its first run asks the yes.
pub fn on_line(made: &Made, grants: &[String]) -> String {
    let said = made_skill::spoken_name(&made.name);
    let uses = crate::skill_task_yes::uses_said(grants);
    let then = if asks_yes(grants) {
        " The first time it runs, I'll ask you to agree."
    } else {
        ""
    };
    format!(
        "I added {said} from a file, not from the Skills Store, and it's on: only when you ask for \
         it, the lamp's agent follows the file, and {uses}.{then}"
    )
}

/// The grants an agent skill's kept `skill.toml` names, in its package's
/// index entry ([`made_skill::agent_grants`]).
pub fn grants_of_toml(toml: &str) -> Vec<String> {
    toml::from_str::<toml::Table>(toml)
        .ok()
        .and_then(|table| table.get("grants").cloned())
        .and_then(|grants| grants.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|grant| grant.as_str().map(str::to_owned))
        .collect()
}

/// After an agent skill `made` went on: with grants only of [`AT_ONCE`],
/// its task's yes recorded by lamp-control at once (`record`, the manage
/// helper reading its folder as it went on); then the line said
/// ([`on_line`]). A record that fails leaves it to ask the yes at its first
/// run, and the line says so.
pub fn went_on(
    made: &Made,
    grants: &[String],
    record: impl FnOnce(&str) -> Result<(), String>,
) -> String {
    if asks_yes(grants) {
        return on_line(made, grants);
    }
    match record(&made.name) {
        Ok(()) => {
            eprintln!(
                "lamp-voice: an agent skill added from a file is on, its task agreed at once ({} grant(s))",
                grants.len()
            );
            on_line(made, grants)
        }
        Err(why) => {
            eprintln!(
                "lamp-voice: an agent skill added from a file: its task's yes not kept: {why}"
            );
            format!(
                "{} The first time it runs, I'll ask you to agree.",
                on_line(made, grants)
            )
        }
    }
}

/// [`went_on`]'s record on a lamp: the skill read by the manage helper at
/// `manage` (`task_asks`) and its yes recorded by lamp-control at
/// `control` for its folder and grants as they are.
pub fn record_by(manage: &Path, control: &Path, name: &str) -> Result<(), String> {
    let asks = crate::skill_task_yes::asks_by(manage, control, name)?
        .ok_or("the lamp's skills don't have its task")?;
    if asks.approved {
        return Ok(());
    }
    crate::skill_task_yes::approve_by(manage, control, &asks.at_once()).map(|_| ())
}

/// The kept agent skill whose tool is `tool`, on the lamp: its name, from
/// the made skills' index in `directory`.
pub fn agent_named(directory: &Path, tool: &str) -> Option<String> {
    made_skill::entries(directory)
        .into_iter()
        .filter(|entry| entry["agent"] == true && entry["on"] == true)
        .filter_map(|entry| entry["name"].as_str().map(str::to_owned))
        .find(|name| name.replace('-', "_") == tool)
}

/// The model's call of `tool`, before it runs: for an agent skill whose
/// task's yes isn't kept, its say-back and the wait for the spoken yes
/// ([`crate::skill_task_yes::read_back`]), and nothing runs; none when it
/// isn't one, or its yes is kept, so the call runs. `asks` reads the skill
/// on the lamp, with lamp-control's word.
pub fn first_run_with(
    directory: &Path,
    tool: &str,
    asking: crate::make_skill_local::Asking,
    pushed: &Path,
    mut asks: impl FnMut(&str) -> Result<Option<crate::skill_task_yes::TaskAsks>, String>,
    now: std::time::Instant,
) -> Option<Value> {
    if !on() || !crate::skill_task_yes::on() {
        return None;
    }
    let name = agent_named(directory, tool)?;
    if let Ok(Some(task)) = asks(&name)
        && task.approved
    {
        return None;
    }
    eprintln!(
        "lamp-voice: request {}: an agent skill added from a file asks its yes before its first run",
        asking.request
    );
    let told = crate::skill_task_yes::read_back(&name, asking, pushed, asks, now)?;
    (told["approved"] != true).then_some(told)
}

/// [`first_run_with`] on a lamp: the product's folders and sockets.
pub fn first_run(
    tool: &str,
    request: u64,
    heard: bool,
    lease: Option<&crate::memory_scope::ScopeLease>,
) -> Option<Value> {
    if !on() || !crate::skill_task_yes::on() {
        return None;
    }
    let keeping = Keeping::when(true)?;
    let control = Path::new(crate::skill_task_yes::CONTROL_SOCKET);
    agent_named(&keeping.directory, tool)?;
    let asking = crate::make_skill_local::Asking {
        request,
        heard,
        owner_hears: crate::services::owner_hears(request, heard, lease),
        sends_here: crate::services::sends_here(),
    };
    first_run_with(
        &keeping.directory,
        tool,
        asking,
        &made_skill::pushed(&keeping.installed),
        |asked| crate::skill_task_yes::asks_by(&keeping.manage, control, asked),
        std::time::Instant::now(),
    )
}

/// What the model is told once an agent skill's yes is kept: run it now,
/// as the person asked.
pub fn approved_next(tool: &str) -> String {
    format!("It is agreed. Call {tool} now, once, as the person asked; say nothing before it.")
}

#[cfg(test)]
mod tests;
