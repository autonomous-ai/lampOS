//! What a made skill may read of a linked service, and what of it is said
//! (#5288): one rule, keyed by service, for `lamp-skills`' read step (what
//! it takes and the lines it says) and the voice's made skills (the brief
//! and the spoken yes). A later ruling for a service changes [`SPOKEN`]
//! alone.
//!
//! - **Which services**: every one in [`catalog::SERVICES`] with a read of
//!   a fixed shape, and none that [`reads::never_read`] refuses
//!   (`gmail-app`, which only sends). The MCP services and Ahrefs have only
//!   free-form reads, so none yet.
//! - **Never a whole text**: a read that gives one item's whole text (a
//!   mail, a file's text, a page's blocks, comments) is never a skill's
//!   ([`WHOLE_TEXTS`]).
//! - **Never a free-form read** (Thirteen's P2 on #5303): a GraphQL query,
//!   any API path or an MCP tool's call can put any text where a title
//!   goes, so a skill reads only fixed lists ([`FREE_FORM`]).
//!   `lamp-services` holds both at its door ([`skill_read_refused`]).
//! - **What is said**: the count, then one short line for each item: a
//!   mail's sender and subject, an event's time and title, and for every
//!   other service the item's title or name ([`TITLE_KEYS`]), never a body,
//!   a message's text, a file's content or a comment.
use crate::{catalog, reads};
use serde_json::Value;

/// How a read's items are said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spoken {
    /// The first builder's ruling for mail: the count, then each mail's
    /// sender and subject; never its text.
    Mail,
    /// The ruling for a calendar: the count, then each event's time and
    /// title.
    Calendar,
    /// The default: the count, then each item's title or name, the first
    /// of these keys an item has (as text, or as an object's own title or
    /// name). An item with none is counted and not said.
    Titles(&'static [&'static str]),
}

/// The keys an item's title or name is read from, by default.
pub const TITLE_KEYS: &[&str] = &["title", "name", "subject", "full_name"];

/// Every rule but the default, by service and, when it names one, what is
/// read. Notion's items come to a skill as their title property alone
/// ([`items`]), so the default says them.
pub const SPOKEN: &[(&str, Option<&str>, Spoken)] = &[
    ("gmail", None, Spoken::Mail),
    ("google_calendar", Some("events"), Spoken::Calendar),
];

/// Reads whose shape the asker writes, never a made skill's: a GraphQL
/// query (GitHub, Linear), any path on the allow-list (`get`, Ahrefs) and
/// an MCP tool's call.
pub const FREE_FORM: &[&str] = &["query", "get", "call"];

/// Reads that give an item's whole text, never a made skill's: a mail, a
/// Drive file's text, Figma's comments.
pub const WHOLE_TEXTS: &[(&str, &str)] = &[
    ("gmail", "message"),
    ("google_drive", "text"),
    ("figma", "comments"),
    ("figma-api", "comments"),
];

/// How `what` of `code` is said.
pub fn rule(code: &str, what: &str) -> Spoken {
    SPOKEN
        .iter()
        .find(|(service, only, _)| *service == code && only.is_none_or(|only| only == what))
        .map_or(Spoken::Titles(TITLE_KEYS), |(_, _, spoken)| *spoken)
}

/// Whether `what` of `code` gives a whole text, which a skill never reads.
pub fn whole_text(code: &str, what: &str) -> bool {
    WHOLE_TEXTS.contains(&(code, what))
}

/// Whether `what` is a read whose shape the asker writes ([`FREE_FORM`]).
pub fn free_form(what: &str) -> bool {
    FREE_FORM.contains(&what)
}

/// Why a made skill can't read `code`, or None when it can: a service the
/// lamp knows, not one that only sends, with a read of a fixed shape.
pub fn not_readable(code: &str) -> Option<String> {
    let Some(service) = catalog::service(code) else {
        return Some(format!("{code:?} is not a service the lamp knows"));
    };
    if reads::never_read(code).is_some() {
        return Some(format!(
            "{code:?} only sends mail, so it has nothing to read"
        ));
    }
    if service.mcp.is_some() {
        return Some(format!(
            "{code:?} is read only through its own tools, which a made skill can't read yet"
        ));
    }
    if service.reads.is_empty() {
        return Some(format!("{code:?} has nothing to read"));
    }
    whats(code)
        .is_empty()
        .then(|| format!("{code:?} has only free-form reads, which a made skill can't read yet"))
}

/// Why a made skill may not read `what` of `code`, or None when it may: a
/// readable service, one of its fixed reads, never a whole text or a
/// free-form read. `lamp-skills` and the voice hold a skill to it, and
/// `lamp-services` holds every call from `lamp-skills` to it.
pub fn skill_read_refused(code: &str, what: &str) -> Option<String> {
    if let Some(why) = not_readable(code) {
        return Some(why);
    }
    if whole_text(code, what) {
        return Some(format!(
            "{code}'s {what} gives a whole text, which a made skill never reads"
        ));
    }
    if free_form(what) {
        return Some(format!(
            "{code}'s {what} is free-form, which a made skill never reads"
        ));
    }
    let fixed: Vec<String> = whats(code)
        .iter()
        .filter_map(|what| what.split_whitespace().next().map(str::to_owned))
        .collect();
    (!fixed.iter().any(|listed| listed == what))
        .then(|| format!("{code} reads {}, not {what:?}", fixed.join(", ")))
}

/// Every service a made skill may read, in the catalogue's order.
pub fn readable() -> Vec<&'static str> {
    catalog::SERVICES
        .iter()
        .map(|service| service.code)
        .filter(|code| not_readable(code).is_none())
        .collect()
}

/// The service's name, as a person says it ("Google Calendar").
pub fn name(code: &str) -> &str {
    catalog::service(code).map_or(code, |service| service.name)
}

/// What a skill may read of `code`, as the voice's brief lists it: the
/// service's reads but its whole texts and free-form reads, each with the
/// keys it takes. None for an MCP service, whose reads are all calls.
pub fn whats(code: &str) -> Vec<String> {
    let mut whats: Vec<String> = reads::WHATS
        .iter()
        .find(|(service, _)| *service == code)
        .map(|(_, whats)| split_whats(whats))
        .unwrap_or_default();
    whats.retain(|what| {
        let word = what.split_whitespace().next().unwrap_or_default();
        !whole_text(code, word) && !free_form(word)
    });
    whats
}

/// `reads::WHATS`' line split at its top-level commas.
fn split_whats(line: &str) -> Vec<String> {
    let (mut out, mut part, mut depth) = (Vec::new(), String::new(), 0usize);
    for c in line.chars() {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                out.push(part.trim().to_owned());
                part.clear();
                continue;
            }
            _ => {}
        }
        part.push(c);
    }
    if !part.trim().is_empty() {
        out.push(part.trim().to_owned());
    }
    out
}

/// What a made skill's spoken yes says it reads: the service, what, and
/// what of it is said.
pub fn plainly(code: &str, what: &str, day: Option<&str>) -> String {
    let name = name(code);
    match rule(code, what) {
        Spoken::Calendar => format!(
            "reads you {}'s {name}: how many events, then each time and title",
            day.unwrap_or("today")
        ),
        Spoken::Mail if what == "recent" => format!(
            "reads you your latest {name}: how many, then each sender and subject, never the text"
        ),
        Spoken::Mail => format!(
            "reads you {name}'s {what}: how many, then each sender and subject, never the text"
        ),
        Spoken::Titles(_) => {
            format!("reads you {name}'s {what}: how many, then each name, never the text")
        }
    }
}

/// The most characters of a title, a sender or a subject said.
pub const MOST_SAID_CHARS: usize = 100;

/// The lines said of a read's quoted answer (`{"quoted": [...]}`): the
/// count, then one line for each item. An item left out (a security mail
/// or message, said only as a string) is neither said nor counted. `day`
/// is a calendar read's "today" or "tomorrow".
pub fn lines(code: &str, what: &str, day: Option<&str>, answer: &Value) -> Vec<String> {
    let items: Vec<&Value> = answer["quoted"]
        .as_array()
        .map(|items| items.iter().filter(|item| item.is_object()).collect())
        .unwrap_or_default();
    let n = items.len();
    let name = name(code);
    let rule = rule(code, what);
    let count = match rule {
        Spoken::Calendar => {
            let day = day.unwrap_or("today");
            match n {
                0 => format!("Nothing is on your calendar {day}."),
                1 => format!("You have one event {day}."),
                n => format!("You have {n} events {day}."),
            }
        }
        Spoken::Mail => match n {
            0 => "You have no recent mail.".into(),
            1 => "You have one recent mail.".into(),
            n => format!("You have {n} recent mails."),
        },
        Spoken::Titles(_) => match n {
            0 => format!("Nothing from {name}."),
            1 => format!("One item from {name}."),
            n => format!("{n} items from {name}."),
        },
    };
    let mut lines = vec![count];
    lines.extend(items.into_iter().filter_map(|item| match rule {
        Spoken::Calendar => Some(event_line(item)),
        Spoken::Mail => Some(mail_line(item)),
        Spoken::Titles(keys) => title_of(item, keys).map(|title| format!("{title}.")),
    }));
    lines
}

/// An item's title or name: the first of `keys` it has as text, or as an
/// object whose own `title` or `name` is text.
fn title_of(item: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        let value = item.get(*key)?;
        let text = value.as_str().or_else(|| {
            ["title", "name"]
                .iter()
                .find_map(|inner| value.get(*inner).and_then(Value::as_str))
        })?;
        Some(said(text)).filter(|title| !title.is_empty())
    })
}

/// An event as said: its local time and its title.
fn event_line(event: &Value) -> String {
    let title = said(event["summary"].as_str().unwrap_or_default());
    let title = if title.is_empty() {
        "an event with no title".to_owned()
    } else {
        title
    };
    match event["start"]["dateTime"].as_str().and_then(clock_of) {
        Some(at) => format!("At {at}, {title}."),
        None => format!("All day, {title}."),
    }
}

/// `HH:MM` of an RFC 3339 time, as the calendar gave it, its leading zero
/// taken off: "9:30".
fn clock_of(time: &str) -> Option<String> {
    let clock = time.get(11..16)?;
    let (hour, minute) = clock.split_once(':')?;
    let hour: u8 = hour.parse().ok()?;
    let fits = hour < 24 && minute.len() == 2 && minute.bytes().all(|b| b.is_ascii_digit());
    fits.then(|| format!("{hour}:{minute}"))
}

/// A mail as said: who it is from (their name, else their address) and
/// its subject. Never its text.
fn mail_line(mail: &Value) -> String {
    let from = mail["from"].as_str().unwrap_or_default();
    let name = match from.split_once('<') {
        Some((name, address)) => {
            let name = name.trim().trim_matches('"').trim();
            if name.is_empty() {
                address.trim_end_matches('>').trim()
            } else {
                name
            }
        }
        None => from.trim(),
    };
    let name = said(name);
    let name = if name.is_empty() {
        "someone".to_owned()
    } else {
        name
    };
    let subject = said(mail["subject"].as_str().unwrap_or_default());
    if subject.is_empty() {
        format!("From {name}, with no subject.")
    } else {
        format!("From {name}: {subject}.")
    }
}

/// Words as said: one line, no control characters, at most
/// [`MOST_SAID_CHARS`].
pub fn said(text: &str) -> String {
    let words: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|word| !word.is_empty())
        .collect();
    let line = words.join(" ");
    let line: String = line.chars().take(MOST_SAID_CHARS).collect();
    line.trim_end_matches(['.', ' ']).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Every service with a fixed read, and not the send-only Gmail link,
    /// the MCP services or Ahrefs, whose reads are all free-form.
    #[test]
    fn every_service_with_a_fixed_read_is_readable() {
        assert_eq!(
            readable(),
            [
                "gmail",
                "google_calendar",
                "google_drive",
                "figma",
                "figma-api",
                "github",
                "linear",
                "asana",
                "facebook"
            ]
        );
        assert!(not_readable("gmail-app").unwrap().contains("only sends"));
        assert!(not_readable("dropbox").is_some());
        for mcp in ["slack", "mondaydotcom", "hubspot", "amplitude", "notion"] {
            assert!(
                not_readable(mcp).unwrap().contains("can't read yet"),
                "{mcp}"
            );
        }
        assert!(not_readable("ahrefs").unwrap().contains("free-form"));
        // Every whole text names a read the service has.
        for (code, what) in WHOLE_TEXTS {
            let line = reads::WHATS.iter().find(|(c, _)| c == code).unwrap().1;
            assert!(
                split_whats(line).iter().any(|w| w.starts_with(what)),
                "{code} {what}"
            );
            assert!(
                !whats(code).iter().any(|w| w.starts_with(what)),
                "{code} {what}"
            );
        }
        assert_eq!(whats("google_drive"), ["files {query, count}", "file {id}"]);
        assert!(whats("notion").is_empty());
        assert!(whats("slack").is_empty());
        assert_eq!(
            whats("github"),
            ["notifications", "issues {repo}", "pulls {repo}", "repos"]
        );
        assert_eq!(whats("linear"), ["my_issues"]);
        assert_eq!(
            whats("asana"),
            [
                "me",
                "workspaces",
                "projects {workspace}",
                "tasks {project | workspace}"
            ]
        );
    }

    /// Mail and the calendar keep the ruling's words; every other service
    /// says the count and each title or name, never a body.
    #[test]
    fn what_is_said_is_one_rule_by_service() {
        assert_eq!(rule("gmail", "search"), Spoken::Mail);
        assert_eq!(rule("google_calendar", "events"), Spoken::Calendar);
        assert_eq!(rule("github", "issues"), Spoken::Titles(TITLE_KEYS));
        let drive = json!({"source": "Google Drive", "quoted": [
            {"name": "Tax 2026", "mimeType": "application/pdf"},
            {"id": "x", "description": "the secret plan"},
            "a security message, left out",
        ]});
        let lines = lines("google_drive", "files", None, &drive);
        assert_eq!(lines, ["2 items from Google Drive.", "Tax 2026."]);
        let github = json!({"quoted": [
            {"subject": {"title": "Fix the build", "type": "PullRequest"}, "body": "x"},
            {"title": "Crash on start", "body": "Steps: run it"},
        ]});
        let said = super::lines("github", "notifications", None, &github);
        assert_eq!(
            said,
            ["2 items from GitHub.", "Fix the build.", "Crash on start."]
        );
        assert!(!said.concat().contains("Steps"));
        assert_eq!(
            super::lines(
                "facebook",
                "posts",
                None,
                &json!({"quoted": [{"message": "a post"}]})
            ),
            ["One item from Facebook Page."]
        );
        assert_eq!(
            super::lines("asana", "tasks", None, &json!({"quoted": []})),
            ["Nothing from Asana."]
        );
    }

    /// A spoken yes names the service and what of it is said.
    #[test]
    fn a_read_is_said_plainly() {
        assert_eq!(
            plainly("google_calendar", "events", Some("tomorrow")),
            "reads you tomorrow's Google Calendar: how many events, then each time and title"
        );
        assert_eq!(
            plainly("gmail", "recent", None),
            "reads you your latest Gmail: how many, then each sender and subject, never the text"
        );
        assert_eq!(
            plainly("google_drive", "files", None),
            "reads you Google Drive's files: how many, then each name, never the text"
        );
    }

    /// Thirteen's P2 on #5303: a query, any path or an MCP call is never a
    /// skill's read, nor a whole text, nor a read the service doesn't list.
    #[test]
    fn a_skill_reads_only_fixed_lists() {
        assert_eq!(skill_read_refused("github", "issues"), None);
        assert_eq!(skill_read_refused("linear", "my_issues"), None);
        for (code, what, why) in [
            ("github", "query", "free-form"),
            ("linear", "query", "free-form"),
            ("ahrefs", "get", "free-form"),
            ("slack", "call", "can't read yet"),
            ("hubspot", "tools", "can't read yet"),
            ("gmail", "message", "whole text"),
            ("notion", "search", "can't read yet"),
            ("google_drive", "nothing", "reads files, file"),
            ("gmail-app", "recent", "only sends"),
        ] {
            let refused = skill_read_refused(code, what).unwrap_or_default();
            assert!(refused.contains(why), "{code} {what}: {refused}");
        }
    }
}
