//! Opt-in requests, plus the unchanged legacy response shapes. Model payloads
//! are data for an offline caller; no request here invokes inference.
use crate::{
    days::{Days, Error, Result, Wider},
    protocol::{self, Request, Response, Who},
    store::{Ask, Day, Entry, Person},
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
        /// #2849: on a room line only, the one known person seen saying it
        /// (`Days::append_ruled_by`).
        #[serde(default)]
        speaker: Option<String>,
    },
    /// #2849 (the first builder's yes, 23 Oct 09:37): the room's lines
    /// `speaker` said, and what was made only from them, removed
    /// (`Days::forget_said`). The room's alone.
    ForgetSaid {
        #[serde(default)]
        person: Who,
        speaker: String,
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
        /// The selective rule ([`Days::forget_linked`]). Absent, a forget is
        /// as before it, so an older voice's forget is unchanged.
        #[serde(default)]
        selective: bool,
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
        /// #1693: an observation with an event day is asked about after
        /// it, first (`Days::follow_up_with`). Without it, as before.
        #[serde(default)]
        events: bool,
        /// #1693 (the first builder on #2602, 22 Oct 09:4x, and 13:1x for
        /// an undated one): the one given, dated or not, is marked asked by
        /// `follow_up_asked`, not when given (`Days::follow_up_until_asked`).
        /// Without it, as before.
        #[serde(default)]
        until_asked: bool,
    },
    /// #1693: the lamp asked about follow-up `id`, given with `until_asked`
    /// (`Days::follow_up_asked`).
    FollowUpAsked {
        #[serde(default)]
        person: Who,
        id: u64,
    },
    /// The voice's `LAMP_MEMORY_EVENTS` (the first builder on #2602, 22 Oct
    /// 22:17): the session follow-up `id` was offered in began with a
    /// request, so the offer is taken back (`Days::follow_up_unoffered`).
    FollowUpUnoffered {
        #[serde(default)]
        person: Who,
        id: u64,
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
    /// A conversation's talk note (`Days::talk_note`, #1693's
    /// long-conversation plan, step 3).
    TalkNote {
        #[serde(default)]
        person: Who,
        text: String,
        source_ids: Vec<u64>,
    },
    /// The scope's talk notes (`Days::talk_notes`), for a recall.
    TalkNotes {
        #[serde(default)]
        person: Who,
        #[serde(default)]
        query: Option<String>,
        #[serde(default)]
        day_from: Option<String>,
        #[serde(default)]
        day_to: Option<String>,
        #[serde(default)]
        limit: Option<usize>,
        #[serde(default)]
        max_bytes: Option<usize>,
    },
    /// #2176 (5a): the room's entries become someone known's (`Days::adopt`).
    Adopt {
        #[serde(default)]
        person: Who,
        to: String,
        ids: Vec<u64>,
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

/// A recall's answer, each kept entry with the day it is about (`on`) as
/// `kept` and `search` give it (#3987): the voice tells it apart from the
/// day it was said. Older readers ignore it.
fn recalled_with_days(store: &Days, person: &Person, entries: &[Entry]) -> Result<Value> {
    let mut answer = value(Response::Recalled {
        entries: entries.to_vec(),
    });
    let days = store.event_days(person)?;
    if let Some(told) = answer["entries"].as_array_mut() {
        for (told, entry) in told.iter_mut().zip(entries) {
            if let (Entry::Kept(_), Some(day)) = (entry, days.get(&entry.id())) {
                told["on"] = json!(day);
            }
        }
    }
    Ok(answer)
}

fn legacy(store: &mut Days, request: Request) -> Result<Value> {
    Ok(match request {
        Request::Append {
            person,
            at_unix_ms,
            heard,
            said,
            speaker,
        } => value(Response::Added {
            id: store.append_ruled_by(
                &one(person)?,
                at_unix_ms,
                &heard,
                &said,
                &[],
                &[],
                speaker.as_deref(),
            )?,
        }),
        // #2510: a correction, checked here, of a kept thing the session
        // was shown; refused, nothing is written.
        Request::Remember {
            person,
            at_unix_ms,
            text,
            source_ids,
            replaces: Some(old),
            shown,
            on,
            speaker,
            ..
        } => {
            let id = store.remember_replacing_on_by(
                &one(person)?,
                at_unix_ms,
                &text,
                &source_ids,
                old,
                &shown,
                on.as_deref(),
                speaker.as_deref(),
            )?;
            json!({"ok":true,"id":id,"replaced":old})
        }
        Request::Remember {
            person,
            at_unix_ms,
            text,
            source_ids,
            once: false,
            on,
            speaker,
            ..
        } => value(Response::Added {
            id: store
                .remember_on_by(
                    &one(person)?,
                    at_unix_ms,
                    &text,
                    &source_ids,
                    false,
                    on.as_deref(),
                    speaker.as_deref(),
                )?
                .0,
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
            on,
            speaker,
            ..
        } => match store.remember_on_by(
            &one(person)?,
            at_unix_ms,
            &text,
            &source_ids,
            true,
            on.as_deref(),
            speaker.as_deref(),
        )? {
            (id, true) => json!({"ok":true,"id":id,"existing":true}),
            (id, false) => value(Response::Added { id }),
        },
        Request::Kept {
            person,
            max_bytes,
            observations,
        } => {
            let person = one(person)?;
            let kept = store.kept_with(
                &person,
                max_bytes.unwrap_or(4096).min(16384) as usize,
                observations,
            )?;
            let mut answer = value(Response::Kept {
                entries: kept.clone(),
            });
            // As search: the day a kept thing is about, so the voice tells it
            // apart from the day it was said (#3987). Older readers ignore it.
            let days = store.event_days(&person)?;
            if let Some(entries) = answer["entries"].as_array_mut() {
                for (entry, kept) in entries.iter_mut().zip(&kept) {
                    if let Some(day) = days.get(&kept.id) {
                        entry["on"] = json!(day);
                    }
                }
            }
            answer
        }
        Request::Recall {
            person,
            query,
            day,
            limit,
            max_bytes,
        } => {
            let person = one(person)?;
            let entries = store.recall(
                &person,
                &ask(query.as_deref(), day.as_deref(), limit, max_bytes)?,
            )?;
            recalled_with_days(store, &person, &entries)?
        }
        Request::ForgetLast { person, kind } => value(Response::Forgot {
            removed: store.forget_last(&one(person)?, kind)?,
        }),
        Request::ForgetAll { person } => value(Response::Emptied {
            removed: store.forget_all(some(person, false)?.as_ref())?,
        }),
        // The app's removal of a person: theirs and their rules scope in
        // one transaction (`Days::forget_person`). Counts only.
        Request::ForgetPerson { person } => {
            let person = match person {
                Who::Id(id) if lamp_msgs::known::is_person_id(&id) => one(Who::Id(id))?,
                _ => {
                    return Err(Error::new(
                        "bad_person",
                        "forget_person takes the id of one person the eyes know",
                    ));
                }
            };
            let (removed, rules) = store.forget_person(&person)?;
            json!({"ok":true,"removed":removed,"rules":rules})
        }
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
            speaker,
        } => {
            json!({"ok":true,"id":store.append_ruled_by(&one(person)?,at_unix_ms,&heard,&said,&source_ids,&rule_ids,speaker.as_deref())?})
        }
        Extra::ForgetSaid { person, speaker } => {
            if one(person)?.as_str() != Person::EVERYONE {
                return Err(Error::new(
                    "bad_person",
                    "forget_said is the room's: who said a line is kept only for the room's",
                ));
            }
            json!({"ok":true,"removed":store.forget_said(&speaker)?})
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
            // `more` is said only for a span of days (#3627); dated kept
            // entries can still carry `on` without a span (#3987).
            let spanned = day_from.is_some();
            let (day, last_day) = span(day, day_from, day_to)?;
            let wider = Wider {
                last_day,
                second: recall_v2,
            };
            let person = one(person)?;
            let (records, more) = store.search_wider_more(
                &person,
                &ask(Some(&query), day.as_deref(), limit, max_bytes)?,
                history,
                wider,
            )?;
            let mut entries = serde_json::to_value(&records).expect("records are JSON values only");
            if let Some(entries) = entries.as_array_mut() {
                for (entry, record) in entries.iter_mut().zip(&records) {
                    if let Some(day) = store.event_day(&person, record.id)? {
                        entry["on"] = json!(day);
                    }
                }
            }
            // #2510: with history, a replaced version says until when it
            // held, the newer version's `at_unix_ms`. Without, as before.
            if history && let Some(entries) = entries.as_array_mut() {
                for (entry, record) in entries.iter_mut().zip(&records) {
                    if !record.active
                        && let Some(until) = store.replaced_at(&person, record.id)?
                    {
                        entry["until_unix_ms"] = json!(until);
                    }
                }
            }
            // #3627: for a span, whether more matched than was given, by
            // count or bytes.
            let mut found = json!({"ok":true,"entries":entries});
            if spanned {
                found["more"] = json!(more);
            }
            found
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
            // `more` is said only for a span of days (#3627); dated kept
            // entries can still carry `on` without a span (#3987).
            let spanned = day_from.is_some();
            let (day, last_day) = span(day, day_from, day_to)?;
            let wider = Wider {
                last_day,
                second: recall_v2,
            };
            let person = one(person)?;
            let (entries, more) = store.recall_wider_more(
                &person,
                &ask(query.as_deref(), day.as_deref(), limit, max_bytes)?,
                wider,
            )?;
            let mut recalled = recalled_with_days(store, &person, &entries)?;
            if spanned {
                recalled["more"] = json!(more);
            }
            recalled
        }
        Extra::Forget {
            person,
            id,
            selective,
        } => {
            json!({"ok":true,"removed":store.forget_linked(&one(person)?,id,selective)?})
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
        Extra::FollowUp {
            person,
            events,
            until_asked,
        } => {
            json!({"ok":true,"follow_up":store.follow_up_until_asked(&one(person)?,events,until_asked)?})
        }
        Extra::FollowUpAsked { person, id } => {
            json!({"ok":true,"asked":store.follow_up_asked(&one(person)?,id)?})
        }
        Extra::FollowUpUnoffered { person, id } => {
            json!({"ok":true,"unoffered":store.follow_up_unoffered(&one(person)?,id)?})
        }
        Extra::ConversationEnd {
            person,
            conversation,
            source_ids,
            previous_ids,
            submit,
        } => {
            let person = one(person)?;
            // Ids it no longer has (a forget since) are passed over and
            // counted, never the end refused for them (#2176 row 13), not
            // even when none is left (N12).
            let (source_ids, unknown) = store.known_sources(&person, &source_ids)?;
            if unknown > 0 && source_ids.is_empty() {
                eprintln!(
                    "lamp-memory: conversation_end named {unknown} source(s) it no longer has; closed with nothing to observe"
                );
            } else if unknown > 0 {
                eprintln!(
                    "lamp-memory: conversation_end named {unknown} source(s) it no longer has; ended with the rest"
                );
            }
            if unknown > 0 && source_ids.is_empty() {
                // Every exchange it named was forgotten since (a forget or
                // forget_everything in the open conversation, Friday's N12
                // on #3504): it closes, with nothing to observe and no job,
                // never refused. Nothing of it is stored.
                if submit {
                    json!({"ok":true,"submitted":false,"reason":"forgotten"})
                } else {
                    json!({"ok":true,"reason":"forgotten"})
                }
            } else if submit {
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
        Extra::TalkNote {
            person,
            text,
            source_ids,
        } => {
            json!({"ok":true,"id":store.talk_note(&one(person)?,&text,&source_ids)?})
        }
        Extra::TalkNotes {
            person,
            query,
            day_from,
            day_to,
            limit,
            max_bytes,
        } => {
            let days = match (day_from, day_to) {
                (None, None) => None,
                (Some(from), Some(to)) => {
                    let bad = || {
                        Error::new(
                            "bad_day",
                            "day_from and day_to are YYYY-MM-DD, oldest first",
                        )
                    };
                    let (from, to) = (
                        Day::parse(&from).ok_or_else(bad)?,
                        Day::parse(&to).ok_or_else(bad)?,
                    );
                    if from > to {
                        return Err(bad());
                    }
                    Some((from, to))
                }
                _ => {
                    return Err(Error::new("bad_day", "day_from and day_to come together"));
                }
            };
            let (notes, more) = store.talk_notes(
                &one(person)?,
                query.as_deref(),
                days,
                limit.unwrap_or(crate::days::TALK_NOTES_MOST),
                max_bytes.unwrap_or(crate::days::TALK_NOTES_BYTES),
            )?;
            let notes: Vec<Value> = notes
                .into_iter()
                .map(|(id, day, text)| json!({"id":id,"day":day,"text":text}))
                .collect();
            json!({"ok":true,"talk_notes":notes,"more":more})
        }
        Extra::Adopt { person, to, ids } => {
            let to = one(Who::Id(to))?;
            let moved = store.adopt(&one(person)?, &to, &ids)?;
            json!({"ok":true,"moved":moved.len(),"ids":moved})
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
            "follow_up_asked",
            "follow_up_unoffered",
            "conversation_end",
            "tidy_apply",
            "adopt",
            "forget_said",
            "talk_note",
            "talk_notes",
        ];
        const OLD: &[&str] = &[
            "append",
            "remember",
            "kept",
            "recall",
            "forget_last",
            "forget_all",
            "forget_person",
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
    let done = result.is_ok();
    let (response, outcome, storage_fault, wrote) = match result {
        Ok(response) => {
            // A `talk_notes` answer's are its notes: counted as `entries`
            // said 0 for each lookup on lamp-06c1 (30 Oct), found or not.
            let count = response
                .get("entries")
                .or_else(|| response.get("talk_notes"))
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
            match e.reason {
                None => json!({"ok":false,"error":e.code,"message":e.message}),
                Some(reason) => {
                    json!({"ok":false,"error":e.code,"reason":reason,"message":e.message})
                }
            },
            match e.reason {
                None => format!("refused error={}", e.code),
                Some(reason) => format!("refused error={} reason={reason}", e.code),
            },
            e.code == "storage",
            false,
        ),
    };
    // A `forget_all` or `forget_person` that was done, for `memory/forgot`:
    // `*` is every person's. A `forget_person` that was done is of a `p-`
    // id; its rules scope went with it and is not told apart.
    let forgot = (done && matches!(op.as_str(), "forget_all" | "forget_person")).then(|| match who
        .as_str()
    {
        "*" => protocol::Forgot::Every,
        id => protocol::Forgot::One(id.into()),
    });
    protocol::Answer {
        line: response.to_string(),
        log: format!("{op} person={who} {outcome}"),
        storage_fault,
        wrote,
        forgot,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{days::DATABASE, protocol::Forgot, store::Options};
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

    const NOW_MS: u64 = NOW;

    #[test]
    fn search_returns_the_day_attached_to_a_dated_fact() {
        let (_scratch, mut store) = Scratch::new("search-event-day");
        let person = "p-0123456789abcdef";
        let send = |store: &mut Days, request: Value| -> Value {
            serde_json::from_str(&answer(store, request.to_string().as_bytes(), NOW).line).unwrap()
        };
        let kept = send(
            &mut store,
            json!({"op":"remember","person":person,"at_unix_ms":NOW,"text":"Sister's wedding is on Saturday","on":"2026-10-17"}),
        );
        assert_eq!(kept["ok"], true, "{kept}");
        let found = send(
            &mut store,
            json!({"op":"search","person":person,"query":"wedding"}),
        );
        assert_eq!(found["ok"], true, "{found}");
        assert_eq!(found["entries"][0]["id"], kept["id"]);
        assert_eq!(found["entries"][0]["on"], "2026-10-17");
    }

    /// #3987: the kept list carries a kept thing's day as search does, and
    /// none on an undated one, so the voice tells the day apart from when
    /// it was said.
    #[test]
    fn kept_returns_the_day_attached_to_a_dated_fact() {
        let (_scratch, mut store) = Scratch::new("kept-event-day");
        let person = "p-0123456789abcdef";
        let send = |store: &mut Days, request: Value| -> Value {
            serde_json::from_str(&answer(store, request.to_string().as_bytes(), NOW).line).unwrap()
        };
        let dated = send(
            &mut store,
            json!({"op":"remember","person":person,"at_unix_ms":NOW,"text":"My dentist appointment is on Friday","on":"2026-10-16"}),
        );
        let plain = send(
            &mut store,
            json!({"op":"remember","person":person,"at_unix_ms":NOW,"text":"I take my coffee black"}),
        );
        let kept = send(&mut store, json!({"op":"kept","person":person}));
        assert_eq!(kept["ok"], true, "{kept}");
        let entries = kept["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 2, "{kept}");
        let of = |id: &Value| entries.iter().find(|entry| entry["id"] == *id).unwrap();
        assert_eq!(of(&dated["id"])["on"], "2026-10-16");
        assert!(of(&plain["id"]).get("on").is_none(), "{kept}");
    }

    /// #3987: a recall, legacy or with #2176's fields, carries a kept
    /// thing's day as `kept` does, and none on an undated one.
    #[test]
    fn recall_returns_the_day_attached_to_a_dated_fact() {
        let (_scratch, mut store) = Scratch::new("recall-event-day");
        let person = "p-0123456789abcdef";
        let send = |store: &mut Days, request: Value| -> Value {
            serde_json::from_str(&answer(store, request.to_string().as_bytes(), NOW).line).unwrap()
        };
        let dated = send(
            &mut store,
            json!({"op":"remember","person":person,"at_unix_ms":NOW,"text":"My dentist appointment is on Friday","on":"2026-10-16"}),
        );
        let plain = send(
            &mut store,
            json!({"op":"remember","person":person,"at_unix_ms":NOW,"text":"The dentist is on Main Street"}),
        );
        for request in [
            json!({"op":"recall","person":person,"query":"dentist"}),
            json!({"op":"recall","person":person,"query":"dentist","recall_v2":true}),
        ] {
            let found = send(&mut store, request.clone());
            assert_eq!(found["ok"], true, "{request} {found}");
            let entries = found["entries"].as_array().unwrap();
            assert_eq!(entries.len(), 2, "{request} {found}");
            let of = |id: &Value| entries.iter().find(|entry| entry["id"] == *id).unwrap();
            assert_eq!(of(&dated["id"])["on"], "2026-10-16", "{request}");
            assert!(of(&plain["id"]).get("on").is_none(), "{request} {found}");
        }
    }

    /// #3987 with #3627: a bounded span keeps the event day on its kept
    /// entry while also saying that more matched than it returned.
    #[test]
    fn a_spanned_recall_keeps_both_the_event_day_and_more() {
        let (_scratch, mut store) = Scratch::new("span-event-day-more");
        let person = "p-0123456789abcdef";
        let send = |store: &mut Days, request: Value| -> Value {
            serde_json::from_str(&answer(store, request.to_string().as_bytes(), NOW).line).unwrap()
        };
        let dated = send(
            &mut store,
            json!({"op":"remember","person":person,"at_unix_ms":NOW,"text":"Dentist appointment on Friday","on":"2026-10-16"}),
        );
        assert_eq!(dated["ok"], true, "{dated}");
        for n in 0..2 {
            let exchange = send(
                &mut store,
                json!({"op":"append","person":person,"at_unix_ms":NOW+n+1,"heard":format!("unrelated talk {n}"),"said":"okay"}),
            );
            assert_eq!(exchange["ok"], true, "{exchange}");
        }
        let found = send(
            &mut store,
            json!({"op":"recall","person":person,"day_from":"2026-10-08","day_to":"2026-10-13","recall_v2":true,"limit":1}),
        );
        assert_eq!(found["ok"], true, "{found}");
        assert_eq!(found["more"], true, "{found}");
        assert_eq!(found["entries"].as_array().unwrap().len(), 1, "{found}");
        assert_eq!(found["entries"][0]["id"], dated["id"], "{found}");
        assert_eq!(found["entries"][0]["on"], "2026-10-16", "{found}");
    }

    /// #3987: a session's note gives a dated fact its day, and an undated
    /// one none, so a session opened after a restart is told the day too.
    #[test]
    fn a_session_note_gives_a_dated_fact_its_day() {
        let (_scratch, mut store) = Scratch::new("note-event-day");
        let person = "p-0123456789abcdef";
        let send = |store: &mut Days, request: Value| -> Value {
            serde_json::from_str(&answer(store, request.to_string().as_bytes(), NOW).line).unwrap()
        };
        let dated = send(
            &mut store,
            json!({"op":"remember","person":person,"at_unix_ms":NOW,"text":"My dentist appointment is on Friday","on":"2026-10-16"}),
        );
        let plain = send(
            &mut store,
            json!({"op":"remember","person":person,"at_unix_ms":NOW,"text":"I take my coffee black"}),
        );
        let opened = send(
            &mut store,
            json!({"op":"session_open","person":person,"session":"s-1","max_bytes":4096}),
        );
        assert_eq!(opened["ok"], true, "{opened}");
        let note: Value =
            serde_json::from_str(opened["snapshot"]["note"].as_str().unwrap()).unwrap();
        let facts = note["facts"].as_array().unwrap();
        assert_eq!(facts.len(), 2, "{note}");
        let of = |id: &Value| facts.iter().find(|fact| fact["id"] == *id).unwrap();
        assert_eq!(of(&dated["id"])["on"], "2026-10-16");
        assert!(of(&plain["id"]).get("on").is_none(), "{note}");
    }

    /// #3613 on the wire: `forget` with `selective:true` takes an unlinked
    /// copy of the fact; without the field (an older voice) it stays, as
    /// before.
    #[test]
    fn forget_is_selective_only_when_the_request_says_so() {
        for selective in [false, true] {
            let (_scratch, mut store) = Scratch::new(&format!("forget-selective-{selective}"));
            let person = "p-0123456789abcdef";
            let send = |store: &mut Days, request: Value| -> Value {
                serde_json::from_str(&answer(store, request.to_string().as_bytes(), NOW).line)
                    .unwrap()
            };
            let kept = send(
                &mut store,
                json!({"op":"remember","person":person,"at_unix_ms":NOW,"text":"My favourite colour is teal"}),
            );
            let copy = send(
                &mut store,
                json!({"op":"append","person":person,"at_unix_ms":NOW+1,"heard":"my favourite colour is teal","said":"Noted"}),
            );
            let mut forget = json!({"op":"forget","person":person,"id":kept["id"]});
            if selective {
                forget["selective"] = json!(true);
            }
            let forgotten = send(&mut store, forget);
            assert_eq!(forgotten["ok"], true, "{forgotten}");
            assert_eq!(
                forgotten["removed"],
                if selective { 2 } else { 1 },
                "{forgotten}"
            );
            assert_eq!(
                store
                    .record(&Person::new(person).unwrap(), copy["id"].as_u64().unwrap())
                    .unwrap()
                    .is_none(),
                selective
            );
        }
    }

    #[test]
    fn a_broad_day_span_keeps_older_kept_facts_among_recent_exchanges() {
        let (_scratch, mut store) = Scratch::new("span-kept-priority");
        let person = "p-0123456789abcdef";
        let send = |store: &mut Days, request: Value| -> Value {
            serde_json::from_str(&answer(store, request.to_string().as_bytes(), NOW).line).unwrap()
        };
        let old = send(
            &mut store,
            json!({"op":"remember","person":person,"at_unix_ms":NOW-5*86_400_000,"text":"works at a bakery"}),
        );
        assert_eq!(old["ok"], true);
        for n in 0..10 {
            let added = send(
                &mut store,
                json!({"op":"append","person":person,"at_unix_ms":NOW+n,"heard":format!("today's unrelated update {n}"),"said":"okay"}),
            );
            assert_eq!(added["ok"], true, "{added}");
        }
        let found = send(
            &mut store,
            json!({"op":"recall","person":person,"day_from":"2026-10-08","day_to":"2026-10-13","recall_v2":true,"limit":6}),
        );
        assert_eq!(found["ok"], true, "{found}");
        assert!(
            found["entries"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["id"] == old["id"]),
            "{found}"
        );
    }

    /// #3627 (the reviewer-lead's P2): a span's answer says `more` exactly,
    /// when there was one past its `limit` or its `max_bytes` left one out,
    /// and not when it gave all; a search says it too.
    #[test]
    fn a_span_answer_says_when_there_is_more_than_it_gives() {
        let (_scratch, mut store) = Scratch::new("span-more");
        let person = "p-0123456789abcdef";
        let send = |store: &mut Days, request: Value| -> Value {
            serde_json::from_str(&answer(store, request.to_string().as_bytes(), NOW).line).unwrap()
        };
        for n in 0..3 {
            let added = send(
                &mut store,
                json!({"op":"append","person":person,"at_unix_ms":NOW+n,"heard":format!("the coffee update {n}"),"said":"okay"}),
            );
            assert_eq!(added["ok"], true, "{added}");
        }
        let span = |limit: u64, max_bytes: u64| json!({"op":"recall","person":person,"day_from":"2026-10-08","day_to":"2026-10-13","recall_v2":true,"limit":limit,"max_bytes":max_bytes});
        for (limit, max_bytes, given, more) in [
            (3, 4096, 3, false),
            (2, 4096, 2, true),
            (6, 30, 1, true),
            (6, 4096, 3, false),
        ] {
            let found = send(&mut store, span(limit, max_bytes));
            assert_eq!(found["ok"], true, "{found}");
            assert_eq!(found["entries"].as_array().unwrap().len(), given, "{found}");
            assert_eq!(found["more"], more, "{limit} {max_bytes}: {found}");
        }
        let searched = send(
            &mut store,
            json!({"op":"search","person":person,"query":"coffee","limit":2,"day_from":"2026-10-08","day_to":"2026-10-13"}),
        );
        assert_eq!(searched["more"], true, "{searched}");
        // No span: no `more`, the answer as it was.
        let plain = send(
            &mut store,
            json!({"op":"search","person":person,"query":"coffee","limit":2}),
        );
        assert!(plain.get("more").is_none(), "{plain}");
    }

    /// `memory/forgot` (#2467's follow-up): only a `forget_all` that was
    /// done says whose: one person's id, "everyone" when unsaid, every
    /// person's for `null`. A refused one, and every other op, says nothing.
    #[test]
    fn only_a_forget_all_that_was_done_says_whose() {
        let (_scratch, mut store) = Scratch::new("forgot");
        let forgot = |store: &mut _, line: serde_json::Value| {
            answer(store, line.to_string().as_bytes(), NOW_MS).forgot
        };
        assert_eq!(
            forgot(
                &mut store,
                json!({"op":"forget_all","person":"p-0123456789abcdef"})
            ),
            Some(Forgot::One("p-0123456789abcdef".into()))
        );
        assert_eq!(
            forgot(&mut store, json!({"op":"forget_all"})),
            Some(Forgot::One("everyone".into()))
        );
        assert_eq!(
            forgot(&mut store, json!({"op":"forget_all","person":null})),
            Some(Forgot::Every)
        );
        assert_eq!(
            forgot(&mut store, json!({"op":"forget_all","person":"NOT AN ID"})),
            None
        );
        assert_eq!(
            forgot(
                &mut store,
                json!({"op":"forget_last","person":"p-0123456789abcdef"})
            ),
            None
        );
        assert_eq!(
            forgot(&mut store, json!({"op":"status","person":null})),
            None
        );
        // #2849: the room's lines a person said are not all of anyone's.
        assert_eq!(
            forgot(
                &mut store,
                json!({"op":"forget_said","speaker":"p-0123456789abcdef"})
            ),
            None
        );
    }

    /// `forget_person` (the app's removal of a person): done, it says the
    /// person's `p-` id, its rules scope not told apart; refused, nothing.
    /// Only a `p-` id is taken, never the room, an `r-` scope or every
    /// person, and its answer is counts only.
    #[test]
    fn a_forget_person_that_was_done_says_whose_and_takes_only_a_known_persons_id() {
        let (_scratch, mut store) = Scratch::new("forget-person");
        let p1 = "p-0123456789abcdef";
        for line in [
            json!({"op":"remember","person":p1,"at_unix_ms":NOW_MS,"text":"tea"}),
            json!({"op":"remember","person":"r-0123456789abcdef","at_unix_ms":NOW_MS,"text":"[quiet] hush"}),
        ] {
            let said = answer(&mut store, line.to_string().as_bytes(), NOW_MS).line;
            let said: serde_json::Value = serde_json::from_str(&said).unwrap();
            assert_eq!(said["ok"], true, "{said}");
        }
        let done = answer(
            &mut store,
            json!({"op":"forget_person","person":p1})
                .to_string()
                .as_bytes(),
            NOW_MS,
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&done.line).unwrap(),
            json!({"ok":true,"removed":1,"rules":1})
        );
        assert_eq!(done.forgot, Some(Forgot::One(p1.into())));
        assert!(!done.log.contains("tea") && !done.log.contains("hush"));
        for refused in [
            json!({"op":"forget_person"}),
            json!({"op":"forget_person","person":null}),
            json!({"op":"forget_person","person":"everyone"}),
            json!({"op":"forget_person","person":"r-0123456789abcdef"}),
            json!({"op":"forget_person","person":"mai"}),
            json!({"op":"forget_person","person":"NOT AN ID"}),
        ] {
            let said = answer(&mut store, refused.to_string().as_bytes(), NOW_MS);
            assert!(
                said.line.contains(r#""error":"bad_person""#),
                "{refused}: {}",
                said.line
            );
            assert_eq!(said.forgot, None, "{refused}");
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

    /// #2510, `LAMP_MEMORY_REPLACES`: a correction replaces a kept thing
    /// by link, only one of this person's, kept, active, not newer and
    /// shown this session; each refusal says why and writes nothing. The
    /// old one is then only history, and forgetting either removes both.
    #[test]
    fn a_correction_replaces_a_kept_thing_this_session_was_shown_by_link() {
        const MAI: &str = "p-0123456789abcdef";
        const BEN: &str = "p-fedcba9876543210";
        let violet = json!({"op":"remember","person":MAI,"at_unix_ms":NOW,"text":"Her favourite colour is violet."});
        let green = |person: &str, at: u64, replaces: u64, shown: Value| json!({"op":"remember","person":person,"at_unix_ms":at,"text":"Her favourite colour is green.","replaces":replaces,"shown":shown});
        let colour =
            |history: bool| json!({"op":"search","person":MAI,"query":"colour","history":history});
        let ids = |line: &Value| -> Vec<u64> {
            line["entries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| e["id"].as_u64().unwrap())
                .collect()
        };
        for forget_newer in [true, false] {
            let (_scratch, mut store) = Scratch::new(&format!("replaces-{forget_newer}"));
            let mut ask = |request: Value| {
                let answer = answer(&mut store, request.to_string().as_bytes(), NOW + 9);
                let line: Value = serde_json::from_str(&answer.line).unwrap();
                (line, answer.wrote, answer.log)
            };
            assert_eq!(ask(violet.clone()).0, json!({"ok":true,"id":1}));
            assert_eq!(
                ask(json!({"op":"append","person":MAI,"at_unix_ms":NOW,"heard":"hello"})).0["id"],
                2
            );
            let mai = ask(json!({"op":"status","person":MAI})).0;
            for (request, reason) in [
                (green(MAI, NOW + 1, 1, json!([])), "not_shown"),
                (green(MAI, NOW + 1, 1, json!([2])), "not_shown"),
                (green(BEN, NOW + 1, 1, json!([1])), "unknown"),
                (green(MAI, NOW + 1, 9, json!([9])), "unknown"),
                (green(MAI, NOW + 1, 2, json!([2])), "not_kept"),
                (green(MAI, NOW - 1, 1, json!([1])), "not_older"),
            ] {
                let (line, wrote, log) = ask(request.clone());
                assert_eq!(line["error"], "refused", "{request}: {line}");
                assert_eq!(line["reason"], reason, "{request}: {line}");
                assert!(!wrote, "{request}");
                assert!(
                    log.ends_with(&format!("refused error=refused reason={reason}")),
                    "{log}"
                );
                assert_eq!(ask(json!({"op":"status","person":MAI})).0, mai, "{request}");
                assert_eq!(ids(&ask(colour(false)).0), [1], "{request}");
            }
            // Accepted: the new one, linked; the old one is only history.
            let (line, wrote, _) = ask(green(MAI, NOW + 1, 1, json!([2, 1])));
            assert_eq!(line, json!({"ok":true,"id":3,"replaced":1}));
            assert!(wrote);
            assert_eq!(ids(&ask(colour(false)).0), [3]);
            let mut both = ids(&ask(colour(true)).0);
            both.sort();
            assert_eq!(both, [1, 3]);
            // Take #4: with history, the replaced one says until when, the
            // newer version's time; the current one, and every one without
            // history, carries no `until_unix_ms`.
            let history = ask(colour(true)).0;
            for entry in history["entries"].as_array().unwrap() {
                match entry["id"].as_u64() {
                    Some(1) => assert_eq!(entry["until_unix_ms"], NOW + 1, "{history}"),
                    _ => assert!(entry.get("until_unix_ms").is_none(), "{history}"),
                }
            }
            let current = ask(colour(false)).0;
            assert!(current["entries"][0].get("until_unix_ms").is_none());
            let kept = ask(json!({"op":"kept","person":MAI})).0;
            assert_eq!(kept["entries"].as_array().unwrap().len(), 1, "{kept}");
            assert_eq!(kept["entries"][0]["id"], 3);
            // Replaced once, never again.
            let (line, wrote, _) = ask(green(MAI, NOW + 2, 1, json!([1])));
            assert_eq!(line["reason"], "inactive", "{line}");
            assert!(!wrote);
            let forgotten =
                ask(json!({"op":"forget","person":MAI,"id":if forget_newer {3} else {1}})).0;
            assert_eq!(forgotten["removed"], 2, "{forgotten}");
            assert!(ids(&ask(colour(true)).0).is_empty());
            assert!(
                ask(json!({"op":"kept","person":MAI})).0["entries"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
        }
    }

    /// #2510 (reviewer-lead P3): `replaces` with `once` is a correction,
    /// never deduplicated, even with the very words it replaces; and a
    /// `replaces` that is no store id is refused with nothing written.
    #[test]
    fn a_correction_with_once_is_never_deduplicated_and_an_id_out_of_range_is_refused() {
        const MAI: &str = "p-0123456789abcdef";
        let (_scratch, mut store) = Scratch::new("replaces-once");
        let mut ask = |request: Value| {
            let answer = answer(&mut store, request.to_string().as_bytes(), NOW + 9);
            let line: Value = serde_json::from_str(&answer.line).unwrap();
            (line, answer.wrote)
        };
        let violet = "Her favourite colour is violet.";
        assert_eq!(
            ask(json!({"op":"remember","person":MAI,"at_unix_ms":NOW,"text":violet})).0,
            json!({"ok":true,"id":1})
        );
        let status = ask(json!({"op":"status","person":MAI})).0;
        for (replaces, error) in [
            (json!(0), "refused"),
            (json!(i64::MAX as u64 + 1), "refused"),
            (json!(u64::MAX), "refused"),
            (json!(-1), "bad_request"),
        ] {
            let (line, wrote) = ask(json!({"op":"remember","person":MAI,"at_unix_ms":NOW+1,
                "text":"Her favourite colour is green.","once":true,"replaces":replaces,"shown":[1]}));
            assert_eq!(line["ok"], false, "{replaces}: {line}");
            assert_eq!(line["error"], error, "{replaces}: {line}");
            assert!(!wrote, "{replaces}");
            assert_eq!(
                ask(json!({"op":"status","person":MAI})).0,
                status,
                "{replaces}"
            );
        }
        // The same words, once: the replaces arm comes first, so it is a new
        // version, not the existing one.
        let (line, wrote) = ask(json!({"op":"remember","person":MAI,"at_unix_ms":NOW+1,
            "text":violet,"once":true,"replaces":1,"shown":[1]}));
        assert_eq!(line, json!({"ok":true,"id":2,"replaced":1}));
        assert!(wrote);
        let kept = ask(json!({"op":"kept","person":MAI})).0;
        assert_eq!(kept["entries"].as_array().unwrap().len(), 1, "{kept}");
        assert_eq!(kept["entries"][0]["id"], 2);
        // Without replaces, once finds the new version.
        assert_eq!(
            ask(json!({"op":"remember","person":MAI,"at_unix_ms":NOW+2,"text":violet,"once":true}))
                .0,
            json!({"ok":true,"id":2,"existing":true})
        );
    }

    /// #1693, step 2: an end whose evidence is over the pass's input bound
    /// (`tidy::MAX_INPUT_BYTES`) is refused with its own message, for the
    /// voice to count, and leaves no job behind.
    #[test]
    fn a_conversation_end_over_the_input_bound_is_refused_as_too_large() {
        let (_scratch, mut store) = Scratch::new("end-too-large");
        let words = "synthetic ".repeat(50);
        let mut ids = Vec::new();
        for _ in 0..64 {
            let line = json!({"op":"append","at_unix_ms":NOW,"heard":words,"said":words});
            let wrote: Value =
                serde_json::from_str(&answer(&mut store, line.to_string().as_bytes(), NOW).line)
                    .unwrap();
            ids.push(wrote["id"].as_u64().unwrap());
        }
        let request = json!({"op":"conversation_end","conversation":"long","source_ids":ids});
        let refused: Value =
            serde_json::from_str(&answer(&mut store, request.to_string().as_bytes(), NOW).line)
                .unwrap();
        assert_eq!(refused["ok"], false, "{refused}");
        assert_eq!(refused["error"], "bad_request");
        assert!(
            refused["message"]
                .as_str()
                .unwrap()
                .contains("exceeds 32 KiB"),
            "{refused}"
        );
        // Half of it fits, and is prepared as any end is.
        let request = json!({"op":"conversation_end","conversation":"half","source_ids":ids[..16]});
        let prepared: Value =
            serde_json::from_str(&answer(&mut store, request.to_string().as_bytes(), NOW).line)
                .unwrap();
        assert!(prepared["input"]["job"].is_string(), "{prepared}");
    }

    /// #2176 row 13 (the first builder, 18:20): a conversation's end that
    /// names an exchange this store no longer has (a forget since) goes on
    /// with the rest, prepared or submitted, rather than being refused; one
    /// with nothing left it has closes with nothing to observe (N12). An id
    /// never given is still refused.
    #[test]
    fn a_conversation_end_passes_over_sources_it_no_longer_has() {
        let (_scratch, mut store) = Scratch::new("end-unknown");
        for heard in ["one", "two", "three", "four"] {
            let line = json!({"op":"append","at_unix_ms":NOW,"heard":heard}).to_string();
            assert!(answer(&mut store, line.as_bytes(), NOW).wrote);
        }
        for id in [3, 4] {
            let line = json!({"op":"forget","id":id}).to_string();
            let forgot = answer(&mut store, line.as_bytes(), NOW).line;
            assert_eq!(forgot, r#"{"ok":true,"removed":1}"#);
        }
        let ended = |store: &mut Days, request: Value| -> Value {
            serde_json::from_str(&answer(store, request.to_string().as_bytes(), NOW).line).unwrap()
        };
        let prepared = ended(
            &mut store,
            json!({"op":"conversation_end","conversation":"talk","source_ids":[1,3,2]}),
        );
        assert_eq!(prepared["ok"], true, "{prepared}");
        assert_eq!(prepared["input"]["source_ids"], json!([1, 2]));
        let submitted = ended(
            &mut store,
            json!({"op":"conversation_end","conversation":"later","source_ids":[4,2],"submit":true}),
        );
        assert_eq!(submitted["ok"], true, "{submitted}");
        let nothing = ended(
            &mut store,
            json!({"op":"conversation_end","conversation":"gone","source_ids":[3,4]}),
        );
        assert_eq!(nothing, json!({"ok":true,"reason":"forgotten"}));
        // An id this scope was never given is the caller's mistake, never a
        // forget.
        let never = ended(
            &mut store,
            json!({"op":"conversation_end","conversation":"never","source_ids":[1,999]}),
        );
        assert_eq!(never["error"], "bad_request", "{never}");
        // No sources at all is still the caller's mistake.
        let none = ended(
            &mut store,
            json!({"op":"conversation_end","conversation":"empty","source_ids":[]}),
        );
        assert_eq!(none["ok"], false, "{none}");
        // Another person's exchange is refused as before, never passed over
        // as if it were forgotten (the reviewer-lead's P3 on #2226). Each
        // person's records are numbered apart: their fifth is an id the
        // room's scope was never given.
        let mut theirs = Value::Null;
        for heard in ["a", "b", "c", "d", "e"] {
            let line =
                json!({"op":"append","person":"p-0123456789abcdef","at_unix_ms":NOW,"heard":heard});
            theirs =
                serde_json::from_str(&answer(&mut store, line.to_string().as_bytes(), NOW).line)
                    .unwrap();
        }
        assert_eq!(theirs["id"], 5, "{theirs}");
        let foreign = ended(
            &mut store,
            json!({"op":"conversation_end","conversation":"foreign","source_ids":[1, 5]}),
        );
        assert_eq!(foreign["ok"], false, "{foreign}");
    }

    /// The N12 follow-up (lamp-06c1, 28 Oct: `conversation_end ... refused
    /// error=bad_request` 2 ms after a `forget_everything`): ids are numbered
    /// per person, so the room's own ids, the same numbers, never make a
    /// person's forgotten exchange look known. Two scopes with overlapping
    /// ids: one of the person's three forgotten, the end observes the other
    /// two; everything of theirs forgotten, the end closes with nothing to
    /// observe; an id never given them is still refused.
    #[test]
    fn a_conversation_end_asks_only_the_persons_own_ids() {
        const MAI: &str = "p-0123456789abcdef";
        let (_scratch, mut store) = Scratch::new("end-per-person");
        let ask = |store: &mut Days, request: Value| -> Value {
            serde_json::from_str(&answer(store, request.to_string().as_bytes(), NOW).line).unwrap()
        };
        // The room has ids 1..=8, more than the person will ever be given.
        for n in 0..8 {
            let line = ask(
                &mut store,
                json!({"op":"append","at_unix_ms":NOW,"heard":format!("room {n}")}),
            );
            assert_eq!(line["id"], n + 1, "{line}");
        }
        let mut ids = vec![];
        for heard in ["one", "two", "three"] {
            let line = ask(
                &mut store,
                json!({"op":"append","person":MAI,"at_unix_ms":NOW,"heard":heard}),
            );
            ids.push(line["id"].as_u64().unwrap());
        }
        assert_eq!(ids, [1, 2, 3], "the person's numbering is their own");
        let forgot = ask(&mut store, json!({"op":"forget","person":MAI,"id":ids[1]}));
        assert_eq!(forgot["ok"], true, "{forgot}");
        let prepared = ask(
            &mut store,
            json!({"op":"conversation_end","person":MAI,"conversation":"talk","source_ids":ids}),
        );
        assert_eq!(prepared["ok"], true, "{prepared}");
        assert_eq!(prepared["input"]["source_ids"], json!([ids[0], ids[2]]));

        let mut later = vec![];
        for heard in ["four", "five"] {
            let line = ask(
                &mut store,
                json!({"op":"append","person":MAI,"at_unix_ms":NOW,"heard":heard}),
            );
            later.push(line["id"].as_u64().unwrap());
        }
        let emptied = ask(&mut store, json!({"op":"forget_all","person":MAI}));
        assert_eq!(emptied["ok"], true, "{emptied}");
        for (conversation, submit, reply) in [
            ("later", false, json!({"ok":true,"reason":"forgotten"})),
            (
                "later2",
                true,
                json!({"ok":true,"submitted":false,"reason":"forgotten"}),
            ),
        ] {
            let closed = ask(
                &mut store,
                json!({"op":"conversation_end","person":MAI,"conversation":conversation,
                    "source_ids":later,"submit":submit}),
            );
            assert_eq!(closed, reply);
        }
        // Never given this person, though the room has an id 7: refused.
        let never = ask(
            &mut store,
            json!({"op":"conversation_end","person":MAI,"conversation":"never","source_ids":[7]}),
        );
        assert_eq!(never["error"], "bad_request", "{never}");
        // The room's own end is untouched by the person's wipe.
        let room = ask(
            &mut store,
            json!({"op":"conversation_end","conversation":"room","source_ids":[4, 5]}),
        );
        assert_eq!(room["input"]["source_ids"], json!([4, 5]), "{room}");
    }

    /// Friday's N12 on #3504: a forget, or a forget_everything, in an open
    /// conversation never gets its end refused. Three exchanges, one
    /// forgotten: the end is prepared on the other two. Then everything
    /// forgotten: the next end closes with nothing to observe, prepared or
    /// submitted, and keeps no job.
    #[test]
    fn a_conversation_end_after_a_forget_in_it_is_never_refused() {
        const MAI: &str = "p-0123456789abcdef";
        let (_scratch, mut store) = Scratch::new("end-after-forget");
        let ask = |store: &mut Days, request: Value| -> Value {
            serde_json::from_str(&answer(store, request.to_string().as_bytes(), NOW).line).unwrap()
        };
        let mut ids = vec![];
        for heard in ["one", "two", "three"] {
            let line = ask(
                &mut store,
                json!({"op":"append","person":MAI,"at_unix_ms":NOW,"heard":heard}),
            );
            ids.push(line["id"].as_u64().unwrap());
        }
        let forgot = ask(&mut store, json!({"op":"forget","person":MAI,"id":ids[1]}));
        assert_eq!(forgot["ok"], true, "{forgot}");
        let prepared = ask(
            &mut store,
            json!({"op":"conversation_end","person":MAI,"conversation":"talk","source_ids":ids}),
        );
        assert_eq!(prepared["ok"], true, "{prepared}");
        assert_eq!(prepared["input"]["source_ids"], json!([ids[0], ids[2]]));
        let heard: Vec<_> = prepared["input"]["records"]
            .as_array()
            .unwrap()
            .iter()
            .map(|record| record["heard"].clone())
            .collect();
        assert_eq!(heard, [json!("one"), json!("three")]);

        let mut later = vec![];
        for heard in ["four", "five"] {
            let line = ask(
                &mut store,
                json!({"op":"append","person":MAI,"at_unix_ms":NOW,"heard":heard}),
            );
            later.push(line["id"].as_u64().unwrap());
        }
        let emptied = ask(&mut store, json!({"op":"forget_all","person":MAI}));
        assert_eq!(emptied["ok"], true, "{emptied}");
        let closed = ask(
            &mut store,
            json!({"op":"conversation_end","person":MAI,"conversation":"later","source_ids":later}),
        );
        assert_eq!(closed, json!({"ok":true,"reason":"forgotten"}));
        let submitted = ask(
            &mut store,
            json!({"op":"conversation_end","person":MAI,"conversation":"later2",
                "source_ids":later,"submit":true}),
        );
        assert_eq!(
            submitted,
            json!({"ok":true,"submitted":false,"reason":"forgotten"})
        );
        // No job was kept: the same end again closes the same way.
        let again = ask(
            &mut store,
            json!({"op":"conversation_end","person":MAI,"conversation":"later","source_ids":later}),
        );
        assert_eq!(again, closed);
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
                "tidy_apply",
                // Who said the room's lines, forgotten (#2849).
                "forget_said"
            ]
        );
        assert_eq!(submissions, [false, true]);
    }

    /// #3113: `kept` with `observations` lists what the lamp noticed of a
    /// person beside what they asked it to keep, current ones only, so a
    /// forget by name finds "my favourite colour" kept on its own. Without
    /// it the listing is what it was, and no other person's is listed.
    #[test]
    fn kept_with_observations_lists_the_persons_current_observations_too() {
        const MAI: &str = "p-0123456789abcdef";
        const AN: &str = "p-fedcba9876543210";
        let (_scratch, mut store) = Scratch::new("kept-observations");
        let mai = Person::new(MAI).unwrap();
        let an = Person::new(AN).unwrap();
        let ask = |store: &mut Days, request: Value| -> Value {
            serde_json::from_str(&answer(store, request.to_string().as_bytes(), NOW + 9).line)
                .unwrap()
        };
        let wedding = ask(
            &mut store,
            json!({"op":"remember","person":MAI,"at_unix_ms":NOW,"text":"Sister's wedding is on Saturday."}),
        );
        assert_eq!(wedding, json!({"ok":true,"id":1}));
        let old = store.observe_for_test(&mai, NOW, "Favourite colour is blue.", false);
        let colour = store.observe_for_test(&mai, NOW + 1, "Favourite colour is violet.", true);
        store.observe_for_test(&an, NOW, "Favourite colour is green.", true);
        let texts = |line: &Value| -> Vec<(u64, String)> {
            line["entries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|entry| {
                    (
                        entry["id"].as_u64().unwrap(),
                        entry["text"].as_str().unwrap().to_owned(),
                    )
                })
                .collect()
        };
        let plain = ask(&mut store, json!({"op":"kept","person":MAI}));
        assert_eq!(
            texts(&plain),
            [(1, "Sister's wedding is on Saturday.".into())]
        );
        let off = ask(
            &mut store,
            json!({"op":"kept","person":MAI,"observations":false}),
        );
        assert_eq!(off, plain);
        let both = ask(
            &mut store,
            json!({"op":"kept","person":MAI,"observations":true,"max_bytes":16384}),
        );
        assert_eq!(
            texts(&both),
            [
                (1, "Sister's wedding is on Saturday.".into()),
                (colour, "Favourite colour is violet.".into())
            ],
            "{both}"
        );
        assert!(!texts(&both).iter().any(|(id, _)| *id == old));
        // Forgotten by its id, it is listed no more; the kept line stays.
        let forgotten = ask(&mut store, json!({"op":"forget","person":MAI,"id":colour}));
        assert_eq!(forgotten["removed"], 1, "{forgotten}");
        let after = ask(
            &mut store,
            json!({"op":"kept","person":MAI,"observations":true}),
        );
        assert_eq!(
            texts(&after),
            [(1, "Sister's wedding is on Saturday.".into())]
        );
        let theirs = ask(
            &mut store,
            json!({"op":"kept","person":AN,"observations":true}),
        );
        assert_eq!(texts(&theirs).len(), 1);
    }
}
