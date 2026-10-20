//! Opt-in requests, plus the unchanged legacy response shapes. Model payloads
//! are data for an offline caller; no request here invokes inference.
use crate::{
    days::{Days, Error, Result, Wider},
    protocol::{self, Request, Response, Who},
    store::{Ask, Day, Person},
    tidy,
};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Extra {
    AppendLinked {
        #[serde(default)]
        person: Who,
        at_unix_ms: u64,
        #[serde(default)]
        heard: String,
        #[serde(default)]
        said: String,
        source_ids: Vec<u64>,
        /// Rules of the person's own rules scope the answer may have
        /// followed (#813): forgetting one removes this exchange.
        #[serde(default)]
        rule_ids: Vec<u64>,
    },
    Search {
        #[serde(default)]
        person: Who,
        query: String,
        #[serde(default)]
        day: Option<String>,
        #[serde(default)]
        limit: Option<u64>,
        #[serde(default)]
        max_bytes: Option<u64>,
        #[serde(default)]
        history: bool,
        #[serde(default)]
        day_from: Option<String>,
        #[serde(default)]
        day_to: Option<String>,
        #[serde(default)]
        recall_v2: bool,
    },
    /// The legacy `recall` with any of #2176's fields: a span of days
    /// (`day_from`, `day_to`) or `recall_v2`. Without them a recall is the
    /// legacy request, answered as it always was.
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
        #[serde(default)]
        day_from: Option<String>,
        #[serde(default)]
        day_to: Option<String>,
        #[serde(default)]
        recall_v2: bool,
    },
    Forget {
        #[serde(default)]
        person: Who,
        id: u64,
    },
    SessionOpen {
        #[serde(default)]
        person: Who,
        session: String,
        #[serde(default)]
        max_bytes: Option<u64>,
    },
    SessionClose {
        #[serde(default)]
        person: Who,
        session: String,
    },
    /// #1693 (3): one open thread to ask about once (`Days::follow_up`).
    FollowUp {
        #[serde(default)]
        person: Who,
    },
    ConversationEnd {
        #[serde(default)]
        person: Who,
        conversation: String,
        source_ids: Vec<u64>,
        #[serde(default)]
        previous_ids: Vec<u64>,
        #[serde(default)]
        submit: bool,
    },
    TidyApply {
        #[serde(default)]
        person: Who,
        job: String,
        observations: Vec<tidy::Proposal>,
    },
}

fn one(person: Who) -> Result<Person> {
    match person {
        Who::Unsaid => Ok(Person::everyone()),
        Who::Id(id) => Person::new(&id)
            .ok_or_else(|| Error::new("bad_person", "a person ID is 1 to 32 of a-z, 0-9, _ and -")),
        Who::All => Err(Error::new(
            "bad_person",
            "only status and forget_all accept a null person",
        )),
    }
}
fn some(person: Who, unsaid_all: bool) -> Result<Option<Person>> {
    match person {
        Who::All => Ok(None),
        Who::Unsaid if unsaid_all => Ok(None),
        p => one(p).map(Some),
    }
}
fn ask<'a>(
    query: Option<&'a str>,
    day: Option<&str>,
    limit: Option<u64>,
    bytes: Option<u64>,
) -> Result<Ask<'a>> {
    let day = day
        .map(|d| {
            Day::parse(d)
                .ok_or_else(|| Error::new("bad_day", "a day is YYYY-MM-DD from 1970 through 9999"))
        })
        .transpose()?;
    Ok(Ask {
        query,
        day,
        limit: limit.unwrap_or(8).min(50) as usize,
        max_bytes: bytes.unwrap_or(4096).min(16384) as usize,
    })
}
/// The most days a span names, both ends counted (#2176): "last week" is
/// seven.
const SPAN_DAYS: u32 = 14;

/// The days a request names: one `day`, or a span from `day_from` to
/// `day_to`, both ends counted, oldest first and at most [`SPAN_DAYS`].
/// The ask's day is the span's first; [`Wider::last_day`] its last.
fn span(
    day: Option<String>,
    from: Option<String>,
    to: Option<String>,
) -> Result<(Option<String>, Option<Day>)> {
    let bad = || {
        Error::new(
            "bad_day",
            "a span is day_from and day_to, YYYY-MM-DD, oldest first, at most 14 days, without day",
        )
    };
    match (day, from, to) {
        (day, None, None) => Ok((day, None)),
        (None, Some(from), Some(to)) => {
            let (first, last) = (
                Day::parse(&from).ok_or_else(bad)?,
                Day::parse(&to).ok_or_else(bad)?,
            );
            if first > last || last.before(SPAN_DAYS - 1).is_some_and(|edge| first < edge) {
                return Err(bad());
            }
            Ok((Some(from), Some(last)))
        }
        _ => Err(bad()),
    }
}
fn value(response: Response) -> Value {
    serde_json::to_value(response).expect("the response contains JSON values only")
}

fn legacy(store: &mut Days, request: Request) -> Result<Value> {
    Ok(match request {
        Request::Append {
            person,
            at_unix_ms,
            heard,
            said,
        } => value(Response::Added {
            id: store.append(&one(person)?, at_unix_ms, &heard, &said, &[])?,
        }),
        Request::Remember {
            person,
            at_unix_ms,
            text,
            source_ids,
            once: false,
        } => value(Response::Added {
            id: store.remember_from(&one(person)?, at_unix_ms, &text, &source_ids)?,
        }),
        // #2176: what this person already keeps (the same words, ignoring
        // case, spacing and a final punctuation mark) is that kept thing; no
        // entry is written, and it gains these `source_ids`.
        Request::Remember {
            person,
            at_unix_ms,
            text,
            source_ids,
            once: true,
        } => match store.remember_once_from(&one(person)?, at_unix_ms, &text, &source_ids, true)? {
            (id, true) => json!({"ok":true,"id":id,"existing":true}),
            (id, false) => value(Response::Added { id }),
        },
        Request::Kept { person, max_bytes } => value(Response::Kept {
            entries: store.kept(&one(person)?, max_bytes.unwrap_or(4096).min(16384) as usize)?,
        }),
        Request::Recall {
            person,
            query,
            day,
            limit,
            max_bytes,
        } => value(Response::Recalled {
            entries: store.recall(
                &one(person)?,
                &ask(query.as_deref(), day.as_deref(), limit, max_bytes)?,
            )?,
        }),
        Request::ForgetLast { person, kind } => value(Response::Forgot {
            removed: store.forget_last(&one(person)?, kind)?,
        }),
        Request::ForgetAll { person } => value(Response::Emptied {
            removed: store.forget_all(some(person, false)?.as_ref())?,
        }),
        Request::Status { person } => value(Response::Status {
            people: store.status(some(person, true)?.as_ref())?,
        }),
    })
}
fn extra(store: &mut Days, request: Extra) -> Result<Value> {
    Ok(match request {
        Extra::AppendLinked {
            person,
            at_unix_ms,
            heard,
            said,
            source_ids,
            rule_ids,
        } => {
            json!({"ok":true,"id":store.append_ruled(&one(person)?,at_unix_ms,&heard,&said,&source_ids,&rule_ids)?})
        }
        Extra::Search {
            person,
            query,
            day,
            limit,
            max_bytes,
            history,
            day_from,
            day_to,
            recall_v2,
        } => {
            let (day, last_day) = span(day, day_from, day_to)?;
            let wider = Wider {
                last_day,
                second: recall_v2,
            };
            json!({"ok":true,"entries":store.search_wider(&one(person)?,&ask(Some(&query),day.as_deref(),limit,max_bytes)?,history,wider)?})
        }
        Extra::Recall {
            person,
            query,
            day,
            limit,
            max_bytes,
            day_from,
            day_to,
            recall_v2,
        } => {
            let (day, last_day) = span(day, day_from, day_to)?;
            let wider = Wider {
                last_day,
                second: recall_v2,
            };
            value(Response::Recalled {
                entries: store.recall_wider(
                    &one(person)?,
                    &ask(query.as_deref(), day.as_deref(), limit, max_bytes)?,
                    wider,
                )?,
            })
        }
        Extra::Forget { person, id } => {
            json!({"ok":true,"removed":store.forget(&one(person)?,id)?})
        }
        Extra::SessionOpen {
            person,
            session,
            max_bytes,
        } => {
            json!({"ok":true,"snapshot":store.session_open(&one(person)?,&session,max_bytes.unwrap_or(1200).min(1200) as usize)?})
        }
        Extra::SessionClose { person, session } => {
            store.session_close(&one(person)?, &session)?;
            json!({"ok":true})
        }
        Extra::FollowUp { person } => {
            json!({"ok":true,"follow_up":store.follow_up(&one(person)?)?})
        }
        Extra::ConversationEnd {
            person,
            conversation,
            source_ids,
            previous_ids,
            submit,
        } => {
            let person = one(person)?;
            if submit {
                store.validate_conversation_submission(
                    &person,
                    &conversation,
                    &source_ids,
                    &previous_ids,
                )?;
                // There is no adapter to take this work. Do not fill the
                // preparation queue or build model input that nobody uses.
                json!({"ok":true,"submitted":false,"reason":"model_unconfigured"})
            } else {
                json!({"ok":true,"input":store.conversation_end_with(&person,&conversation,&source_ids,&previous_ids)?})
            }
        }
        Extra::TidyApply {
            person,
            job,
            observations,
        } => {
            json!({"ok":true,"ids":store.tidy_apply(&one(person)?,&job,&tidy::Output{observations})?})
        }
    })
}

pub fn answer(store: &mut Days, line: &[u8], now: u64) -> protocol::Answer {
    let mut op = "?".to_owned();
    let mut who = "?".to_owned();
    let result = (|| -> Result<Value> {
        if line.len() > protocol::MOST_LINE_BYTES {
            return Err(Error::new("too_long", "a request line is at most 64 KiB"));
        }
        store.maintain(now)?;
        let parsed: Value = serde_json::from_slice(line)
            .map_err(|_| Error::new("bad_json", "the request is not JSON"))?;
        let name = parsed
            .get("op")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new("bad_request", "a request needs an op"))?;
        const NEW: &[&str] = &[
            "append_linked",
            "search",
            "forget",
            "session_open",
            "session_close",
            "follow_up",
            "conversation_end",
            "tidy_apply",
        ];
        const OLD: &[&str] = &[
            "append",
            "remember",
            "kept",
            "recall",
            "forget_last",
            "forget_all",
            "status",
        ];
        if !NEW.contains(&name) && !OLD.contains(&name) {
            return Err(Error::new("unknown_op", "the memory operation is unknown"));
        }
        op = name.into();
        who = match parsed.get("person") {
            None => Person::EVERYONE.into(),
            Some(Value::Null) => "*".into(),
            Some(Value::String(s)) if Person::new(s).is_some() => s.clone(),
            _ => "?".into(),
        };
        // #2176: a recall with a span or the second version is the new one.
        let wider = name == "recall"
            && ["day_from", "day_to", "recall_v2"]
                .iter()
                .any(|field| parsed.get(field).is_some());
        if NEW.contains(&name) || wider {
            extra(
                store,
                serde_json::from_value(parsed)
                    .map_err(|_| Error::new("bad_request", "invalid fields for this operation"))?,
            )
        } else {
            legacy(
                store,
                serde_json::from_value(parsed)
                    .map_err(|_| Error::new("bad_request", "invalid fields for this operation"))?,
            )
        }
    })();
    let (response, outcome, storage_fault, wrote) = match result {
        Ok(response) => {
            let count = response
                .get("entries")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            let outcome = format!("ok entries={count}");
            // Reads, maintenance and successful no-op removals must not
            // clear an earlier failure to add a record.
            let wrote = matches!(op.as_str(), "append" | "remember" | "append_linked")
                && response["existing"] != true;
            (response, outcome, false, wrote)
        }
        Err(e) => (
            json!({"ok":false,"error":e.code,"message":e.message}),
            format!("refused error={}", e.code),
            e.code == "storage",
            false,
        ),
    };
    protocol::Answer {
        line: response.to_string(),
        log: format!("{op} person={who} {outcome}"),
        storage_fault,
        wrote,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{days::DATABASE, store::Options};
    use std::{fs, path::PathBuf};

    const NOW: u64 = 1_791_892_800_000;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> (Self, Days) {
            let dir = std::env::temp_dir()
                .join(format!("lamp-days-protocol-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            let scratch = Self(dir);
            let store = scratch.open();
            (scratch, store)
        }

        fn open(&self) -> Days {
            Days::open(
                Options {
                    dir: self.0.clone(),
                    keep_days: 30,
                    local: crate::store::LocalTime::Offset(0),
                },
                NOW,
            )
            .unwrap()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn only_successful_additions_report_write_evidence_without_changing_the_wire_or_log() {
        let (_scratch, mut store) = Scratch::new("answer-metadata");
        for (request, id) in [
            (json!({"op":"append","at_unix_ms":NOW,"heard":"hello"}), 1),
            (json!({"op":"remember","at_unix_ms":NOW,"text":"coffee"}), 2),
            (
                json!({"op":"append_linked","at_unix_ms":NOW,"heard":"again","source_ids":[1]}),
                3,
            ),
        ] {
            let answer = answer(&mut store, request.to_string().as_bytes(), NOW);
            assert!(answer.wrote, "{}", answer.line);
            assert!(!answer.storage_fault);
            assert_eq!(answer.line, json!({"ok":true,"id":id}).to_string());
            assert_eq!(
                answer.log,
                format!(
                    "{} person=everyone ok entries=0",
                    request["op"].as_str().unwrap()
                )
            );
        }
        for request in [
            json!({"op":"kept"}),
            json!({"op":"recall"}),
            json!({"op":"status"}),
            json!({"op":"search","query":"hello"}),
            json!({"op":"session_open","session":"first"}),
            json!({"op":"session_close","session":"first"}),
            json!({"op":"conversation_end","conversation":"talk","source_ids":[1]}),
            json!({"op":"forget_last"}),
            json!({"op":"forget_all"}),
            json!({"op":"forget_last"}),
            json!({"op":"forget","id":999}),
        ] {
            let answer = answer(&mut store, request.to_string().as_bytes(), NOW);
            assert!(!answer.wrote, "{request}");
            assert!(!answer.storage_fault, "{request}: {}", answer.line);
            let response: Value = serde_json::from_str(&answer.line).unwrap();
            assert_eq!(response["ok"], true, "{request}: {}", answer.line);
            assert!(response.get("wrote").is_none());
            assert!(response.get("storage_fault").is_none());
        }
    }

    /// #2176, `LAMP_MEMORY_REMEMBER_ONCE`: with `once`, what this person
    /// already keeps, active and the same words ignoring case, spacing and
    /// a final punctuation mark, is that kept thing, and nothing is written.
    /// Anything else is kept as before.
    #[test]
    fn a_remember_once_the_same_person_already_keeps_word_for_word_is_that_kept_thing() {
        let (_scratch, mut store) = Scratch::new("remember-once");
        const MAI: &str = "p-0123456789abcdef";
        let mut ask = |request: Value| {
            let answer = answer(&mut store, request.to_string().as_bytes(), NOW);
            let line: Value = serde_json::from_str(&answer.line).unwrap();
            assert_eq!(line["ok"], true, "{line}");
            (line, answer.wrote)
        };
        let once = |text: &str, person: &str| json!({"op":"remember","person":person,"at_unix_ms":NOW,"text":text,"once":true});
        let (first, wrote) = ask(once("Takes her coffee black.", MAI));
        assert_eq!(first, json!({"ok":true,"id":1}));
        assert!(wrote);
        // Word for word, then differing only by case, space and the stop.
        for text in ["Takes her coffee black.", "  takes HER   coffee black "] {
            let (again, wrote) = ask(once(text, MAI));
            assert_eq!(again, json!({"ok":true,"id":1,"existing":true}), "{text}");
            assert!(!wrote, "{text}");
        }
        // Another text, and the same text of someone else: kept anew.
        assert_eq!(ask(once("takes her tea black", MAI)).0["id"], 2);
        assert_eq!(
            ask(once("Takes her coffee black.", "everyone")).0,
            json!({"ok":true,"id":1})
        );
        let kept = |store: &Days, person: &str| {
            store
                .kept(&Person::new(person).unwrap(), 16384)
                .unwrap()
                .len()
        };
        assert_eq!(kept(&store, MAI), 2);
        assert_eq!(kept(&store, "everyone"), 1);
        let line = |store: &mut Days, request: Value| {
            answer(store, request.to_string().as_bytes(), NOW).line
        };
        // Forgotten, it is kept anew (one no longer active: `days`'s test).
        store.forget(&Person::new(MAI).unwrap(), 1).unwrap();
        let fresh = line(&mut store, once("takes her coffee black", MAI));
        assert_eq!(fresh, json!({"ok":true,"id":3}).to_string());
        // Without `once`, the same text twice is two kept things, as before.
        for id in [4, 5] {
            let request =
                json!({"op":"remember","person":MAI,"at_unix_ms":NOW,"text":"takes her tea black"});
            assert_eq!(
                line(&mut store, request),
                json!({"ok":true,"id":id}).to_string()
            );
        }
    }

    #[test]
    fn caller_refusals_and_overlong_lines_do_not_report_storage_or_write_evidence() {
        let (_scratch, mut store) = Scratch::new("refusal-metadata");
        for (request, code) in [
            ("not JSON".to_owned(), "bad_json"),
            (json!({"op":"missing"}).to_string(), "unknown_op"),
            (
                json!({"op":"remember","person":"../other","at_unix_ms":NOW,"text":"coffee"})
                    .to_string(),
                "bad_person",
            ),
            (
                json!({"op":"append","at_unix_ms":NOW,"heard":" "}).to_string(),
                "empty",
            ),
            (
                json!({"op":"remember","at_unix_ms":NOW,"text":" "}).to_string(),
                "empty",
            ),
            (
                json!({"op":"append_linked","at_unix_ms":NOW,"heard":"hello","source_ids":[999]})
                    .to_string(),
                "bad_request",
            ),
            (" ".repeat(protocol::MOST_LINE_BYTES + 1), "too_long"),
        ] {
            let answer = answer(&mut store, request.as_bytes(), NOW);
            assert!(!answer.storage_fault, "{code}");
            assert!(!answer.wrote, "{code}");
            let response: Value = serde_json::from_str(&answer.line).unwrap();
            assert_eq!(response["error"], code);
        }
    }

    #[test]
    fn a_sqlite_write_failure_reports_storage_but_no_write_evidence() {
        let (scratch, store) = Scratch::new("storage-metadata");
        drop(store);
        let database = rusqlite::Connection::open(scratch.0.join(DATABASE)).unwrap();
        database
            .execute_batch(
                "CREATE TRIGGER refuse_insert BEFORE INSERT ON entries BEGIN SELECT RAISE(ABORT,'private_abort_payload'); END;",
            )
            .unwrap();
        drop(database);
        let mut store = scratch.open();
        for request in [
            json!({"op":"append","at_unix_ms":NOW,"heard":"hello"}),
            json!({"op":"remember","at_unix_ms":NOW,"text":"coffee"}),
            json!({"op":"append_linked","at_unix_ms":NOW,"heard":"hello","source_ids":[]}),
        ] {
            let answer = answer(&mut store, request.to_string().as_bytes(), NOW);
            assert!(answer.storage_fault, "{}", answer.line);
            assert!(!answer.wrote);
            assert_eq!(
                answer.line,
                json!({"ok":false,"error":"storage","message":"the memory transaction could not be completed"})
                    .to_string()
            );
            assert_eq!(
                answer.log,
                format!(
                    "{} person=everyone refused error=storage",
                    request["op"].as_str().unwrap()
                )
            );
            assert!(!answer.log.contains("private_abort_payload"));
        }
        assert!(store.status(None).unwrap().is_empty());
    }

    #[test]
    fn every_opt_in_request_shape_in_the_document_parses() {
        let document = include_str!("../../../docs/memory.md");
        let (_, optional) = document
            .split_once("### Additional requests with `--memory-days`")
            .unwrap();
        let mut names = Vec::new();
        let mut submissions = Vec::new();
        for line in optional
            .lines()
            .map(str::trim)
            .filter(|s| s.starts_with("{\"op\""))
        {
            let request: Value = serde_json::from_str(line).unwrap();
            names.push(request["op"].as_str().unwrap().to_owned());
            if let Extra::ConversationEnd { submit, .. } =
                serde_json::from_value::<Extra>(request).unwrap()
            {
                submissions.push(submit);
            }
        }
        assert_eq!(
            names,
            [
                "search",
                "append_linked",
                // With the rules it may have followed (#813).
                "append_linked",
                "forget",
                "session_open",
                "session_close",
                "conversation_end",
                "conversation_end",
                "tidy_apply"
            ]
        );
        assert_eq!(submissions, [false, true]);
    }
}
