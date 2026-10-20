//! What another process says to the memory, and what the memory says back.
//!
//! A request is one JSON object on one line, with an `"op"`. The answer is
//! one JSON object on one line: `{"ok":true,...}`, or
//! `{"ok":false,"error":"short_code","message":"..."}` for anything that
//! could not be done. A refusal never ends the connection.
//!
//! Every request is for one person, `"everyone"` when none is named. An id
//! that is not `[a-z0-9_-]{1,32}` is refused here, before the store is
//! asked anything, so no such text ever becomes part of a path. `null` for a
//! person means every person, and only `forget_all` and `status` take it:
//! there is no request that gives one person's memory in answer to another.
//!
//! [`answer`] also makes the one line that is logged for a request. It
//! names the request, the person and the counts. It never has a word of
//! what was remembered, asked for or refused in it: the log is read by
//! whoever looks after the lamp, and the memory is not theirs.
use crate::store::{Ask, Day, Entry, Kept, Kind, Person, Refused, Removed, Status, Store};
use serde::{Deserialize, Deserializer, Serialize, Serializer, ser::SerializeMap};
use std::io;

/// A longer request line is refused whole.
pub const MOST_LINE_BYTES: usize = 64 * 1024;
/// How many entries a recall gives when it does not say, and the most it
/// can ask for.
pub const RECALL_ENTRIES: u64 = 8;
pub const MOST_RECALL_ENTRIES: u64 = 50;
/// How many bytes of text a recall gives when it does not say, and the most
/// it can ask for.
pub const RECALL_BYTES: u64 = 4096;
pub const MOST_RECALL_BYTES: u64 = 16_384;
/// How many bytes of kept text are given when the request does not say.
pub const KEPT_BYTES: u64 = 4096;

const OPS: [&str; 7] = [
    "append",
    "remember",
    "kept",
    "recall",
    "forget_last",
    "forget_all",
    "status",
];

/// Whose memory a request is for, as the request put it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum Who {
    /// No `"person"` at all.
    #[default]
    Unsaid,
    /// `"person": null`: every person.
    All,
    /// An id, not yet looked at.
    Id(String),
}

impl<'de> Deserialize<'de> for Who {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(match Option::<String>::deserialize(deserializer)? {
            Some(id) => Self::Id(id),
            None => Self::All,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    /// An exchange the lamp took part in. Either side may be missing: the
    /// lamp sometimes speaks first.
    Append {
        #[serde(default)]
        person: Who,
        at_unix_ms: u64,
        #[serde(default)]
        heard: String,
        #[serde(default)]
        said: String,
    },
    /// Something the lamp was asked to keep.
    Remember {
        #[serde(default)]
        person: Who,
        at_unix_ms: u64,
        text: String,
        /// The Days store's: the conversation's exchanges it came up in
        /// (#1693), removed with it. The legacy store keeps none.
        #[serde(default)]
        source_ids: Vec<u64>,
        /// The Days store's (#2176, `LAMP_MEMORY_REMEMBER_ONCE`): what this
        /// person already keeps (the same words, ignoring case, spacing and a
        /// final punctuation mark) is answered with that kept thing's id and
        /// `"existing":true`; no entry is written, and it gains these
        /// `source_ids`. The legacy store ignores it.
        #[serde(default)]
        once: bool,
    },
    /// What it was asked to keep, for telling a new session.
    Kept {
        #[serde(default)]
        person: Who,
        #[serde(default)]
        max_bytes: Option<u64>,
    },
    Recall {
        #[serde(default)]
        person: Who,
        #[serde(default)]
        query: Option<String>,
        #[serde(default)]
        day: Option<String>,
        #[serde(default)]
        limit: Option<u64>,
        #[serde(default)]
        max_bytes: Option<u64>,
    },
    ForgetLast {
        #[serde(default)]
        person: Who,
        /// `"kept"` or `"exchange"`: the last of that kind only.
        #[serde(default)]
        kind: Option<Kind>,
    },
    ForgetAll {
        #[serde(default)]
        person: Who,
    },
    Status {
        #[serde(default)]
        person: Who,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Response {
    /// An exchange or a kept thing was written, with this id.
    Added {
        id: u64,
    },
    Kept {
        entries: Vec<Kept>,
    },
    Recalled {
        entries: Vec<Entry>,
    },
    /// What `forget_last` removed, if there was anything.
    Forgot {
        removed: Option<Removed>,
    },
    /// How many entries `forget_all` removed.
    Emptied {
        removed: u64,
    },
    Status {
        people: Vec<Status>,
    },
    Refused {
        error: &'static str,
        message: String,
    },
}

impl Serialize for Response {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("ok", &!matches!(self, Self::Refused { .. }))?;
        match self {
            Self::Added { id } => map.serialize_entry("id", id)?,
            Self::Kept { entries } => map.serialize_entry("entries", entries)?,
            Self::Recalled { entries } => map.serialize_entry("entries", entries)?,
            Self::Forgot { removed } => map.serialize_entry("removed", removed)?,
            Self::Emptied { removed } => map.serialize_entry("removed", removed)?,
            Self::Status { people } => map.serialize_entry("people", people)?,
            Self::Refused { error, message } => {
                map.serialize_entry("error", error)?;
                map.serialize_entry("message", message)?;
            }
        }
        map.end()
    }
}

fn refuse(error: &'static str, message: impl Into<String>) -> Response {
    Response::Refused {
        error,
        message: message.into(),
    }
}

fn disk(error: io::Error) -> Response {
    refuse("storage", error.to_string())
}

fn not_added(refused: Refused) -> Response {
    match refused {
        Refused::Empty => refuse("empty", "there is no text in it to remember"),
        Refused::Time => refuse(
            "bad_time",
            "at_unix_ms is not a moment between 1970 and the year 9999 on the lamp's clock",
        ),
        Refused::Disk(error) => disk(error),
    }
}

fn person_of(id: &str) -> Result<Person, Response> {
    Person::new(id)
        .ok_or_else(|| refuse("bad_person", "a person id is 1 to 32 of a-z, 0-9, _ and -"))
}

/// The one person a request is for.
fn one(who: Who) -> Result<Person, Response> {
    match who {
        Who::Unsaid => Ok(Person::everyone()),
        Who::Id(id) => person_of(&id),
        Who::All => Err(refuse(
            "bad_person",
            "\"person\": null means every person, and only forget_all and status take it",
        )),
    }
}

impl Request {
    /// The request on this line, or the refusal to send back.
    pub fn parse(line: &[u8]) -> Result<Self, Response> {
        let value: serde_json::Value = serde_json::from_slice(line)
            .map_err(|error| refuse("bad_json", format!("the line is not JSON: {error}")))?;
        let op = value
            .as_object()
            .and_then(|object| object.get("op"))
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| refuse("bad_request", "a request is a JSON object with an \"op\""))?;
        if !OPS.contains(&op) {
            return Err(refuse(
                "unknown_op",
                format!("the ops are {}", OPS.join(", ")),
            ));
        }
        serde_json::from_value(value).map_err(|error| refuse("bad_request", error.to_string()))
    }

    pub fn op(&self) -> &'static str {
        match self {
            Self::Append { .. } => "append",
            Self::Remember { .. } => "remember",
            Self::Kept { .. } => "kept",
            Self::Recall { .. } => "recall",
            Self::ForgetLast { .. } => "forget_last",
            Self::ForgetAll { .. } => "forget_all",
            Self::Status { .. } => "status",
        }
    }

    /// Whose memory, for the log: an id that is one, `*` for every person,
    /// `?` for anything that is not an id. What was sent as an id and is
    /// not one is never written anywhere.
    fn whose(&self) -> &str {
        let (who, unsaid) = match self {
            Self::Append { person, .. }
            | Self::Remember { person, .. }
            | Self::Kept { person, .. }
            | Self::Recall { person, .. }
            | Self::ForgetLast { person, .. }
            | Self::ForgetAll { person } => (person, Person::EVERYONE),
            Self::Status { person } => (person, "*"),
        };
        match who {
            Who::Unsaid => unsaid,
            Who::All => "*",
            Who::Id(id) if Person::new(id).is_some() => id,
            Who::Id(_) => "?",
        }
    }
}

impl Response {
    /// The line that answers, without its newline.
    pub fn line(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| {
            r#"{"ok":false,"error":"storage","message":"the answer could not be written"}"#.into()
        })
    }

    /// How it went, for the log: counts and codes, never text.
    fn outcome(&self) -> String {
        match self {
            Self::Added { id } => format!("ok id={id}"),
            Self::Kept { entries } => {
                let bytes: usize = entries.iter().map(|kept| kept.text.len()).sum();
                format!("ok entries={} bytes={bytes}", entries.len())
            }
            Self::Recalled { entries } => {
                let bytes: usize = entries.iter().map(Entry::bytes).sum();
                format!("ok entries={} bytes={bytes}", entries.len())
            }
            Self::Forgot { removed: None } => "ok removed=0".into(),
            Self::Forgot {
                removed: Some(removed),
            } => {
                let kind = match removed.kind {
                    Kind::Exchange => "exchange",
                    Kind::Kept => "kept",
                };
                format!("ok removed=1 kind={kind} id={}", removed.id)
            }
            Self::Emptied { removed } => format!("ok removed={removed}"),
            Self::Status { people } => {
                let sum = |count: fn(&Status) -> u64| people.iter().map(count).sum::<u64>();
                format!(
                    "ok people={} exchanges={} kept={} bytes={} skipped_lines={}",
                    people.len(),
                    sum(|status| status.exchanges),
                    sum(|status| status.kept),
                    sum(|status| status.bytes),
                    sum(|status| status.skipped_lines),
                )
            }
            // The disk's own words for what went wrong have no remembered
            // text in them. Every other message can quote the request.
            Self::Refused {
                error: "storage",
                message,
            } => format!("refused error=storage ({message})"),
            Self::Refused { error, .. } => format!("refused error={error}"),
        }
    }
}

/// Do what is asked. `now_unix_ms` is this process's own clock: the day the
/// journal's age is counted from is taken from it and from nothing a
/// request says.
pub fn handle(store: &mut Store, request: Request, now_unix_ms: u64) -> Response {
    // Whatever is asked first on a new day, the days now too old go before
    // it is answered.
    store.turn_to(now_unix_ms);
    done(store, request, now_unix_ms).unwrap_or_else(|refusal| refusal)
}

fn done(store: &mut Store, request: Request, now_unix_ms: u64) -> Result<Response, Response> {
    Ok(match request {
        Request::Append {
            person,
            at_unix_ms,
            heard,
            said,
        } => {
            let id = store
                .append(&one(person)?, at_unix_ms, &heard, &said, now_unix_ms)
                .map_err(not_added)?;
            Response::Added { id }
        }
        // The legacy store keeps no links, and no `once` (#2176).
        Request::Remember {
            person,
            at_unix_ms,
            text,
            ..
        } => {
            let id = store
                .remember(&one(person)?, at_unix_ms, &text)
                .map_err(not_added)?;
            Response::Added { id }
        }
        Request::Kept { person, max_bytes } => {
            let room = usize::try_from(max_bytes.unwrap_or(KEPT_BYTES)).unwrap_or(usize::MAX);
            Response::Kept {
                entries: store.kept(&one(person)?, room).map_err(disk)?,
            }
        }
        Request::Recall {
            person,
            query,
            day,
            limit,
            max_bytes,
        } => {
            let person = one(person)?;
            let day =
                match day {
                    Some(day) => Some(Day::parse(&day).ok_or_else(|| {
                        refuse("bad_day", "a day is YYYY-MM-DD, from 1970-01-01 on")
                    })?),
                    None => None,
                };
            let ask = Ask {
                query: query.as_deref(),
                day,
                limit: limit.unwrap_or(RECALL_ENTRIES).min(MOST_RECALL_ENTRIES) as usize,
                max_bytes: max_bytes.unwrap_or(RECALL_BYTES).min(MOST_RECALL_BYTES) as usize,
            };
            Response::Recalled {
                entries: store.recall(&person, &ask).map_err(disk)?,
            }
        }
        Request::ForgetLast { person, kind } => Response::Forgot {
            removed: store.forget_last(&one(person)?, kind).map_err(disk)?,
        },
        Request::ForgetAll { person } => {
            let person = match person {
                Who::All => None,
                one_person => Some(one(one_person)?),
            };
            Response::Emptied {
                removed: store.forget_all(person.as_ref()).map_err(disk)?,
            }
        }
        Request::Status { person } => {
            let person = match person {
                Who::Unsaid | Who::All => None,
                Who::Id(id) => Some(person_of(&id)?),
            };
            Response::Status {
                people: store.status(person.as_ref()).map_err(disk)?,
            }
        }
    })
}

/// The line to send back for a request line, and the line to log.
pub struct Answer {
    pub line: String,
    pub log: String,
    /// The typed response reports a storage failure, not a caller refusal.
    /// This is internal health evidence and is not added to the wire or log.
    pub storage_fault: bool,
    /// A successful append, remember or append_linked committed a new record.
    /// Reads and removals do not establish that adding records works.
    pub wrote: bool,
}

/// Answer one request line.
pub fn answer(store: &mut Store, line: &[u8], now_unix_ms: u64) -> Answer {
    match Request::parse(line) {
        Ok(request) => {
            let asked = format!("{} person={}", request.op(), request.whose());
            let response = handle(store, request, now_unix_ms);
            Answer {
                line: response.line(),
                log: format!("{asked} {}", response.outcome()),
                storage_fault: matches!(
                    &response,
                    Response::Refused {
                        error: "storage",
                        ..
                    }
                ),
                wrote: matches!(&response, Response::Added { .. }),
            }
        }
        // A line that is no request has no op worth naming: what stands
        // where the op should be is the sender's text.
        Err(refusal) => Answer {
            line: refusal.line(),
            log: format!("? {}", refusal.outcome()),
            storage_fault: matches!(
                &refusal,
                Response::Refused {
                    error: "storage",
                    ..
                }
            ),
            wrote: false,
        },
    }
}

/// The answer to a line too long to be read.
pub fn too_long() -> Answer {
    let refusal = refuse(
        "too_long",
        format!("a request line is at most {MOST_LINE_BYTES} bytes"),
    );
    Answer {
        line: refusal.line(),
        log: format!("? {}", refusal.outcome()),
        storage_fault: false,
        wrote: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Options;
    use serde_json::{Value, json};
    use std::{fs, path::PathBuf};

    /// 2026-10-13 12:00 UTC.
    const NOON: u64 = 1_791_892_800_000;
    const DAY: u64 = 86_400_000;

    /// A directory of this test's own, with the store's directory in it, so
    /// that a path climbing out of the store would still land where the
    /// test can see it.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> (Self, Store) {
            let dir = std::env::temp_dir().join(format!("lm-{}-p-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            let scratch = Self(dir);
            let store = scratch.open(NOON);
            (scratch, store)
        }

        fn open(&self, now_unix_ms: u64) -> Store {
            let options = Options {
                dir: self.0.join("store"),
                keep_days: 30,
                local: crate::store::LocalTime::Offset(0),
            };
            Store::open(options, now_unix_ms).unwrap()
        }

        /// Everything under the scratch directory, as paths from it.
        fn everything(&self) -> Vec<String> {
            fn walk(dir: &std::path::Path, from: &std::path::Path, found: &mut Vec<String>) {
                for entry in fs::read_dir(dir).unwrap() {
                    let path = entry.unwrap().path();
                    found.push(path.strip_prefix(from).unwrap().display().to_string());
                    if path.is_dir() {
                        walk(&path, from, found);
                    }
                }
            }
            let mut found = Vec::new();
            walk(&self.0, &self.0, &mut found);
            found.sort();
            found
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Send one request line, as a JSON value, and read the answer line.
    fn ask(store: &mut Store, request: Value) -> Value {
        ask_at(store, request, NOON)
    }

    fn ask_at(store: &mut Store, request: Value, now_unix_ms: u64) -> Value {
        let answer = answer(store, request.to_string().as_bytes(), now_unix_ms);
        assert!(!answer.line.contains('\n'));
        serde_json::from_str(&answer.line).unwrap()
    }

    fn refusal(store: &mut Store, line: &[u8]) -> String {
        let answer: Value = serde_json::from_str(&answer(store, line, NOON).line).unwrap();
        assert_eq!(answer["ok"], false, "{answer}");
        assert!(answer["message"].is_string());
        answer["error"].as_str().unwrap().to_owned()
    }

    fn ids(answer: &Value) -> Vec<u64> {
        assert_eq!(answer["ok"], true, "{answer}");
        answer["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["id"].as_u64().unwrap())
            .collect()
    }

    #[test]
    fn only_successful_additions_report_write_evidence_without_changing_the_wire_or_log() {
        let (_scratch, mut store) = Scratch::new("answer-metadata");
        for (request, op, id) in [
            (
                json!({"op":"append","at_unix_ms":NOON,"heard":"hello"}),
                "append",
                1,
            ),
            (
                json!({"op":"remember","at_unix_ms":NOON,"text":"coffee"}),
                "remember",
                2,
            ),
        ] {
            let answer = answer(&mut store, request.to_string().as_bytes(), NOON);
            assert!(answer.wrote);
            assert!(!answer.storage_fault);
            assert_eq!(answer.line, format!(r#"{{"ok":true,"id":{id}}}"#));
            assert_eq!(answer.log, format!("{op} person=everyone ok id={id}"));
        }
        for request in [
            json!({"op":"kept"}),
            json!({"op":"recall"}),
            json!({"op":"status"}),
            json!({"op":"forget_last"}),
            json!({"op":"forget_all"}),
            json!({"op":"forget_last"}),
        ] {
            let answer = answer(&mut store, request.to_string().as_bytes(), NOON);
            assert!(!answer.wrote, "{request}");
            assert!(!answer.storage_fault, "{request}");
            let response: Value = serde_json::from_str(&answer.line).unwrap();
            assert_eq!(response["ok"], true, "{request}: {}", answer.line);
            assert!(response.get("wrote").is_none());
            assert!(response.get("storage_fault").is_none());
        }
    }

    /// #2176: the legacy store ignores the Days store's `once`, and keeps
    /// the same text twice as before.
    #[test]
    fn the_legacy_store_ignores_once() {
        let (_scratch, mut store) = Scratch::new("remember-once");
        for id in [1, 2] {
            let request = json!({"op":"remember","at_unix_ms":NOON,"text":"coffee","once":true});
            assert_eq!(ask(&mut store, request), json!({"ok":true,"id":id}));
        }
    }

    #[test]
    fn caller_refusals_and_overlong_lines_do_not_report_storage_or_write_evidence() {
        let (_scratch, mut store) = Scratch::new("refusal-metadata");
        for (request, code) in [
            ("not JSON".to_owned(), "bad_json"),
            (json!({"op":"missing"}).to_string(), "unknown_op"),
            (
                json!({"op":"remember","person":"../other","at_unix_ms":NOON,"text":"coffee"})
                    .to_string(),
                "bad_person",
            ),
            (
                json!({"op":"append","at_unix_ms":NOON,"heard":" "}).to_string(),
                "empty",
            ),
            (
                json!({"op":"remember","at_unix_ms":NOON,"text":" "}).to_string(),
                "empty",
            ),
        ] {
            let answer = answer(&mut store, request.as_bytes(), NOON);
            assert!(!answer.storage_fault, "{request}");
            assert!(!answer.wrote, "{request}");
            let response: Value = serde_json::from_str(&answer.line).unwrap();
            assert_eq!(response["error"], code);
        }
        let answer = too_long();
        assert!(!answer.storage_fault);
        assert!(!answer.wrote);
        assert_eq!(answer.log, "? refused error=too_long");
    }

    #[test]
    fn an_actual_store_write_failure_reports_storage_but_no_write_evidence() {
        let (scratch, mut store) = Scratch::new("storage-metadata");
        // Replacing the store directory with a regular file makes its child
        // paths unusable even when this test runs with elevated privileges.
        fs::rename(scratch.0.join("store"), scratch.0.join("saved")).unwrap();
        fs::write(scratch.0.join("store"), b"not a directory").unwrap();
        for request in [
            json!({"op":"append","at_unix_ms":NOON,"heard":"hello"}),
            json!({"op":"remember","at_unix_ms":NOON,"text":"coffee"}),
        ] {
            let answer = answer(&mut store, request.to_string().as_bytes(), NOON);
            assert!(answer.storage_fault, "{}", answer.line);
            assert!(!answer.wrote);
            let response: Value = serde_json::from_str(&answer.line).unwrap();
            assert_eq!(response["ok"], false);
            assert_eq!(response["error"], "storage");
            assert!(response.get("storage_fault").is_none());
            assert!(answer.log.contains("refused error=storage ("));
        }
    }

    #[test]
    fn every_default_request_written_in_the_document_is_read_and_done() {
        let (_scratch, mut store) = Scratch::new("document");
        let document = include_str!("../../../docs/memory.md");
        let (default, _) = document
            .split_once("### Additional requests with `--memory-days`")
            .expect("the document distinguishes default and opt-in requests");
        let requests = default
            .lines()
            .map(str::trim)
            .filter(|line| line.starts_with("{\"op\""));
        let mut ops = Vec::new();
        for line in requests {
            let request = Request::parse(line.as_bytes())
                .unwrap_or_else(|refusal| panic!("{line}: {refusal:?}"));
            ops.push(request.op());
            let answer = answer(&mut store, line.as_bytes(), NOON).line;
            assert!(answer.starts_with("{\"ok\":true,"), "{line}: {answer}");
        }
        // One example for each request there is, in the order of the code.
        assert_eq!(ops, OPS);
    }

    #[test]
    fn each_request_reads_from_its_line() {
        let parse = |line: &str| Request::parse(line.as_bytes()).unwrap();
        assert_eq!(
            parse(r#"{"op":"append","at_unix_ms":5,"heard":"hello","said":"hi"}"#),
            Request::Append {
                person: Who::Unsaid,
                at_unix_ms: 5,
                heard: "hello".into(),
                said: "hi".into(),
            }
        );
        assert_eq!(
            parse(r#"{"said":"hi","person":"mai","at_unix_ms":5,"op":"append"}"#),
            Request::Append {
                person: Who::Id("mai".into()),
                at_unix_ms: 5,
                heard: String::new(),
                said: "hi".into(),
            }
        );
        assert_eq!(
            parse(r#"{"op":"remember","at_unix_ms":5,"text":"black coffee"}"#),
            Request::Remember {
                person: Who::Unsaid,
                at_unix_ms: 5,
                text: "black coffee".into(),
                source_ids: vec![],
                once: false,
            }
        );
        // The Days store's: where it came from (#1693).
        assert_eq!(
            parse(r#"{"op":"remember","at_unix_ms":5,"text":"black coffee","source_ids":[3]}"#),
            Request::Remember {
                person: Who::Unsaid,
                at_unix_ms: 5,
                text: "black coffee".into(),
                source_ids: vec![3],
                once: false,
            }
        );
        assert_eq!(
            parse(r#"{"op":"kept","max_bytes":900}"#),
            Request::Kept {
                person: Who::Unsaid,
                max_bytes: Some(900),
            }
        );
        assert_eq!(
            parse(r#"{"op":"recall"}"#),
            Request::Recall {
                person: Who::Unsaid,
                query: None,
                day: None,
                limit: None,
                max_bytes: None,
            }
        );
        assert_eq!(
            parse(
                r#"{"op":"recall","query":"coffee","day":"2026-10-13","limit":3,"max_bytes":100}"#
            ),
            Request::Recall {
                person: Who::Unsaid,
                query: Some("coffee".into()),
                day: Some("2026-10-13".into()),
                limit: Some(3),
                max_bytes: Some(100),
            }
        );
        assert_eq!(
            parse(r#"{"op":"recall","query":null,"day":null}"#),
            parse(r#"{"op":"recall"}"#)
        );
        assert_eq!(
            parse(r#"{"op":"forget_last"}"#),
            Request::ForgetLast {
                person: Who::Unsaid,
                kind: None,
            }
        );
        assert_eq!(
            parse(r#"{"op":"forget_all","person":null}"#),
            Request::ForgetAll { person: Who::All }
        );
        assert_eq!(
            parse(r#"{"op":"forget_all"}"#),
            Request::ForgetAll {
                person: Who::Unsaid
            }
        );
        assert_eq!(
            parse(" {\"op\":\"status\"} \r"),
            Request::Status {
                person: Who::Unsaid
            }
        );
    }

    #[test]
    fn a_line_that_is_no_request_is_refused_by_name_and_nothing_is_written() {
        let (scratch, mut store) = Scratch::new("refused");
        let lines: &[(&[u8], &str)] = &[
            (b"", "bad_json"),
            (b"hello lamp", "bad_json"),
            (b"{\"op\":\"status\"", "bad_json"),
            (b"{\"op\":\"status\"} {}", "bad_json"),
            (
                b"{\"op\":\"remember\",\"at_unix_ms\":1,\"text\":\"\xff\xfe\"}",
                "bad_json",
            ),
            (b"[]", "bad_request"),
            (b"\"status\"", "bad_request"),
            (b"{}", "bad_request"),
            (b"{\"op\":7}", "bad_request"),
            (b"{\"heard\":\"no op\"}", "bad_request"),
            (b"{\"op\":\"forget\"}", "unknown_op"),
            (b"{\"op\":\"Append\"}", "unknown_op"),
            (b"{\"op\":\"\"}", "unknown_op"),
            (
                b"{\"op\":\"append\",\"heard\":\"no time\",\"said\":\"x\"}",
                "bad_request",
            ),
            (
                b"{\"op\":\"append\",\"at_unix_ms\":-1,\"heard\":\"x\"}",
                "bad_request",
            ),
            (
                b"{\"op\":\"append\",\"at_unix_ms\":1.5,\"heard\":\"x\"}",
                "bad_request",
            ),
            (
                b"{\"op\":\"append\",\"at_unix_ms\":1,\"heard\":7}",
                "bad_request",
            ),
            (
                b"{\"op\":\"append\",\"at_unix_ms\":1,\"herd\":\"a slip\"}",
                "bad_request",
            ),
            (b"{\"op\":\"remember\",\"at_unix_ms\":1}", "bad_request"),
            (b"{\"op\":\"recall\",\"limit\":\"three\"}", "bad_request"),
            (b"{\"op\":\"status\",\"person\":7}", "bad_request"),
            (
                b"{\"op\":\"append\",\"at_unix_ms\":1,\"heard\":\" \",\"said\":\"\"}",
                "empty",
            ),
            (b"{\"op\":\"append\",\"at_unix_ms\":1}", "empty"),
            (
                b"{\"op\":\"remember\",\"at_unix_ms\":1,\"text\":\"\\n\\t\"}",
                "empty",
            ),
            (
                b"{\"op\":\"append\",\"at_unix_ms\":18446744073709551615,\"said\":\"x\"}",
                "bad_time",
            ),
            (
                b"{\"op\":\"remember\",\"at_unix_ms\":253402300800000,\"text\":\"x\"}",
                "bad_time",
            ),
            (b"{\"op\":\"recall\",\"day\":\"yesterday\"}", "bad_day"),
            (b"{\"op\":\"recall\",\"day\":\"2026-02-30\"}", "bad_day"),
            (b"{\"op\":\"recall\",\"day\":\"../../etc\"}", "bad_day"),
        ];
        for (line, error) in lines {
            assert_eq!(
                refusal(&mut store, line),
                *error,
                "{}",
                String::from_utf8_lossy(line)
            );
        }
        assert_eq!(scratch.everything(), ["store"]);
        assert_eq!(too_long().log, "? refused error=too_long");
        let line: Value = serde_json::from_str(&too_long().line).unwrap();
        assert_eq!(
            (&line["ok"], &line["error"]),
            (&json!(false), &json!("too_long"))
        );
        // The connection is as good as it was: the next request is done.
        let added = ask(
            &mut store,
            json!({"op":"remember","at_unix_ms":NOON,"text":"still here"}),
        );
        assert_eq!(added, json!({"ok":true,"id":1}));
    }

    #[test]
    fn forgetting_can_be_asked_for_the_last_of_one_kind() {
        let (_scratch, mut store) = Scratch::new("forget-kind");
        // The last of one kind, whatever else came after it.
        ask(
            &mut store,
            json!({"op":"remember","at_unix_ms":NOON,"text":"keep"}),
        );
        ask(
            &mut store,
            json!({"op":"append","at_unix_ms":NOON,"heard":"later","said":"yes"}),
        );
        let forgot = ask(&mut store, json!({"op":"forget_last","kind":"kept"}));
        assert_eq!(forgot["removed"]["kind"], "kept", "{forgot}");
        let forgot = ask(&mut store, json!({"op":"forget_last","kind":"kept"}));
        assert_eq!(forgot, json!({"ok":true,"removed":null}));
        let forgot = ask(&mut store, json!({"op":"forget_last","kind":"exchange"}));
        assert_eq!(forgot["removed"]["kind"], "exchange", "{forgot}");
        let refused = answer(&mut store, br#"{"op":"forget_last","kind":"all"}"#, NOON).line;
        assert!(
            refused.starts_with(r#"{"ok":false,"error":"bad_request","message":""#),
            "{refused}"
        );
    }

    #[test]
    fn what_is_added_is_recalled_kept_counted_and_forgotten_in_these_shapes() {
        let (_scratch, mut store) = Scratch::new("shapes");
        let exchange = json!({"op":"append","at_unix_ms":NOON,"heard":"How do I take\n my coffee?","said":"Black."});
        let added = answer(&mut store, exchange.to_string().as_bytes(), NOON).line;
        assert_eq!(added, r#"{"ok":true,"id":1}"#);
        let added = ask(
            &mut store,
            json!({"op":"remember","at_unix_ms":NOON + 1,"text":"  cà phê sữa đá on Fridays "}),
        );
        assert_eq!(added, json!({"ok":true,"id":2}));

        let line = answer(
            &mut store,
            br#"{"op":"recall","query":"coffee fridays"}"#,
            NOON,
        )
        .line;
        assert_eq!(
            line,
            format!(
                r#"{{"ok":true,"entries":[{{"kind":"exchange","id":1,"at_unix_ms":{NOON},"heard":"How do I take my coffee?","said":"Black."}},{{"kind":"kept","id":2,"at_unix_ms":{},"text":"cà phê sữa đá on Fridays"}}]}}"#,
                NOON + 1
            )
        );
        let kept = ask(&mut store, json!({"op":"kept","max_bytes":4096}));
        assert_eq!(
            kept,
            json!({"ok":true,"entries":[{"id":2,"at_unix_ms":NOON + 1,"text":"cà phê sữa đá on Fridays"}]})
        );
        let day = ask(&mut store, json!({"op":"recall","day":"2026-10-13"}));
        assert_eq!(ids(&day), [1]);
        assert_eq!(day["entries"][0]["kind"], "exchange");
        assert_eq!(ids(&ask(&mut store, json!({"op":"recall"}))), [1]);
        assert_eq!(
            ids(&ask(&mut store, json!({"op":"recall","day":"2026-10-12"}))),
            [0u64; 0]
        );

        let status = ask(&mut store, json!({"op":"status"}));
        let bytes = status["people"][0]["bytes"].as_u64().unwrap();
        assert!(bytes > 100);
        assert_eq!(
            status,
            json!({"ok":true,"people":[{
                "person":"everyone","exchanges":1,"kept":1,"bytes":bytes,
                "oldest_day":"2026-10-13","newest_day":"2026-10-13","skipped_lines":0
            }]})
        );

        // What was forgotten is named by kind, id and time, never by text.
        let line = answer(&mut store, br#"{"op":"forget_last"}"#, NOON).line;
        assert_eq!(
            line,
            format!(
                r#"{{"ok":true,"removed":{{"kind":"kept","id":2,"at_unix_ms":{}}}}}"#,
                NOON + 1
            )
        );
        let forgot = ask(&mut store, json!({"op":"forget_last"}));
        assert_eq!(
            forgot,
            json!({"ok":true,"removed":{"kind":"exchange","id":1,"at_unix_ms":NOON}})
        );
        let forgot = answer(&mut store, br#"{"op":"forget_last"}"#, NOON).line;
        assert_eq!(forgot, r#"{"ok":true,"removed":null}"#);
        let refused = answer(&mut store, br#"{"op":"forget"}"#, NOON).line;
        assert!(
            refused.starts_with(r#"{"ok":false,"error":"unknown_op","message":""#),
            "{refused}"
        );
        assert_eq!(
            ask(&mut store, json!({"op":"status"})),
            json!({"ok":true,"people":[]})
        );
        assert_eq!(
            ask(&mut store, json!({"op":"kept"})),
            json!({"ok":true,"entries":[]})
        );
        assert_eq!(
            handle(
                &mut store,
                Request::ForgetLast {
                    person: Who::Unsaid,
                    kind: None,
                },
                NOON
            ),
            Response::Forgot { removed: None }
        );
        let removed = Removed {
            kind: Kind::Kept,
            id: 3,
            at_unix_ms: 7,
        };
        ask(
            &mut store,
            json!({"op":"remember","at_unix_ms":7,"text":"x"}),
        );
        assert_eq!(
            handle(
                &mut store,
                Request::ForgetLast {
                    person: Who::Unsaid,
                    kind: None,
                },
                NOON
            ),
            Response::Forgot {
                removed: Some(removed)
            }
        );
    }

    #[test]
    fn a_bad_person_id_is_refused_and_nothing_on_the_disk_is_touched() {
        let (scratch, mut store) = Scratch::new("ids");
        let long = "x".repeat(33);
        let bad = [
            "../x",
            "",
            "Everyone",
            long.as_str(),
            "..",
            ".",
            "/tmp/x",
            "a/b",
            "mai lan",
            "x\u{0}",
            "đá",
        ];
        for id in bad {
            for request in [
                json!({"op":"append","person":id,"at_unix_ms":NOON,"heard":"hello","said":"hi"}),
                json!({"op":"remember","person":id,"at_unix_ms":NOON,"text":"a fact"}),
                json!({"op":"kept","person":id}),
                json!({"op":"recall","person":id}),
                json!({"op":"recall","person":id,"query":"hello","day":"2026-10-13"}),
                json!({"op":"forget_last","person":id}),
                json!({"op":"forget_all","person":id}),
                json!({"op":"status","person":id}),
            ] {
                let answer = answer(&mut store, request.to_string().as_bytes(), NOON);
                let line: Value = serde_json::from_str(&answer.line).unwrap();
                assert_eq!(line["ok"], false, "{request}");
                assert_eq!(line["error"], "bad_person", "{request}");
                // Nor is what was sent as an id written to the log.
                let op = request["op"].as_str().unwrap();
                assert_eq!(
                    answer.log,
                    format!("{op} person=? refused error=bad_person")
                );
            }
            // Not in the store, and not beside it either.
            assert_eq!(scratch.everything(), ["store"], "{id:?}");
        }
        // With something there to lose, a bad id still loses nothing.
        ask(
            &mut store,
            json!({"op":"append","at_unix_ms":NOON,"heard":"hello","said":"hi"}),
        );
        fs::write(scratch.0.join("x"), "beside the store").unwrap();
        let before = scratch.everything();
        for id in bad {
            let forgot = ask(&mut store, json!({"op":"forget_all","person":id}));
            assert_eq!(forgot["error"], "bad_person");
        }
        assert_eq!(scratch.everything(), before);
        assert_eq!(ids(&ask(&mut store, json!({"op":"recall"}))), [1]);
        // The longest id there is, and the shortest.
        let longest = "x".repeat(32);
        for id in [longest.as_str(), "7", "-", "p_2-b"] {
            let added = ask(
                &mut store,
                json!({"op":"remember","person":id,"at_unix_ms":NOON,"text":"ok"}),
            );
            assert_eq!(added, json!({"ok":true,"id":1}), "{id}");
        }
    }

    #[test]
    fn each_person_has_a_memory_apart_and_null_means_every_person_only_where_that_is_safe() {
        let (_scratch, mut store) = Scratch::new("people");
        for (person, text) in [("an", "tulip"), ("binh", "rose")] {
            let said = json!({"op":"append","person":person,"at_unix_ms":NOON,"heard":text,"said":"noted"});
            assert_eq!(ask(&mut store, said)["id"], 1);
            let kept = json!({"op":"remember","person":person,"at_unix_ms":NOON,"text":text});
            assert_eq!(ask(&mut store, kept)["id"], 2);
        }
        ask(
            &mut store,
            json!({"op":"append","at_unix_ms":NOON,"heard":"daisy","said":"noted"}),
        );

        // No request for one person gives another's, whatever is asked.
        for request in [
            json!({"op":"recall","person":"binh","query":"tulip rose daisy noted"}),
            json!({"op":"recall","person":"binh"}),
            json!({"op":"recall","person":"binh","day":"2026-10-13"}),
            json!({"op":"kept","person":"binh"}),
            json!({"op":"status","person":"binh"}),
        ] {
            let line = ask(&mut store, request).to_string();
            assert!(line.contains("\"ok\":true"));
            assert!(!line.contains("tulip") && !line.contains("daisy") && !line.contains("\"an\""));
        }
        // With nobody named it is the memory called everyone, which is one
        // person's like any other, not the sum of them.
        let everyone = ask(
            &mut store,
            json!({"op":"recall","query":"tulip rose daisy noted"}),
        );
        assert_eq!(ids(&everyone), [1]);
        assert_eq!(everyone["entries"][0]["heard"], "daisy");

        // Null would mean all of them at once. Nothing that reads or adds
        // takes it.
        for request in [
            json!({"op":"append","person":null,"at_unix_ms":NOON,"heard":"hello","said":"hi"}),
            json!({"op":"remember","person":null,"at_unix_ms":NOON,"text":"a fact"}),
            json!({"op":"kept","person":null}),
            json!({"op":"recall","person":null}),
            json!({"op":"forget_last","person":null}),
        ] {
            let refused = ask(&mut store, request);
            assert_eq!(
                (&refused["ok"], &refused["error"]),
                (&json!(false), &json!("bad_person"))
            );
        }
        let people = |status: Value| -> Vec<String> {
            let people = status["people"].as_array().unwrap().iter();
            people
                .map(|person| person["person"].as_str().unwrap().to_owned())
                .collect()
        };
        assert_eq!(
            people(ask(&mut store, json!({"op":"status"}))),
            ["an", "binh", "everyone"]
        );
        assert_eq!(
            people(ask(&mut store, json!({"op":"status","person":null}))),
            ["an", "binh", "everyone"]
        );
        assert_eq!(
            people(ask(&mut store, json!({"op":"status","person":"an"}))),
            ["an"]
        );
        assert_eq!(
            people(ask(&mut store, json!({"op":"status","person":"nobody"}))),
            [""; 0]
        );

        // Forgetting: the last thing of one, all of one, all of everyone.
        let forgot = ask(&mut store, json!({"op":"forget_last","person":"an"}));
        assert_eq!(
            forgot["removed"],
            json!({"kind":"kept","id":2,"at_unix_ms":NOON})
        );
        assert_eq!(
            ask(&mut store, json!({"op":"forget_all","person":"an"})),
            json!({"ok":true,"removed":1})
        );
        assert_eq!(
            people(ask(&mut store, json!({"op":"status"}))),
            ["binh", "everyone"]
        );
        assert_eq!(
            ids(&ask(
                &mut store,
                json!({"op":"recall","person":"binh","query":"rose"})
            )),
            [1, 2]
        );
        // Unnamed, forget_all is for the one called everyone.
        assert_eq!(
            ask(&mut store, json!({"op":"forget_all"})),
            json!({"ok":true,"removed":1})
        );
        assert_eq!(people(ask(&mut store, json!({"op":"status"}))), ["binh"]);
        ask(
            &mut store,
            json!({"op":"append","at_unix_ms":NOON,"heard":"daisy","said":"noted"}),
        );
        assert_eq!(
            ask(&mut store, json!({"op":"forget_all","person":null})),
            json!({"ok":true,"removed":3})
        );
        assert_eq!(
            ask(&mut store, json!({"op":"status"})),
            json!({"ok":true,"people":[]})
        );
        assert_eq!(
            ask(&mut store, json!({"op":"forget_all","person":null})),
            json!({"ok":true,"removed":0})
        );
    }

    #[test]
    fn a_recall_gives_eight_entries_unless_told_and_never_more_than_fifty_or_sixteen_kibibytes() {
        let (_scratch, mut store) = Scratch::new("most");
        // Sixty exchanges of 400 bytes of text each.
        let said = "s".repeat(397);
        for n in 0..60u64 {
            let exchange = json!({"op":"append","at_unix_ms":NOON + n,"heard":"ask","said":said});
            assert_eq!(ask(&mut store, exchange)["id"], n + 1);
        }
        let count = |store: &mut Store, request: Value| ids(&ask(store, request)).len();
        assert_eq!(
            ids(&ask(&mut store, json!({"op":"recall"}))),
            (53..=60).collect::<Vec<u64>>()
        );
        // 4096 bytes are ten of them; 16384 are forty.
        assert_eq!(count(&mut store, json!({"op":"recall","limit":50})), 10);
        assert_eq!(
            count(
                &mut store,
                json!({"op":"recall","limit":50,"max_bytes":16384})
            ),
            40
        );
        assert_eq!(
            count(
                &mut store,
                json!({"op":"recall","limit":50,"max_bytes":1000000})
            ),
            40
        );
        assert_eq!(
            count(
                &mut store,
                json!({"op":"recall","limit":20,"max_bytes":16384})
            ),
            20
        );
        assert_eq!(count(&mut store, json!({"op":"recall","limit":1})), 1);
        assert_eq!(count(&mut store, json!({"op":"recall","limit":0})), 0);
        assert_eq!(count(&mut store, json!({"op":"recall","max_bytes":399})), 0);
        assert_eq!(
            count(
                &mut store,
                json!({"op":"recall","query":"ask","day":"2026-10-13"})
            ),
            8
        );
        // Forty bytes of text each, sixty of them: the count is what stops
        // these, at fifty however many are asked for.
        for n in 0..60u64 {
            ask(
                &mut store,
                json!({"op":"append","person":"small","at_unix_ms":NOON + n,"heard":"h".repeat(20),"said":"s".repeat(20)}),
            );
        }
        assert_eq!(
            count(
                &mut store,
                json!({"op":"recall","person":"small","limit":50})
            ),
            50
        );
        assert_eq!(
            count(
                &mut store,
                json!({"op":"recall","person":"small","limit":4000000000u64})
            ),
            50
        );
        assert_eq!(
            count(
                &mut store,
                json!({"op":"recall","person":"small","query":"hhhhhhhhhhhhhhhhhhhh","limit":51})
            ),
            50
        );

        // What is kept is given by bytes alone: 4096 unless told.
        for n in 0..30u64 {
            ask(
                &mut store,
                json!({"op":"remember","at_unix_ms":NOON,"text":format!("{n:0>300}")}),
            );
        }
        assert_eq!(count(&mut store, json!({"op":"kept"})), 13);
        assert_eq!(count(&mut store, json!({"op":"kept","max_bytes":600})), 2);
        assert_eq!(
            count(&mut store, json!({"op":"kept","max_bytes":100000})),
            30
        );
    }

    #[test]
    fn the_first_request_of_a_new_day_deletes_the_days_too_old_whatever_it_asks() {
        let (scratch, mut store) = Scratch::new("turn");
        let old =
            json!({"op":"append","at_unix_ms":NOON - 30 * DAY,"heard":"a month ago","said":"yes"});
        ask(&mut store, old);
        ask(
            &mut store,
            json!({"op":"append","at_unix_ms":NOON,"heard":"today","said":"yes"}),
        );
        assert_eq!(
            ids(&ask_at(
                &mut store,
                json!({"op":"recall"}),
                NOON + DAY / 2 - 1
            )),
            [1, 2]
        );
        // Midnight: thirty-one days on from the first.
        assert_eq!(
            ids(&ask_at(&mut store, json!({"op":"recall"}), NOON + DAY / 2)),
            [2]
        );
        assert_eq!(
            scratch.everything(),
            [
                "store",
                "store/everyone",
                "store/everyone/journal-2026-10-13.jsonl"
            ]
        );
    }

    #[test]
    fn the_log_line_names_the_request_the_person_and_the_counts_and_never_the_text() {
        let (_scratch, mut store) = Scratch::new("log");
        let mut logged = Vec::new();
        let mut send = |store: &mut Store, line: &str| {
            let answer = answer(store, line.as_bytes(), NOON);
            logged.push(answer.log.clone());
            answer.log
        };
        assert_eq!(
            send(
                &mut store,
                r#"{"op":"append","at_unix_ms":1791892800000,"heard":"my pin is zebra","said":"zebra it is"}"#
            ),
            "append person=everyone ok id=1"
        );
        assert_eq!(
            send(
                &mut store,
                r#"{"op":"remember","person":"mai","at_unix_ms":1791892800000,"text":"zebra"}"#
            ),
            "remember person=mai ok id=1"
        );
        assert_eq!(
            send(&mut store, r#"{"op":"kept","person":"mai"}"#),
            "kept person=mai ok entries=1 bytes=5"
        );
        assert_eq!(
            send(&mut store, r#"{"op":"recall","query":"zebra"}"#),
            "recall person=everyone ok entries=1 bytes=26"
        );
        assert_eq!(
            send(
                &mut store,
                r#"{"op":"recall","person":"zebra zebra","query":"zebra"}"#
            ),
            "recall person=? refused error=bad_person"
        );
        let status = send(&mut store, r#"{"op":"status"}"#);
        assert!(
            status.starts_with("status person=* ok people=2 exchanges=1 kept=1 bytes="),
            "{status}"
        );
        assert!(status.ends_with(" skipped_lines=0"), "{status}");
        assert_eq!(
            send(&mut store, r#"{"op":"zebra"}"#),
            "? refused error=unknown_op"
        );
        assert_eq!(
            send(&mut store, r#"{"op":"append","zebra":1}"#),
            "? refused error=bad_request"
        );
        assert_eq!(
            send(&mut store, r#"{"op":"append","at_unix_ms":"zebra"}"#),
            "? refused error=bad_request"
        );
        assert_eq!(send(&mut store, "zebra"), "? refused error=bad_json");
        assert_eq!(
            send(&mut store, r#"{"op":"recall","day":"zebra"}"#),
            "recall person=everyone refused error=bad_day"
        );
        assert_eq!(
            send(
                &mut store,
                r#"{"op":"append","at_unix_ms":1,"heard":" ","said":""}"#
            ),
            "append person=everyone refused error=empty"
        );
        assert_eq!(
            send(&mut store, r#"{"op":"forget_last","person":"mai"}"#),
            "forget_last person=mai ok removed=1 kind=kept id=1"
        );
        assert_eq!(
            send(&mut store, r#"{"op":"forget_last","person":"mai"}"#),
            "forget_last person=mai ok removed=0"
        );
        assert_eq!(
            send(&mut store, r#"{"op":"forget_all","person":null}"#),
            "forget_all person=* ok removed=1"
        );
        assert_eq!(
            send(&mut store, r#"{"op":"forget_all"}"#),
            "forget_all person=everyone ok removed=0"
        );
        assert_eq!(logged.len(), 16);
        for line in logged {
            assert!(!line.contains("zebra") && !line.contains('\n'), "{line}");
        }
    }
}
