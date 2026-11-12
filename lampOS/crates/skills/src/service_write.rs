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
use lamp_services::skill_writes;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
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

    /// The tools socket's line for `skill`'s write.
    pub fn line(&self, skill: &str) -> String {
        json!({"op": "call", "name": skill_writes::FUNCTION,
               "args": {"skill": skill, "write": self.content}})
        .to_string()
    }
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
