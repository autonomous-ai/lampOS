//! Writes, in two phases. A `service_write` without a `send_id` checks what
//! was asked, keeps it in memory for two minutes and answers with its full
//! read-back (the target, every recipient, who can see it, the whole text)
//! and a random `send_id`; nothing is sent. The same `send_id` with
//! `"said":"send"` sends exactly that write, once. Anything else, a second
//! try or a late one, sends nothing. Off unless `LAMP_SERVICES_WRITE=on`.
//! Gmail's `save_draft`, over an app password's link only, goes the same
//! two phases and is saved in Drafts, never sent.
use crate::catalog::{self, FACEBOOK_VERSION, Method};
use crate::mail::{self, Outgoing};
use crate::reads::{self, page_id};
use crate::store::Entry;
use serde_json::{Value, json};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

/// How long a write waits for its "send".
pub const HOLD: Duration = Duration::from_secs(120);
/// The longest text a write carries, in characters.
pub const TEXT_MOST: usize = 3_000;
/// The longest subject or title.
pub const TITLE_MOST: usize = 200;
/// The most recipients one mail has.
pub const RECIPIENTS_MOST: usize = 20;

/// What a confirmed write does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// A mail through Gmail's SMTP.
    Mail(Outgoing),
    /// A mail saved in Gmail's Drafts over IMAP, never sent.
    Draft(Outgoing),
    /// One HTTP call on the service's write list, before its token.
    Http {
        method: Method,
        url: String,
        body: Value,
    },
    /// One write tool of an MCP service, called with these arguments.
    Mcp { tool: String, arguments: Value },
}

/// A write checked and read back, waiting for its "send".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pending {
    pub service: &'static str,
    pub action: Action,
    pub read_back: String,
    /// The linked account it was prepared as ([`Entry::account`]): a link
    /// replaced by another account's before the "send" sends or saves
    /// nothing (Reviewer-two on #5982: a draft prepared as one account was
    /// saved into the next one linked).
    pub account: String,
}

/// What each service writes, by `action`, for the tools' description.
pub const ACTIONS: &[(&str, &str)] = &[
    (
        "gmail",
        "send_mail {to, cc, subject, text}, save_draft {to, cc, subject, text} (app password link only)",
    ),
    ("gmail-app", "send_mail {to, cc, subject, text}"),
    (
        "google_calendar",
        "create_event {title, start, end, calendar, attendees, text}",
    ),
    (
        "github",
        "create_issue {repo, title, text}, comment {repo, number, text}",
    ),
    ("linear", "create_issue {team_id, title, text}"),
    ("asana", "create_task {project | workspace, title, text}"),
    ("figma", "comment {id, text}"),
    ("figma-api", "comment {id, text}"),
    ("facebook", "page_post {text}"),
];

/// What a Gmail link that can't save a draft answers `save_draft`: a
/// Google sign-in's (`gmail-app`, or `gmail` made with one). Its own words
/// are the voice's interim line (#5962), so the person still hears the
/// send it offers.
pub const NO_DRAFTS: &str = "this Gmail link can't save drafts: only one linked with an app password can. Say \"I can't save a draft yet. I can read it back and send it when you say 'send it'.\"";

/// What an MCP service writes, for the tools' description.
pub const MCP_ACTIONS: &str = "call {tool, arguments}";

/// What one write needs a link to allow: the service, its action, what it
/// does in the person's words, and the scopes any one of which allows it.
pub struct Allowed {
    pub code: &'static str,
    pub action: &'static str,
    pub what: &'static str,
    pub scopes: &'static [&'static str],
}

/// The scopes a write needs, from each service's own docs (Gmail's
/// `users.messages.send`, Calendar's `events.insert`), each a full scope
/// URL as Google records it. Only Google's sign-ins record their scopes;
/// [`scope_allows`] matches them whole. Google Drive's are with its writes
/// (#5825).
pub const SCOPES: &[Allowed] = &[
    Allowed {
        code: "gmail",
        action: "send_mail",
        what: "send mail",
        scopes: GMAIL_SEND,
    },
    Allowed {
        code: "gmail-app",
        action: "send_mail",
        what: "send mail",
        scopes: GMAIL_SEND,
    },
    Allowed {
        code: "google_calendar",
        action: "create_event",
        what: "add calendar events",
        scopes: &[
            "https://www.googleapis.com/auth/calendar",
            "https://www.googleapis.com/auth/calendar.events",
            "https://www.googleapis.com/auth/calendar.events.owned",
        ],
    },
];

const GMAIL_SEND: &[&str] = &[
    "https://www.googleapis.com/auth/gmail.send",
    "https://www.googleapis.com/auth/gmail.compose",
    "https://www.googleapis.com/auth/gmail.modify",
    "https://mail.google.com/",
];

/// Whether `entry`'s scopes hold one of `needed`, each taken whole as a
/// full scope URL (a list kept as one spaced string read alike); a bare
/// name such as `gmail.send` allows nothing.
/// Drive's create uses it too once #5825 lands.
pub fn scope_allows(entry: &Entry, needed: &[&str]) -> bool {
    entry
        .scopes
        .iter()
        .flat_map(|scopes| scopes.split_whitespace())
        .any(|scope| needed.contains(&scope))
}

/// What `args`' write on `code` would do, when `entry` is a sign-in that
/// records its scopes and none of them allows it; else `None`. An app
/// password, a personal key or a link recording no scopes is not checked.
pub fn scope_missing(code: &str, args: &Value, entry: &Entry) -> Option<&'static str> {
    let recorded = entry.scopes.iter().any(|scopes| !scopes.trim().is_empty());
    if !entry.oauth() || !recorded {
        return None;
    }
    let action = text(args, "action")?;
    let allowed = SCOPES
        .iter()
        .find(|allowed| allowed.code == code && allowed.action == action)?;
    (!scope_allows(entry, allowed.scopes)).then_some(allowed.what)
}

/// What a write on `code` does, for a refusal that doesn't name its action.
pub fn what_written(code: &str) -> &'static str {
    SCOPES
        .iter()
        .find(|allowed| allowed.code == code)
        .map_or("make this change", |allowed| allowed.what)
}

/// Whether a 401 or 403's body says the link's scopes are too few, in
/// Google's words for it.
pub fn scope_refused(body: &[u8]) -> bool {
    let body = String::from_utf8_lossy(body);
    body.contains("ACCESS_TOKEN_SCOPE_INSUFFICIENT") || body.contains("insufficientPermissions")
}

/// An MCP service's write, checked and read back: `tool` is the service's
/// own listing of it, which is a write (`mcp::is_read` false).
pub fn prepare_mcp(
    service: &'static catalog::Service,
    entry: &Entry,
    tool: &Value,
    arguments: Value,
) -> Result<Pending, String> {
    let name = tool
        .get("name")
        .and_then(Value::as_str)
        .ok_or("the tool has no name")?;
    if crate::mcp::is_read(service.code, tool) {
        return Err(format!(
            "{name} only reads: use service_read with what \"call\""
        ));
    }
    crate::mcp::write_allowed(service.code, name, &arguments)?;
    Ok(Pending {
        account: entry.account(),
        service: service.code,
        read_back: crate::mcp::read_back(service, entry, tool, &arguments),
        action: Action::Mcp {
            tool: name.to_owned(),
            arguments,
        },
    })
}

fn text<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

/// A text the person said, whole: bounded, its line breaks kept.
fn body<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    let body = text(args, key).ok_or_else(|| format!("the write needs its {key}"))?;
    if body.chars().count() > TEXT_MOST
        || body
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(format!("{key} is at most {TEXT_MOST} characters"));
    }
    Ok(body)
}

/// A one-line title or subject.
fn title<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    let title = text(args, key).ok_or_else(|| format!("the write needs its {key}"))?;
    if title.chars().count() > TITLE_MOST || title.chars().any(char::is_control) {
        return Err(format!(
            "{key} is one line of at most {TITLE_MOST} characters"
        ));
    }
    Ok(title)
}

/// An id that goes in a path.
fn id<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    let id = text(args, key).ok_or_else(|| format!("the write needs its {key}"))?;
    if id.len() > 256 || id.chars().any(|c| c.is_control() || "/\\?#".contains(c)) {
        return Err(format!("{key} is not an id"));
    }
    Ok(id)
}

/// Addresses given as a list or one comma-separated text, each checked.
fn addresses(args: &Value, key: &str) -> Result<Vec<String>, String> {
    let given: Vec<String> = match args.get(key) {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::String(text)) => text
            .split(',')
            .map(|one| one.trim().to_owned())
            .filter(|one| !one.is_empty())
            .collect(),
        Some(Value::Array(list)) => list
            .iter()
            .map(|one| one.as_str().map(|one| one.trim().to_owned()))
            .collect::<Option<_>>()
            .ok_or_else(|| format!("{key} is a list of email addresses"))?,
        Some(_) => return Err(format!("{key} is a list of email addresses")),
    };
    if let Some(bad) = given.iter().find(|one| !mail::address_ok(one)) {
        return Err(format!("{bad:?} is not an email address"));
    }
    Ok(given)
}

fn time<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    let time = text(args, key).ok_or_else(|| format!("the event needs its {key}"))?;
    if !reads::calendar_time(time) {
        return Err(format!(
            "{key} is a time with this lamp's own UTC offset, like 2026-11-03T09:00:00+HH:MM"
        ));
    }
    Ok(time)
}

fn base(code: &str) -> Result<&'static str, String> {
    catalog::service(code)
        .and_then(|service| service.api)
        .ok_or_else(|| "the lamp doesn't write to that service".into())
}

fn http(
    code: &'static str,
    method: Method,
    segments: &[&str],
    query: &[(&str, String)],
    body: Value,
    read_back: String,
) -> Result<Pending, String> {
    Ok(Pending {
        account: String::new(),
        service: code,
        action: Action::Http {
            method,
            url: reads::url(base(code)?, segments, query)?,
            body,
        },
        read_back,
    })
}

/// A write checked and read back, or why not. `entry` is the linked
/// account it is made as, and the write is bound to it.
pub fn prepare(code: &str, args: &Value, entry: &Entry) -> Result<Pending, String> {
    prepare_unbound(code, args, entry).map(|pending| Pending {
        account: entry.account(),
        ..pending
    })
}

/// [`prepare`], its account not yet bound.
fn prepare_unbound(code: &str, args: &Value, entry: &Entry) -> Result<Pending, String> {
    let service = catalog::service(code).ok_or("the lamp doesn't know that service")?;
    let code = service.code;
    let action = text(args, "action").ok_or("the write needs its action")?;
    if service.mcp.is_some() {
        // Its tool is checked against the service's own list first.
        return Err(format!("{code} writes: {MCP_ACTIONS}"));
    }
    match (code, action) {
        // An app password's link or a Google sign-in's, sent alike.
        ("gmail" | "gmail-app", "send_mail") => {
            let (out, copy) = outgoing(args)?;
            Ok(Pending {
                account: String::new(),
                service: code,
                read_back: format!(
                    "Send an email from {} to {}{copy}. Subject: \"{}\". It says: \"{}\". Only the people named get it.",
                    entry.account(),
                    out.to.join(", "),
                    out.subject,
                    out.text,
                ),
                action: Action::Mail(out),
            })
        }
        // The owner's ask of 10 Nov: a mail drafted by voice saved in
        // Gmail's Drafts, over the app password's IMAP (APPEND); a Google
        // sign-in's link has no `gmail.compose` here and saves none.
        ("gmail", "save_draft") if !entry.oauth() => {
            let (out, copy) = outgoing(args)?;
            Ok(Pending {
                account: String::new(),
                service: code,
                read_back: format!(
                    "Save a draft to {}{copy}, in the Gmail Drafts of {}. Subject: \"{}\". It says: \"{}\". Nothing is sent.",
                    out.to.join(", "),
                    entry.account(),
                    out.subject,
                    out.text,
                ),
                action: Action::Draft(out),
            })
        }
        ("gmail" | "gmail-app", "save_draft") => Err(NO_DRAFTS.into()),
        ("google_calendar", "create_event") => {
            let calendar = text(args, "calendar").unwrap_or("primary");
            if calendar.len() > 256 || calendar.contains(['/', '\\']) {
                return Err("calendar is a calendar's id".into());
            }
            let name = title(args, "title")?;
            let (start, end) = (time(args, "start")?, time(args, "end")?);
            let attendees = addresses(args, "attendees")?;
            if attendees.len() > RECIPIENTS_MOST {
                return Err(format!("an event invites at most {RECIPIENTS_MOST} people"));
            }
            let notes = match text(args, "text") {
                Some(_) => Some(body(args, "text")?),
                None => None,
            };
            let mut event = json!({
                "summary": name,
                "start": {"dateTime": start},
                "end": {"dateTime": end},
            });
            if let Some(notes) = notes {
                event["description"] = json!(notes);
            }
            let mut query = Vec::new();
            let mut invited = String::new();
            if !attendees.is_empty() {
                event["attendees"] = json!(
                    attendees
                        .iter()
                        .map(|email| json!({"email": email}))
                        .collect::<Vec<_>>()
                );
                query.push(("sendUpdates", "all".to_owned()));
                invited = format!(
                    ", inviting {} (each gets an invitation by email)",
                    attendees.join(", ")
                );
            }
            let noted = notes.map_or(String::new(), |notes| format!(" Notes: \"{notes}\"."));
            http(
                code,
                Method::Post,
                &["calendar", "v3", "calendars", calendar, "events"],
                &query,
                event,
                format!(
                    "Add an event to your Google Calendar ({calendar}): \"{name}\", from {start} to {end}{invited}.{noted} Seen by whoever your calendar is shared with{}.",
                    if attendees.is_empty() {
                        ""
                    } else {
                        " and the people invited"
                    }
                ),
            )
        }
        ("github", "create_issue") => {
            let (owner, name) = repo(args)?;
            let (heading, text) = (title(args, "title")?, body(args, "text")?);
            http(
                code,
                Method::Post,
                &["repos", owner, name, "issues"],
                &[],
                json!({"title": heading, "body": text}),
                format!(
                    "Open a GitHub issue in {owner}/{name} titled \"{heading}\", saying: \"{text}\". Everyone who can see {owner}/{name} sees it; anyone at all if it is public."
                ),
            )
        }
        ("github", "comment") => {
            let (owner, name) = repo(args)?;
            let number = args
                .get("number")
                .and_then(Value::as_u64)
                .filter(|number| *number > 0)
                .ok_or("the comment needs the issue's number")?
                .to_string();
            let text = body(args, "text")?;
            http(
                code,
                Method::Post,
                &["repos", owner, name, "issues", &number, "comments"],
                &[],
                json!({"body": text}),
                format!(
                    "Comment on GitHub issue {owner}/{name}#{number}: \"{text}\". Everyone who can see {owner}/{name} sees it; anyone at all if it is public."
                ),
            )
        }
        ("linear", "create_issue") => {
            let team = id(args, "team_id")?;
            let (heading, text) = (title(args, "title")?, body(args, "text")?);
            http(
                code,
                Method::Post,
                &["graphql"],
                &[],
                json!({
                    "query": "mutation($input: IssueCreateInput!) { issueCreate(input: $input) { success issue { identifier url } } }",
                    "variables": {"input": {"teamId": team, "title": heading, "description": text}},
                }),
                format!(
                    "Create a Linear issue in team {team} titled \"{heading}\", saying: \"{text}\". Your Linear workspace's members see it."
                ),
            )
        }
        ("asana", "create_task") => {
            let (heading, text) = (title(args, "title")?, body(args, "text")?);
            let (data, place) = match args.get("project").and_then(Value::as_str) {
                Some(_) => {
                    let project = id(args, "project")?;
                    (
                        json!({"name": heading, "notes": text, "projects": [project]}),
                        format!("project {project}"),
                    )
                }
                None => {
                    let workspace = id(args, "workspace")?;
                    (
                        json!({"name": heading, "notes": text, "workspace": workspace, "assignee": "me"}),
                        format!("workspace {workspace}, assigned to you"),
                    )
                }
            };
            http(
                code,
                Method::Post,
                &["api", "1.0", "tasks"],
                &[],
                json!({"data": data}),
                format!(
                    "Create an Asana task in {place} named \"{heading}\", with the notes: \"{text}\". Whoever shares that project or workspace sees it."
                ),
            )
        }
        ("figma" | "figma-api", "comment") => {
            let file = id(args, "id")?;
            let text = body(args, "text")?;
            http(
                code,
                Method::Post,
                &["v1", "files", file, "comments"],
                &[],
                json!({"message": text}),
                format!(
                    "Comment on the Figma file {file}: \"{text}\". Everyone with access to the file sees it."
                ),
            )
        }
        ("facebook", "page_post") => {
            let page = page_id(entry);
            if page == "me" {
                return Err("the Facebook link names no Page".into());
            }
            let text = body(args, "text")?;
            http(
                code,
                Method::Post,
                &[FACEBOOK_VERSION, page, "feed"],
                &[],
                json!({"message": text}),
                format!(
                    "Post publicly on your Facebook Page {page}: \"{text}\". Anyone on Facebook can see it."
                ),
            )
        }
        _ => {
            let actions = ACTIONS
                .iter()
                .find(|(service, _)| *service == code)
                .map(|(_, actions)| *actions);
            Err(match actions {
                Some(actions) => format!("{code} writes: {actions}"),
                None => format!("the lamp only reads {}", service.name),
            })
        }
    }
}

/// A mail's recipients, subject and text, each checked, and the words
/// that say its copies in a read-back.
fn outgoing(args: &Value) -> Result<(Outgoing, String), String> {
    let to = addresses(args, "to")?;
    let cc = addresses(args, "cc")?;
    if to.is_empty() {
        return Err("the mail needs at least one address in to".into());
    }
    if to.len() + cc.len() > RECIPIENTS_MOST {
        return Err(format!("a mail goes to at most {RECIPIENTS_MOST} people"));
    }
    let subject = title(args, "subject")?.to_owned();
    let text = body(args, "text")?.to_owned();
    let copy = if cc.is_empty() {
        String::new()
    } else {
        format!(", with a copy to {}", cc.join(", "))
    };
    Ok((
        Outgoing {
            to,
            cc,
            subject,
            text,
        },
        copy,
    ))
}

fn repo(args: &Value) -> Result<(&str, &str), String> {
    let repo = text(args, "repo").ok_or("the write needs its repo")?;
    repo.split_once('/')
        .filter(|(owner, name)| {
            [owner, name].iter().all(|part| {
                !part.is_empty()
                    && part.len() <= 100
                    && part
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
                    && **part != "."
                    && **part != ".."
            })
        })
        .ok_or_else(|| "repo is owner/name".into())
}

/// A random id: 16 bytes from the system's generator, as hex.
pub fn random_id() -> String {
    use ring::rand::SecureRandom;
    let mut bytes = [0u8; 16];
    // The system generator failing leaves zeros; the id is then still
    // single-use and short-lived.
    let _ = ring::rand::SystemRandom::new().fill(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The one write waiting for its "send", with its id and when it was made.
#[derive(Debug)]
struct Held {
    id: String,
    pending: Pending,
    made: Instant,
}

/// Where a write waits. One at a time: a new one replaces the one before.
#[derive(Debug, Default)]
pub struct Desk(Mutex<Option<Held>>);

impl Desk {
    /// Hold `pending` from `now`; its `send_id`.
    pub fn hold(&self, pending: Pending, now: Instant) -> String {
        let id = random_id();
        *self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Held {
            id: id.clone(),
            pending,
            made: now,
        });
        id
    }

    /// The write `id` names, if it is the one waiting and still within
    /// [`HOLD`] at `now`. Whatever is waiting is gone after: a write is
    /// sent once, and a wrong or late id cancels it.
    pub fn take(&self, id: &str, now: Instant) -> Option<Pending> {
        let held = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()?;
        (held.id == id && now.saturating_duration_since(held.made) <= HOLD).then_some(held.pending)
    }

    /// Drop what is waiting: for `code` alone, or with `None` anything.
    pub fn forget(&self, code: Option<&str>) {
        let mut held = self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if code.is_none_or(|code| {
            held.as_ref()
                .is_some_and(|held| held.pending.service == code)
        }) {
            *held = None;
        }
    }

    /// Whether a write is waiting.
    pub fn waiting(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gmail() -> Entry {
        Entry::from_set(
            "gmail",
            &json!({"auth_type":"pat","api_key":"pw","credentials":{"email":"me@home.test"}}),
            0,
        )
        .unwrap()
    }

    /// A Google sign-in for Gmail recording `scopes`.
    fn gmail_scoped(scopes: Value) -> Entry {
        Entry::from_set(
            "gmail",
            &json!({"auth_type":"oauth","access_token":"t","expires_in":3600,"scopes":scopes}),
            0,
        )
        .unwrap()
    }

    /// Scopes are full URLs, a list kept as one spaced string read alike;
    /// a bare name allows nothing.
    #[test]
    fn a_scope_allows_only_as_its_full_url() {
        let send = json!({"action":"send_mail"});
        let spaced = gmail_scoped(json!([
            "https://www.googleapis.com/auth/gmail.readonly https://www.googleapis.com/auth/gmail.send"
        ]));
        assert!(scope_allows(&spaced, GMAIL_SEND));
        assert_eq!(scope_missing("gmail", &send, &spaced), None);
        let full_gmail = gmail_scoped(json!(["https://mail.google.com/"]));
        assert_eq!(scope_missing("gmail", &send, &full_gmail), None);
        for bare in [json!(["gmail.send"]), json!(["gmail.readonly gmail.send"])] {
            let entry = gmail_scoped(bare.clone());
            assert!(!scope_allows(&entry, GMAIL_SEND), "{bare}");
            assert_eq!(
                scope_missing("gmail", &send, &entry),
                Some("send mail"),
                "{bare}"
            );
        }
        // A link recording no scopes is not checked.
        for none in [json!([]), json!(["  "])] {
            assert_eq!(scope_missing("gmail", &send, &gmail_scoped(none)), None);
        }
    }

    #[test]
    fn notion_writes_only_create_add_to_and_update_pages() {
        let notion = catalog::service("notion").unwrap();
        let entry = Entry::from_set(
            "notion",
            &json!({"auth_type":"oauth","access_token":"planted-notion-4710","expires_in":3600}),
            0,
        )
        .unwrap();
        let tool = |name: &str| json!({"name": name});
        let update = json!({"data": {"page_id": "p1", "command": "insert_content_after", "new_str": "Milk"}});
        assert!(prepare_mcp(notion, &entry, &tool("notion-update-page"), update).is_ok());
        let create =
            json!({"parent": {"page_id": "p1"}, "pages": [{"properties": {"title": "Groceries"}}]});
        assert!(prepare_mcp(notion, &entry, &tool("notion-create-pages"), create).is_ok());
        // Left out: delete, archive, move, duplicate, a database, a comment, a share.
        for name in [
            "notion-move-pages",
            "notion-duplicate-page",
            "notion-create-database",
            "notion-update-database",
            "notion-create-comment",
            "notion-delete-page",
            "notion-share-page",
        ] {
            let refused = prepare_mcp(notion, &entry, &tool(name), json!({})).unwrap_err();
            assert!(
                refused.contains("never deletes, archives, moves or shares"),
                "{name}: {refused}"
            );
        }
        // An allowed update that would trash or archive the page is refused too.
        for key in ["archived", "in_trash", "Is_Archived"] {
            let args = json!({"data": {"page_id": "p1", "properties": {key: true}}});
            assert!(
                prepare_mcp(notion, &entry, &tool("notion-update-page"), args).is_err(),
                "{key}"
            );
        }
    }

    #[test]
    fn a_read_back_says_every_recipient_and_the_whole_text() {
        let args = json!({
            "action": "send_mail",
            "to": ["mom@home.test", "dad@home.test"],
            "cc": "sis@home.test",
            "subject": "Dinner",
            "text": "Home by 7.\nLove",
        });
        let pending = prepare("gmail", &args, &gmail()).unwrap();
        for part in [
            "me@home.test",
            "mom@home.test, dad@home.test",
            "sis@home.test",
            "\"Dinner\"",
            "Home by 7.\nLove",
            "Only the people named",
        ] {
            assert!(pending.read_back.contains(part), "{part}");
        }
        // A send-only Google sign-in's mail is read back the same, from
        // its own account, as the same mail.
        let signed_in = Entry::from_set(
            "gmail-app",
            &json!({"auth_type":"oauth","access_token":"t","user_email":"me@home.test"}),
            0,
        )
        .unwrap();
        let from_app = prepare("gmail-app", &args, &signed_in).unwrap();
        assert_eq!(from_app.service, "gmail-app");
        assert_eq!(from_app.read_back, pending.read_back);
        assert_eq!(from_app.action, pending.action);
        let post = prepare(
            "facebook",
            &json!({"action":"page_post","text":"Open today"}),
            &Entry {
                credentials: [("page_id".to_owned(), "42".to_owned())].into(),
                ..Entry::default()
            },
        )
        .unwrap();
        assert!(post.read_back.contains("publicly"));
        let Action::Http { method, url, .. } = &post.action else {
            panic!("http");
        };
        let (_, path) = crate::net::split(url).unwrap();
        assert!(catalog::write_allowed("facebook", *method, &path));
        // Each service's write lands on its own write list.
        for (code, args) in [
            (
                "github",
                json!({"action":"create_issue","repo":"o/r","title":"T","text":"B"}),
            ),
            (
                "github",
                json!({"action":"comment","repo":"o/r","number":4,"text":"B"}),
            ),
            (
                "linear",
                json!({"action":"create_issue","team_id":"t","title":"T","text":"B"}),
            ),
            (
                "asana",
                json!({"action":"create_task","workspace":"1","title":"T","text":"B"}),
            ),
            ("figma", json!({"action":"comment","id":"k","text":"B"})),
            (
                "google_calendar",
                json!({"action":"create_event","title":"T","start":"2026-11-03T09:00:00Z","end":"2026-11-03T10:00:00Z","attendees":["a@b.co"]}),
            ),
        ] {
            let pending = prepare(code, &args, &Entry::default()).unwrap();
            let Action::Http { method, url, .. } = &pending.action else {
                panic!("http");
            };
            let (host, path) = crate::net::split(url).unwrap();
            assert!(catalog::host_allowed(code, &host), "{code}");
            assert!(
                catalog::write_allowed(code, *method, &path),
                "{code} {path}"
            );
        }
        for (code, args) in [
            (
                "google_calendar",
                json!({"action":"create_event","title":"T","start":"2026-11-05T09:00:00","end":"2026-11-05T09:15:00"}),
            ),
            (
                "gmail",
                json!({"action":"send_mail","to":"not an address","subject":"s","text":"t"}),
            ),
            (
                "gmail",
                json!({"action":"send_mail","to":"a@b.co","subject":"s\r\nBcc: x@y.z","text":"t"}),
            ),
            (
                "gmail",
                json!({"action":"send_mail","to":"a@b.co\r\nRCPT TO:<x@y.z>","subject":"s","text":"t"}),
            ),
            ("google_drive", json!({"action":"delete","id":"x"})),
            ("ahrefs", json!({"action":"anything"})),
            (
                "github",
                json!({"action":"create_issue","repo":"../x","title":"T","text":"B"}),
            ),
            ("facebook", json!({"action":"page_post","text":"no page"})),
        ] {
            assert!(prepare(code, &args, &gmail()).is_err(), "{code} {args}");
        }
    }

    /// The owner's ask of 10 Nov: `save_draft` is prepared as `send_mail`
    /// is, read back as a draft that sends nothing, and only over an app
    /// password's link; a Google sign-in's Gmail is told it can't.
    #[test]
    fn a_draft_is_read_back_as_a_draft_and_only_for_an_app_password() {
        let args = json!({
            "action": "save_draft",
            "to": ["mom@home.test"],
            "subject": "Dinner",
            "text": "Home by 7.\nLove",
        });
        let pending = prepare("gmail", &args, &gmail()).unwrap();
        assert_eq!(
            pending.read_back,
            "Save a draft to mom@home.test, in the Gmail Drafts of me@home.test. Subject: \"Dinner\". It says: \"Home by 7.\nLove\". Nothing is sent."
        );
        assert_eq!(
            pending.action,
            Action::Draft(Outgoing {
                to: vec!["mom@home.test".into()],
                cc: Vec::new(),
                subject: "Dinner".into(),
                text: "Home by 7.\nLove".into(),
            })
        );
        // Checked as a send is.
        for bad in [
            json!({"action":"save_draft","subject":"s","text":"t"}),
            json!({"action":"save_draft","to":"a@b.co","subject":"s\r\nBcc: x@y.z","text":"t"}),
            json!({"action":"save_draft","to":"a@b.co","subject":"s"}),
        ] {
            assert!(prepare("gmail", &bad, &gmail()).is_err(), "{bad}");
        }
        // A Google sign-in's link, either name: refused with the interim line.
        let signed_in = gmail_scoped(json!(["https://www.googleapis.com/auth/gmail.send"]));
        for code in ["gmail", "gmail-app"] {
            assert_eq!(prepare(code, &args, &signed_in), Err(NO_DRAFTS.to_owned()));
        }
        assert!(NO_DRAFTS.contains("I can't save a draft yet."));
        // Told only as gmail's, the app password's.
        let actions = |code: &str| {
            ACTIONS
                .iter()
                .find(|(service, _)| *service == code)
                .unwrap()
                .1
        };
        assert!(
            actions("gmail")
                .contains("save_draft {to, cc, subject, text} (app password link only)")
        );
        assert!(!actions("gmail-app").contains("save_draft"));
    }

    #[test]
    fn a_write_is_sent_once_within_two_minutes() {
        let pending = prepare(
            "gmail",
            &json!({"action":"send_mail","to":"a@b.co","subject":"s","text":"t"}),
            &gmail(),
        )
        .unwrap();
        let desk = Desk::default();
        let start = Instant::now();
        let id = desk.hold(pending.clone(), start);
        assert_eq!(id.len(), 32);
        assert_eq!(
            desk.take(&id, start + Duration::from_secs(5)),
            Some(pending.clone())
        );
        // Once only.
        assert_eq!(desk.take(&id, start + Duration::from_secs(6)), None);
        // Late: gone.
        let id = desk.hold(pending.clone(), start);
        assert_eq!(desk.take(&id, start + HOLD + Duration::from_secs(1)), None);
        // A wrong id cancels the one waiting.
        let id = desk.hold(pending.clone(), start);
        assert_eq!(desk.take("guess", start), None);
        assert_eq!(desk.take(&id, start), None);
        // A new write replaces the one before.
        let first = desk.hold(pending.clone(), start);
        let second = desk.hold(pending.clone(), start);
        assert_ne!(first, second);
        assert_eq!(desk.take(&first, start), None);
        assert!(!desk.waiting());
        // Removing a service forgets its write.
        desk.hold(pending.clone(), start);
        desk.forget(Some("github"));
        assert!(desk.waiting());
        desk.forget(Some("gmail"));
        assert!(!desk.waiting());
    }
}
