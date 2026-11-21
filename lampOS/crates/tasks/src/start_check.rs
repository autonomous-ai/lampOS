//! A new start that timed out, settled (#8170). `harness new` answered
//! nothing within its bound (`timeout`, no exit), yet Harness may have
//! started the work on the computer: on lamp-06c1 on 21 Nov one did, and the
//! lamp neither followed nor closed it. Such a task keeps its dispatch
//! `uncertain`, its code [`crate::engine::START_CHECKING`], and is never sent
//! again; lamp-tasks reads that computer's harness list (`harness ps
//! --machine <id> --json`, one read on the Harness worker, never on the
//! voice's path) on its next round and every [`crate::engine::WATCH_CONFIRM_MS`]
//! after, and settles it by what the list shows ([`pick`]):
//!
//! - **Only by the lamp's own name for that start.** Every start is sent
//!   with `--name <launch name>` (`lamp-<32 hex>`, the store's and the
//!   task's digest), a name no person gives. Exactly one row of that name is
//!   the one; it is tracked and closed as any other task the lamp started.
//!   Two of that name, or one another task of the lamp's has: not sure.
//! - **No fallback.** Harness 0.6.79's `ps --json` gives no time a harness
//!   was made (`lastActive` is its last activity), and the rows before the
//!   `new` are not read, so nothing tells a person's harness from the
//!   lamp's once Harness lists the lamp's by its title ("its name until the
//!   agent titles its session", `harness new --help`): no row is ever taken
//!   but by the lamp's own name.
//! - **Not sure, never "not started", when it could be there.** With no row
//!   of its name, a row in a folder of the same last part that the lamp
//!   doesn't know (not another task's, not in the session index, not named
//!   by another start of the lamp's), in any state, could be its start
//!   titled: [`Pick::Maybe`], told as not sure it started. Only with no such
//!   row is it not started, and only in this boot, by the bound
//!   ([`crate::engine::NOT_STARTED`]).
//!
//! A harness the person started themselves is never taken: only a row of
//! the lamp's own name for that start is.
use crate::harness::Launch;
use serde_json::Value;
use std::collections::BTreeSet;

/// One row of `harness ps --machine <id> --json` (Harness 0.6.79: `name`,
/// `id`, `state`, `folder`, `lastActive`, among others), as much as a check
/// needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Started {
    pub id: String,
    pub name: Option<String>,
    pub folder: Option<String>,
}

/// The rows of a `ps --json` answer, bounded as every other read of it
/// ([`crate::harness::PS_STDOUT_LIMIT`], [`crate::harness::MOST_LISTED`]);
/// a row with no agent id (a creation still under way) is left out.
pub fn parse(bytes: &[u8]) -> Result<Vec<Started>, &'static str> {
    if bytes.len() > crate::harness::PS_STDOUT_LIMIT {
        return Err("status_invalid_reply");
    }
    let reply: Value = serde_json::from_slice(bytes).map_err(|_| "status_invalid_reply")?;
    let rows = reply["harnesses"]
        .as_array()
        .ok_or("status_invalid_reply")?;
    if rows.len() > crate::harness::MOST_LISTED {
        return Err("status_invalid_reply");
    }
    let text = |row: &Value, key: &str| {
        row[key]
            .as_str()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    };
    Ok(rows
        .iter()
        .filter_map(|row| {
            let id = row["id"]
                .as_str()
                .filter(|id| crate::harness::agent_id(id))?;
            Some(Started {
                id: id.to_owned(),
                name: text(row, "name"),
                folder: text(row, "folder"),
            })
        })
        .collect())
}

/// What a list says of a start that timed out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pick {
    /// Listed by the lamp's own name for it: the lamp's, closed as any.
    Named(String),
    /// Two of its name, or one another task has: neither is taken.
    Two,
    /// Not by its name, but a row the lamp doesn't know is in a folder of
    /// the same last part: it could be the start, titled. Never taken, and
    /// never said not started.
    Maybe,
    /// Nothing listed could be it.
    None,
}

/// A lamp-made name: `lamp-` and hex, as every start's ([`Launch::name`]).
fn lamp_made(name: &str) -> bool {
    name.strip_prefix("lamp-")
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_hexdigit()))
}

/// A folder's last part, as `ps` lists it (`/Users/x/harnesses/docs`) or a
/// target names it (`docs`, `~/harnesses/docs`).
fn last_part(folder: &str) -> Option<&str> {
    folder
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|part| !part.is_empty() && *part != "~")
}

/// Which harness of `rows` is `launch`'s start. `known`: every harness the
/// lamp already knows, never this start's.
pub fn pick(rows: &[Started], launch: &Launch, known: &BTreeSet<String>) -> Pick {
    let named: Vec<&Started> = rows
        .iter()
        .filter(|row| row.name.as_deref() == Some(launch.name.as_str()))
        .collect();
    match named.as_slice() {
        [one] if !known.contains(&one.id) => return Pick::Named(one.id.clone()),
        [] => {}
        _ => return Pick::Two,
    }
    // A folder this wide only ever says "not sure", never takes a row: wider
    // is the safe way (a cloud agent has none, and its miss is not sure).
    let Some(folder) = last_part(&launch.target.project) else {
        return Pick::Maybe;
    };
    let maybe = rows.iter().any(|row| {
        row.folder.as_deref().and_then(last_part) == Some(folder)
            && !known.contains(&row.id)
            && !row.name.as_deref().is_some_and(lamp_made)
    });
    if maybe { Pick::Maybe } else { Pick::None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{EngineKind, Target};

    const OURS: &str = "11111111-1111-4111-8111-111111111111";
    const THEIRS: &str = "33333333-3333-4333-8333-333333333333";
    const OTHER: &str = "44444444-4444-4444-8444-444444444444";
    const NAME: &str = "lamp-0123456789abcdef0123456789abcdef";
    const FOLDER: &str = "/Users/me/harnesses/project";

    fn launch() -> Launch {
        Launch {
            name: NAME.into(),
            prompt: "Write a short poem".into(),
            target: Target {
                machine_id: "0123456789abcdef0123456789abcdef".into(),
                project: "project".into(),
                engine: EngineKind::Claude,
            },
            session: None,
        }
    }
    fn row(id: &str, name: &str, folder: &str) -> Started {
        Started {
            id: id.into(),
            name: Some(name.into()),
            folder: Some(folder.into()),
        }
    }
    fn none() -> BTreeSet<String> {
        BTreeSet::new()
    }

    /// Its own name finds it, and the person's harness beside it, working in
    /// the same folder, is never taken.
    #[test]
    fn a_start_listed_by_the_lamps_own_name_is_the_one() {
        let rows = [row(THEIRS, "my notes", FOLDER), row(OURS, NAME, FOLDER)];
        assert_eq!(pick(&rows, &launch(), &none()), Pick::Named(OURS.into()));
    }

    /// The first builder's 01:05 guard: two of its name, or its name on a
    /// harness another task has, and neither is taken.
    #[test]
    fn two_of_its_name_are_neither_taken() {
        let twice = [row(OURS, NAME, FOLDER), row(OTHER, NAME, FOLDER)];
        assert_eq!(pick(&twice, &launch(), &none()), Pick::Two);
        let known = BTreeSet::from([THEIRS.to_owned()]);
        let held = [row(THEIRS, NAME, FOLDER)];
        assert_eq!(pick(&held, &launch(), &known), Pick::Two);
    }

    /// Twelve's HOLD on #8240, item 1: the person has worked in the same
    /// folder all along and the lamp's start never began. Their harness,
    /// working since the ask, is never taken: not sure, never "not started"
    /// either, since a titled start of the lamp's would look the same.
    #[test]
    fn a_harness_the_person_started_is_never_taken() {
        for rows in [
            vec![row(THEIRS, "my notes", FOLDER)],
            vec![row(THEIRS, "my notes", "/Users/me/code/project/")],
            vec![row(OURS, "Short poem", FOLDER)],
        ] {
            assert_eq!(pick(&rows, &launch(), &none()), Pick::Maybe, "{rows:?}");
        }
    }

    /// Nothing that could be it: another folder, a harness the lamp knows,
    /// or another start of the lamp's.
    #[test]
    fn with_nothing_that_could_be_it_the_start_is_not_listed() {
        let known = BTreeSet::from([THEIRS.to_owned()]);
        for rows in [
            vec![],
            vec![row(THEIRS, "my notes", "/Users/me/code/other")],
            vec![row(THEIRS, "my notes", FOLDER)],
            vec![row(OTHER, "lamp-00ff", FOLDER)],
        ] {
            assert_eq!(pick(&rows, &launch(), &known), Pick::None, "{rows:?}");
        }
    }

    /// `ps --json` as Harness 0.6.79 writes it; a creation under way (no
    /// agent id) is passed over, and an unreadable answer is refused.
    #[test]
    fn a_list_is_read_as_harness_writes_it() {
        let listed = serde_json::json!({"harnesses":[
            {"name":NAME,"id":OURS,"agent":"claude","machine":"Mac","state":"working",
             "folder":FOLDER,"branch":null,"startedBy":"person",
             "yours":false,"self":false,"lastActive":"2026-11-21T00:41:00.000Z"},
            {"name":"lamp-ab","id":"c-123","state":"starting","folder":null,"lastActive":null}
        ]});
        let rows = parse(listed.to_string().as_bytes()).unwrap();
        assert_eq!(rows, [row(OURS, NAME, FOLDER)]);
        assert!(parse(b"not json").is_err());
        assert!(parse(br#"{"no":"rows"}"#).is_err());
    }
}
