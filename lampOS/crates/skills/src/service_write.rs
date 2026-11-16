//! A made skill's write to one of the owner's linked services (#6066, the
//! owner's lift of rule (b), 10 Nov): "when I arrive, mail my boss that I'm
//! in". Off unless `LAMP_SKILL_WRITES=on` ([`ENVIRONMENT`]), here and in
//! `lamp-services` (`lamp_services::skill_writes`).
//!
//! - **The step**: `write = { service = "<code>", action = "<action>",
//!   ... }`, a skill's only step, with a trigger and no tool, and
//!   `[permissions] services = { <code> = "write" }`. One of
//!   `lamp_services::skill_writes::KINDS`: a mail, a calendar event, an
//!   issue, a task or a comment; never a public post, a draft or an MCP
//!   tool call. **Every field is the file's own**, checked as it loads
//!   (`lamp_services::skill_writes::check`); nothing is filled at run time.
//! - **Loads only with the switch on**, and with its permission.
//! - **Who sends**: this program, on `lamp-services`' tools socket, with the
//!   skill's name and the step's exact content ([`WriteStep::content`]).
//!   `lamp-services` sends it only if the owner said yes to that content
//!   for that skill under the same link, within its bounds. Putting the
//!   skill on approves nothing: until the owner's yes it is refused,
//!   "waiting for the owner's yes".
//! - **Stopped**: a refusal that says its link is gone or another's stops
//!   the skill's writes until this program starts again ([`stop`]).
//! - **The journal**: the outcome only, never a recipient or a text.
//! - **Through the lamp's agent** (the owner's ruling, 13 Nov: everything
//!   that touches a linked service runs through it): with
//!   `LAMP_SKILL_WRITE_AGENT=on` ([`AGENT_ENVIRONMENT`], on in the
//!   product's four units) and `LAMP_AGENT_SERVICES=on` ([`agent`]), this program never
//!   calls `lamp-services` for the write: it hands it to `lamp-tasks` as a
//!   skill's run on the `do.task` socket ([`hand`]), with its one grant
//!   (`<code>-write`, [`WriteStep::grant`]) and its exact content, which
//!   the agent sends through `lamp-services mcp`'s `service_write`.
//!   `lamp-control` mints the run's token for that one grant
//!   ([`fixed_write_of`]), and `lamp-services` sends it only as the owner's
//!   kept yes has it, within the same bounds, once a run. An agent that
//!   couldn't take it (lamp-tasks not there or not reached, or refusing it
//!   before its agent began: Harness signed out, no Hermes) has it sent as
//!   above instead, with one journal line why (the first builder's ruling
//!   on #6823); once the run may have begun, never ([`hand`]). Off, the
//!   write goes as above, unchanged.
use lamp_services::skill_writes;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Mutex;

/// The switch, here and in `lamp-services`. `on` only.
pub const ENVIRONMENT: &str = skill_writes::ENVIRONMENT;

#[cfg(test)]
thread_local! {
    /// The switch as a test of this thread sets it.
    static ON_HERE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Whether a skill's writes are on in this process.
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

/// A skill's fixed write run through the lamp's agent: `on` only, here, in
/// `lamp-tasks`, `lamp-control` and `lamp-services`.
pub const AGENT_ENVIRONMENT: &str = skill_writes::AGENT_ENVIRONMENT;

#[cfg(test)]
thread_local! {
    /// [`AGENT_ENVIRONMENT`] as a test of this thread sets it.
    static AGENT_HERE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Whether [`AGENT_ENVIRONMENT`] is on in this process.
pub fn agent_on() -> bool {
    #[cfg(test)]
    if let Some(on) = AGENT_HERE.with(std::cell::Cell::get) {
        return on;
    }
    std::env::var(AGENT_ENVIRONMENT).ok().as_deref() == Some("on")
}

/// [`agent_on`] for this test's thread alone.
#[cfg(test)]
pub fn set_agent_here(on: Option<bool>) {
    AGENT_HERE.with(|here| here.set(on));
}

/// Whether a fixed write goes through the lamp's agent: [`agent_on`], and
/// the agent's connectors on (`LAMP_AGENT_SERVICES`,
/// [`crate::task::agent_services_on`]), without which its run has no
/// `service_write`.
pub fn agent() -> bool {
    agent_on() && crate::task::agent_services_on()
}

/// What a write step is told as it loads with the switch off.
pub const OFF: &str = "write: a skill's writes are off on this lamp (LAMP_SKILL_WRITES is not on)";

/// `write = { … }`, as the folder writes it: `service`, `action` and the
/// action's own fields (`lamp_services::skill_writes::KINDS`).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriteFile {
    service: String,
    action: String,
    to: Option<Vec<String>>,
    cc: Option<Vec<String>>,
    subject: Option<String>,
    text: Option<String>,
    title: Option<String>,
    start: Option<String>,
    end: Option<String>,
    calendar: Option<String>,
    attendees: Option<Vec<String>>,
    repo: Option<String>,
    number: Option<u64>,
    team_id: Option<String>,
    project: Option<String>,
    workspace: Option<String>,
    id: Option<String>,
}

/// A write step as loaded: its content, checked.
#[derive(Clone, Debug, PartialEq)]
pub struct WriteStep {
    /// The service's code in `lamp-services`' catalogue.
    pub service: String,
    /// What it does there.
    pub action: String,
    content: Value,
}

impl WriteFile {
    /// The step, or why the folder's words are not one.
    pub fn step(self) -> Result<WriteStep, String> {
        let mut fields = Map::new();
        let strings = [
            ("subject", self.subject),
            ("text", self.text),
            ("title", self.title),
            ("start", self.start),
            ("end", self.end),
            ("calendar", self.calendar),
            ("repo", self.repo),
            ("team_id", self.team_id),
            ("project", self.project),
            ("workspace", self.workspace),
            ("id", self.id),
        ];
        for (key, value) in strings {
            if let Some(value) = value {
                fields.insert(key.into(), json!(value));
            }
        }
        for (key, value) in [
            ("to", self.to),
            ("cc", self.cc),
            ("attendees", self.attendees),
        ] {
            if let Some(value) = value {
                fields.insert(key.into(), json!(value));
            }
        }
        if let Some(number) = self.number {
            fields.insert("number".into(), json!(number));
        }
        let content = skill_writes::content(&self.service, &self.action, &fields);
        skill_writes::check(&content).map_err(|why| format!("write: {why}"))?;
        Ok(WriteStep {
            service: self.service,
            action: self.action,
            content,
        })
    }
}

impl WriteStep {
    /// The exact content `lamp-services` checks against the owner's yes.
    pub fn content(&self) -> &Value {
        &self.content
    }

    /// The step as its skill names it.
    pub fn named(&self) -> Value {
        json!({"service": self.service, "action": self.action})
    }

    /// The one grant its run holds through the lamp's agent: `<code>-write`
    /// as lamp-control names it ([`skill_writes::run_grant`]).
    pub fn grant(&self) -> String {
        skill_writes::run_grant(&self.service)
    }

    /// The line `lamp-tasks` takes for `skill`'s write, its folder `folder`,
    /// run through the lamp's agent: `{"op": "skill_write", "skill",
    /// "folder", "grants": [<the one>], "write": <the exact content>}`
    /// (`lamp_tasks::skill_task::WriteHandOver`).
    pub fn hand_line(&self, skill: &str, folder: &Path) -> String {
        json!({"op": "skill_write", "skill": skill,
               "folder": folder.to_string_lossy(),
               "grants": [self.grant()], "write": self.content})
        .to_string()
    }

    /// The tools socket's line for `skill`'s write.
    pub fn line(&self, skill: &str) -> String {
        json!({"op": "call", "name": skill_writes::FUNCTION,
               "args": {"skill": skill, "write": self.content}})
        .to_string()
    }
}

/// The most bytes of a `skill.toml` read for its fixed write.
const MOST_FILE_BYTES: u64 = 64 * 1024;

/// The fixed write of the skill in `folder`, read from its `skill.toml`
/// alone, whatever this process's switches: its only `[[do]]` step a
/// `write` and nothing beside it, its content checked ([`WriteFile::step`]);
/// none otherwise, or for a file that is a link. lamp-control names a fixed
/// write's run's one grant from it (`lamp_control::task_yes::KeptYes`),
/// as `lamp-services` holds its send to the owner's kept yes.
pub fn fixed_write_of(folder: &Path) -> Option<WriteStep> {
    use std::io::Read as _;
    #[derive(Deserialize)]
    struct Steps {
        #[serde(rename = "do", default)]
        steps: Vec<toml::Table>,
    }
    let path = folder.join("skill.toml");
    if !std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_file()) {
        return None;
    }
    let mut text = String::new();
    std::fs::File::open(&path)
        .ok()?
        .take(MOST_FILE_BYTES + 1)
        .read_to_string(&mut text)
        .ok()?;
    if text.len() as u64 > MOST_FILE_BYTES {
        return None;
    }
    let file: Steps = toml::from_str(&text).ok()?;
    let [step] = file.steps.as_slice() else {
        return None;
    };
    if step.len() != 1 {
        return None;
    }
    step.get("write")?
        .clone()
        .try_into::<WriteFile>()
        .ok()?
        .step()
        .ok()
}

/// Why the lamp's agent didn't run a fixed write to its end.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unrun {
    /// lamp-tasks not reached, or it refused the run before its agent
    /// began ([`NOT_TAKEN`]): nothing of it can have been sent.
    NotTaken(String),
    /// Its agent may have begun, or what became of the run is not known: it
    /// may have sent, so nothing more is tried.
    Unknown(String),
}

/// lamp-tasks' refusals that come before a run's agent begins: off, a line
/// it doesn't take, no token, another run going, or its agent not started
/// (`lamp_tasks::skill_task::NOT_STARTED`: Harness signed out or not set
/// up, no Hermes target, a launch Harness never sent). `run_failed` is
/// none of them: the run had begun.
pub const NOT_TAKEN: [&str; 6] = [
    "off",
    "bad_request",
    "too_long",
    "no_token",
    "busy",
    "not_started",
];

/// lamp-tasks' answer to a fixed write's line, read when its run ended:
/// what the run became ([`crate::task::answered`]), or why not, as
/// [`Unrun`]. An answer that can't be read is [`Unrun::Unknown`].
pub fn agent_answered(answer: &str) -> Result<String, Unrun> {
    let code = serde_json::from_str::<Value>(answer.trim())
        .ok()
        .and_then(|answer| answer["error"].as_str().map(str::to_owned));
    crate::task::answered(answer).map_err(|why| match code {
        Some(code) if NOT_TAKEN.contains(&code.as_str()) => Unrun::NotTaken(why),
        _ => Unrun::Unknown(why),
    })
}

/// `skill`'s write handed to `lamp-tasks` (its socket, `--tasks-socket`)
/// as a run on the lamp's agent ([`agent`]), from its `folder`, through
/// `send` (the socket and the line; lamp-tasks' answer when the run ends,
/// [`agent_answered`]). `lamp-services` is never called for it here, but
/// when the agent couldn't take it (no folder, no socket, lamp-tasks not
/// reached, or [`Unrun::NotTaken`]): then it is sent straight, as with the
/// switch off, through `direct` (with every check of `lamp-services'`), and
/// `journal` is told the one line why (the first builder's ruling on
/// #6823: no lamp loses a send it has today). Once the run may have begun
/// ([`Unrun::Unknown`], or a run that ended, its send refused for its
/// content or a cap among it), nothing more: never two sends. The
/// journal's words for the run.
pub fn hand(
    skill: &str,
    write: &WriteStep,
    folder: Option<&Path>,
    socket: Option<&Path>,
    send: impl FnOnce(&Path, &str) -> Result<String, Unrun>,
    direct: impl FnOnce() -> String,
    journal: impl FnOnce(&str),
) -> String {
    let why = match (folder, socket) {
        (None, _) => "its folder is not known".to_owned(),
        (_, None) => "no lamp-tasks hand-over on this lamp (--tasks-socket)".to_owned(),
        (Some(folder), Some(socket)) => match send(socket, &write.hand_line(skill, folder)) {
            Ok(said) => return format!("handed to the lamp's agent: {said}"),
            Err(Unrun::Unknown(why)) => {
                return format!("the lamp's agent's run failed, so nothing more is tried: {why}");
            }
            Err(Unrun::NotTaken(why)) => why,
        },
    };
    journal(&format!(
        "the lamp's agent couldn't take its write ({why}), so it goes straight to lamp-services"
    ));
    direct()
}

/// The skills whose writes are stopped until this program starts again.
static STOPPED: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// Stop `skill`'s writes.
pub fn stop(skill: &str) {
    STOPPED
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .insert(skill.to_owned());
}

/// Whether `skill`'s writes are stopped.
pub fn stopped(skill: &str) -> bool {
    STOPPED
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .contains(skill)
}

/// The journal's words for `lamp-services`' answer to a write, and
/// whether the skill's writes stop with it: "sent to gmail", or the
/// refusal (which names no recipient or text).
pub fn outcome(answer: Result<Value, String>, service: &str) -> (String, bool) {
    match answer {
        Ok(_) => (format!("sent to {service}"), false),
        Err(why) => {
            let stops = why.starts_with(skill_writes::STOPPED);
            (why, stops)
        }
    }
}

/// Whether a write's outcome is `lamp-services`' "not sent: waiting for
/// the owner's yes": nothing was made, so its run spends nothing
/// (`Engine::unwritten`).
pub fn waiting(outcome: &str) -> bool {
    outcome == skill_writes::WAITING
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(text: &str) -> Result<WriteStep, String> {
        toml::from_str::<WriteFile>(text)
            .map_err(|error| error.to_string())
            .and_then(WriteFile::step)
    }

    #[test]
    fn a_write_is_fixed_and_checked_as_it_loads() {
        let step = file(
            "service = \"gmail\"\naction = \"send_mail\"\nto = [\"boss@work.test\"]\nsubject = \"In\"\ntext = \"I'm at my desk.\"",
        )
        .unwrap();
        assert_eq!(
            step.content(),
            &json!({"service":"gmail","action":"send_mail","to":["boss@work.test"],"subject":"In","text":"I'm at my desk."})
        );
        assert_eq!(
            skill_writes::hash(step.content()).len(),
            64,
            "hashed as lamp-services does"
        );
        for (bad, why) in [
            (
                "service = \"gmail\"\naction = \"send_mail\"\nto = [\"x\"]\nsubject = \"s\"\ntext = \"t\"",
                "not an email address",
            ),
            (
                "service = \"gmail\"\naction = \"send_mail\"\nsubject = \"s\"\ntext = \"t\"",
                "at least one address",
            ),
            (
                "service = \"facebook\"\naction = \"page_post\"\ntext = \"t\"",
                "a skill writes only",
            ),
            (
                "service = \"gmail\"\naction = \"send_mail\"\nto = [\"a@b.test\"]\nsubject = \"s\"\ntext = \"t\"\nsend_id = \"x\"",
                "unknown field",
            ),
            (
                "service = \"gmail\"\naction = \"send_mail\"\nto = [\"a@b.test\"]\nsubject = \"s\"\ntext = \"t\"\ntitle = \"x\"",
                "takes no title",
            ),
        ] {
            let refused = file(bad).unwrap_err();
            assert!(refused.contains(why), "{bad}: {refused}");
        }
    }

    fn arrival() -> WriteStep {
        file("service = \"gmail-app\"\naction = \"send_mail\"\nto = [\"boss@work.test\"]\nsubject = \"In\"\ntext = \"I'm at my desk.\"").unwrap()
    }

    /// LAMP_SKILL_WRITE_AGENT (the owner's ruling, 13 Nov): on only with
    /// exactly `on`, and with the agent's connectors on too.
    #[test]
    fn a_write_goes_through_the_agent_only_with_both_switches() {
        for (write_agent, services, through) in [
            (Some(true), Some(true), true),
            (Some(true), Some(false), false),
            (Some(false), Some(true), false),
            (None, Some(true), false),
        ] {
            set_agent_here(write_agent);
            crate::task::set_agent_services_here(services);
            assert_eq!(agent(), through, "{write_agent:?} {services:?}");
        }
        set_agent_here(None);
        crate::task::set_agent_services_here(None);
        assert_eq!(AGENT_ENVIRONMENT, "LAMP_SKILL_WRITE_AGENT");
    }

    /// A hand-over whose sends are counted: the agent's answer `answer`,
    /// and how often the straight send and the journal were reached.
    fn handed(
        folder: Option<&Path>,
        socket: Option<&Path>,
        answer: Result<String, Unrun>,
    ) -> (String, Vec<String>, usize, Vec<String>) {
        let mut lines = Vec::new();
        let mut direct = 0;
        let mut journal = Vec::new();
        let said = hand(
            "arrive-mail",
            &arrival(),
            folder,
            socket,
            |_, line| {
                lines.push(line.to_owned());
                answer
            },
            || {
                direct += 1;
                "sent to gmail-app".into()
            },
            |line| journal.push(line.to_owned()),
        );
        (said, lines, direct, journal)
    }

    /// Through the agent, the write goes to lamp-tasks as a skill's run with
    /// its one grant and its exact content, and lamp-services is not called:
    /// the straight send is never reached once the run is taken.
    #[test]
    fn a_fixed_write_is_handed_to_lamp_tasks_with_its_one_grant() {
        let folder = Path::new("/var/lib/lampOS/skills/arrive-mail");
        let socket = Path::new("/run/lampos-tasks/skill-task.sock");
        let (said, lines, direct, journal) = handed(
            Some(folder),
            Some(socket),
            agent_answered(r#"{"ok":true,"id":7}"#),
        );
        assert_eq!(said, "handed to the lamp's agent: its run ended, task 7");
        assert_eq!((direct, journal.len()), (0, 0));
        let line: Value = serde_json::from_str(&lines[0]).unwrap();
        assert_eq!(
            line,
            json!({"op": "skill_write", "skill": "arrive-mail",
                   "folder": "/var/lib/lampOS/skills/arrive-mail",
                   "grants": ["gmail-write"], "write": arrival().content()})
        );
    }

    /// #6823: an agent that couldn't take the write (lamp-tasks refused it
    /// before its agent began, wasn't reached, or isn't there) has it sent
    /// straight, once, with one journal line why.
    #[test]
    fn a_write_the_agent_couldnt_take_is_sent_straight_once() {
        let folder = Path::new("/var/lib/lampOS/skills/arrive-mail");
        let socket = Path::new("/run/lampos-tasks/skill-task.sock");
        let harness = r#"{"ok":false,"error":"not_started","message":"Harness isn't set up on this lamp yet, so a skill's run can't be started"}"#;
        for answer in [
            agent_answered(harness),
            agent_answered(r#"{"ok":false,"error":"busy","message":"one runs at a time"}"#),
            agent_answered(r#"{"ok":false,"error":"no_token","message":"no yes"}"#),
            agent_answered(r#"{"ok":false,"error":"off","message":"off"}"#),
            Err(Unrun::NotTaken(
                "lamp-tasks couldn't be reached: gone".into(),
            )),
        ] {
            let (said, _, direct, journal) = handed(Some(folder), Some(socket), answer);
            assert_eq!((said.as_str(), direct), ("sent to gmail-app", 1));
            assert_eq!(journal.len(), 1);
            assert!(
                journal[0].contains("couldn't take its write"),
                "{journal:?}"
            );
        }
        let (_, _, direct, journal) = handed(Some(folder), Some(socket), agent_answered(harness));
        assert!(journal[0].contains("Harness isn't set up"), "{journal:?}");
        assert_eq!(direct, 1);
        for (folder, socket) in [(None, Some(socket)), (Some(folder), None)] {
            let (said, lines, direct, journal) = handed(folder, socket, Ok("never".into()));
            assert_eq!(
                (said.as_str(), direct, journal.len()),
                ("sent to gmail-app", 1, 1)
            );
            assert!(lines.is_empty());
        }
    }

    /// #6823: never two sends. A run that ended (its send refused by
    /// lamp-services for its content or a cap among it), one that began and
    /// failed, or an answer that can't be read: nothing is sent straight.
    #[test]
    fn no_straight_send_once_the_agents_run_began() {
        let folder = Path::new("/var/lib/lampOS/skills/arrive-mail");
        let socket = Path::new("/run/lampos-tasks/skill-task.sock");
        for answer in [
            // The run ended: its send refused for its content or the cap.
            agent_answered(r#"{"ok":true,"id":8}"#),
            agent_answered(
                r#"{"ok":false,"error":"run_failed","message":"the run was ended at its deadline, 840 s"}"#,
            ),
            agent_answered("not json"),
            Err(Unrun::Unknown(
                "lamp-tasks' answer never came: timed out".into(),
            )),
        ] {
            let (said, _, direct, journal) = handed(Some(folder), Some(socket), answer);
            assert_eq!((direct, journal.len()), (0, 0), "{said}");
        }
    }

    /// lamp-control's mint reads a skill's fixed write from its file alone:
    /// a lone `write` step, checked; anything else, none.
    #[test]
    fn a_fixed_write_is_read_from_a_lone_write_step() {
        let dir = tempfile::tempdir().unwrap();
        let write = |toml: &str| {
            std::fs::write(dir.path().join("skill.toml"), toml).unwrap();
            fixed_write_of(dir.path())
        };
        let lone = "version = \"1.0.0\"\n[permissions]\nservices = { gmail-app = \"write\" }\n[[do]]\nwrite = { service = \"gmail-app\", action = \"send_mail\", to = [\"boss@work.test\"], subject = \"In\", text = \"I'm at my desk.\" }\n";
        assert_eq!(write(lone), Some(arrival()));
        assert_eq!(write(lone).unwrap().grant(), "gmail-write");
        // A second step, a step beside it, or content no skill writes: none.
        assert_eq!(write(&format!("{lone}[[do]]\nsay = \"hi\"\n")), None);
        assert_eq!(
            write(&lone.replace("write = {", "say = \"hi\"\nwrite = {")),
            None
        );
        assert_eq!(write(&lone.replace("boss@work.test", "boss")), None);
        assert_eq!(write("version = \"1.0.0\"\n"), None);
    }

    #[test]
    fn a_stopping_refusal_stops_the_skill() {
        let (said, stops) = outcome(
            Err(format!("{}the Gmail link changed", skill_writes::STOPPED)),
            "gmail",
        );
        assert!(stops && said.contains("link changed"));
        let (said, stops) = outcome(Err(skill_writes::WAITING.into()), "gmail");
        assert!(!stops && said.contains("owner's yes"));
        // Waiting for the yes: its run gives back what it spent.
        assert!(waiting(&said));
        assert!(!waiting(skill_writes::NOT_APPROVED));
        assert!(!waiting("sent to gmail"));
        assert_eq!(outcome(Ok(json!({})), "gmail").0, "sent to gmail");
        assert!(!stopped("write-test-skill"));
        stop("write-test-skill");
        assert!(stopped("write-test-skill"));
    }
}
