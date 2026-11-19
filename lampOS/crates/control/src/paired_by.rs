//! Who made each pairing, as the side file says, and the computers' daily
//! notes counts read from it (tracker row C157, the first builder's yes of
//! 17 Nov: a pairing made from the owner's app counts as the owner's
//! confirmation; a keeper's `lamp-control code` never does).
//!
//! **The side file's contract.** This module only reads it; the pairing
//! side writes it (a later change). `<state-dir>/paired-by.json`
//! ([`FILE`]), `/var/lib/lampos-control/paired-by.json` in the product: a
//! regular file, never a link, at most [`FILE_MOST_BYTES`], written whole
//! and renamed over, 0600 like `clients.json`:
//!
//! ```json
//! {"v": 1, "clients": {"c_3f9a…": {"by": "app", "paired_at": 1794000000}}}
//! ```
//!
//! - `by` is `"app"` only for a pairing whose code the owner's app asked
//!   for; a keeper's code is `"keeper"` or not listed. Only `"app"` counts.
//! - `paired_at` is that pairing's own, in Unix seconds, as `clients.json`
//!   keeps it: a mark whose `paired_at` is not the computer's now (the same
//!   key paired again since) is stale and does not count.
//! - Any other `v`, a file that is missing, a link, too large or not this
//!   shape: no mark at all, so nothing is counted for anyone.
//!
//! **The count read** (the owner's socket, `{"note_changes":
//! {"since_unix_ms": N}}`, from the voice alone): for each computer marked
//! `app` whose notes changed since then, `{client, name, changes,
//! last_unix_ms}`. A count and the time of the last change only: never a
//! key, a prefix or a value.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Value, json};

use crate::notes::Notes;
use crate::pairing::Client;

/// The side file's name in the state folder.
pub const FILE: &str = "paired-by.json";
/// The side file's largest: 16 computers' marks fit in far less.
pub const FILE_MOST_BYTES: u64 = 64 * 1024;
/// The side file's format version.
const VERSION: u64 = 1;
/// The owner's socket's op.
pub const OP: &str = "note_changes";

/// The computers the side file in `state` marks as paired from the owner's
/// app, each with the `paired_at` of that pairing; empty when it says none
/// or cannot be read as the contract has it.
pub fn app_paired(state: &Path) -> BTreeMap<String, u64> {
    let file = state.join(FILE);
    let Ok(meta) = std::fs::symlink_metadata(&file) else {
        return BTreeMap::new();
    };
    if !meta.file_type().is_file() || meta.len() > FILE_MOST_BYTES {
        eprintln!(
            "lamp-control: {FILE} is not a plain file of at most {FILE_MOST_BYTES} bytes; no pairing counts as the app's"
        );
        return BTreeMap::new();
    }
    let Ok(bytes) = std::fs::read(&file) else {
        return BTreeMap::new();
    };
    marks(&bytes)
}

/// [`app_paired`]'s reading of the file's bytes.
fn marks(bytes: &[u8]) -> BTreeMap<String, u64> {
    let Ok(whole) = serde_json::from_slice::<Value>(bytes) else {
        return BTreeMap::new();
    };
    if whole["v"] != VERSION {
        return BTreeMap::new();
    }
    let Some(clients) = whole["clients"].as_object() else {
        return BTreeMap::new();
    };
    clients
        .iter()
        .filter(|(_, mark)| mark["by"] == "app")
        .filter_map(|(id, mark)| Some((id.clone(), mark["paired_at"].as_u64()?)))
        .collect()
}

/// The answer to [`OP`]: each paired computer marked `app` for its
/// pairing now, not the local agent, whose notes changed since `since_ms`.
pub fn note_changes(
    clients: &[Client],
    marks: &BTreeMap<String, u64>,
    notes: Option<&Notes>,
    since_ms: u64,
) -> Value {
    let computers: Vec<Value> = notes
        .into_iter()
        .flat_map(|notes| {
            clients
                .iter()
                .filter(|client| !client.is_local())
                .filter(|client| marks.get(&client.id) == Some(&client.paired_at))
                .filter_map(move |client| {
                    let (changes, last) = notes.changes_since(&client.id, since_ms);
                    (changes > 0).then(|| {
                        json!({
                            "client": client.id,
                            "name": client.name,
                            "changes": changes,
                            "last_unix_ms": last,
                        })
                    })
                })
        })
        .collect();
    json!({ "computers": computers })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::PathBuf;

    fn client(id: &str, paired_at: u64) -> Client {
        Client {
            id: id.into(),
            name: format!("{id} laptop"),
            key: "a2V5".into(),
            grants: BTreeSet::new(),
            unknown_grants: BTreeSet::new(),
            paired_at,
        }
    }

    struct Dir(PathBuf);
    impl Dir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "lamp-control-paired-by-{name}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Notes with one kept change for each of `ids`.
    fn changed(dir: &Path, ids: &[&str]) -> Notes {
        let notes = Notes::new(dir.join("notes"));
        for id in ids {
            let step = notes.set(id, "secret_token_name", &json!("v")).unwrap();
            step.wait().unwrap();
        }
        notes
    }

    /// A pairing the app made is counted; a keeper's code, a stale mark
    /// (paired again since) and an unmarked computer never are; and the
    /// answer names no key (C157).
    #[test]
    fn only_a_pairing_the_app_made_is_counted_and_no_key_is_said() {
        let dir = Dir::new("app");
        std::fs::write(
            dir.0.join(FILE),
            json!({"v": 1, "clients": {
                "c_app": {"by": "app", "paired_at": 100},
                "c_keeper": {"by": "keeper", "paired_at": 100},
                "c_stale": {"by": "app", "paired_at": 99},
            }})
            .to_string(),
        )
        .unwrap();
        let clients = [
            client("c_app", 100),
            client("c_keeper", 100),
            client("c_stale", 100),
            client("c_none", 100),
        ];
        let notes = changed(&dir.0, &["c_app", "c_keeper", "c_stale", "c_none"]);
        let answer = note_changes(&clients, &app_paired(&dir.0), Some(&notes), 0);
        let computers = answer["computers"].as_array().unwrap();
        assert_eq!(computers.len(), 1, "{answer}");
        assert_eq!(computers[0]["client"], "c_app");
        assert_eq!(computers[0]["changes"], 1);
        assert!(
            !answer.to_string().contains("secret_token_name"),
            "{answer}"
        );
    }

    /// No side file, a link, another version or a bad shape: no mark, so
    /// nothing is counted.
    #[test]
    fn without_a_readable_side_file_nothing_is_counted() {
        let dir = Dir::new("none");
        let clients = [client("c_app", 100)];
        let notes = changed(&dir.0, &["c_app"]);
        let counted = |dir: &Path| note_changes(&clients, &app_paired(dir), Some(&notes), 0);
        assert_eq!(counted(&dir.0), json!({"computers": []}));
        let good = json!({"v": 1, "clients": {"c_app": {"by": "app", "paired_at": 100}}});
        for bad in [
            json!({"v": 2, "clients": {"c_app": {"by": "app", "paired_at": 100}}}),
            json!({"v": 1, "clients": [["c_app", "app"]]}),
            json!({"v": 1, "clients": {"c_app": {"by": "app"}}}),
        ] {
            std::fs::write(dir.0.join(FILE), bad.to_string()).unwrap();
            assert_eq!(counted(&dir.0), json!({"computers": []}), "{bad}");
        }
        let elsewhere = dir.0.join("elsewhere.json");
        std::fs::write(&elsewhere, good.to_string()).unwrap();
        std::fs::remove_file(dir.0.join(FILE)).unwrap();
        std::os::unix::fs::symlink(&elsewhere, dir.0.join(FILE)).unwrap();
        assert_eq!(counted(&dir.0), json!({"computers": []}));
        std::fs::remove_file(dir.0.join(FILE)).unwrap();
        std::fs::write(dir.0.join(FILE), good.to_string()).unwrap();
        assert_eq!(counted(&dir.0)["computers"].as_array().unwrap().len(), 1);
    }

    /// The local agent is never counted, nor a computer with no change.
    #[test]
    fn the_local_agent_and_a_computer_with_no_change_are_not_listed() {
        let dir = Dir::new("local");
        let mut agent = client("c_agent", 100);
        agent.key.clear();
        let marks = BTreeMap::from([("c_agent".to_owned(), 100), ("c_quiet".to_owned(), 100)]);
        let notes = changed(&dir.0, &["c_agent"]);
        let answer = note_changes(&[agent, client("c_quiet", 100)], &marks, Some(&notes), 0);
        assert_eq!(answer, json!({"computers": []}));
        assert_eq!(note_changes(&[], &marks, None, 0), json!({"computers": []}));
    }
}
