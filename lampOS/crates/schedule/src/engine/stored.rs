//! Tolerant reads belong only to persisted state. Shared time and request
//! types stay strict for live messages and delivery-ledger validation.
use super::{Error, Item, Receipt, SCHEMA, State, Unread, storage};
use crate::time::{Deadline, Elapsed, Repeat, Weekday};
use serde::{Deserialize, Deserializer, de::IgnoredAny};
use serde_json::{Value, value::RawValue};

pub(super) enum Read {
    Current(Box<State>, bool),
    Newer(u64),
}

#[derive(Deserialize)]
struct Header {
    schema: u64,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum CachedReceipt {
    Known(Box<Receipt>),
    Unreadable(IgnoredAny),
}

#[derive(Deserialize)]
struct Cache {
    last_receipt: Option<CachedReceipt>,
}

pub(super) fn read(bytes: &[u8]) -> Result<Read, Error> {
    let header: Header = serde_json::from_slice(bytes).map_err(storage)?;
    if header.schema > u64::from(SCHEMA) {
        return Ok(Read::Newer(header.schema));
    }
    // Decode the original bytes, not a Value round trip: duplicate known
    // fields remain errors. Only the cached receipt can be discarded.
    let mut state: State = serde_json::from_slice(bytes).map_err(storage)?;
    // A reminder or timer this build can't read (a newer build's value with
    // no `schema` bump, or a zone its time zone rules don't know) never stops
    // the part: it is passed over with a line, never sounded here, and kept
    // in the store as it is (#1680). Only its `id` reaches the journal.
    #[derive(Deserialize)]
    struct Rows<'a> {
        #[serde(borrow)]
        items: Vec<&'a RawValue>,
    }
    let Rows { items } = serde_json::from_slice(bytes).map_err(storage)?;
    state.items.clear();
    for raw in items {
        let why = match serde_json::from_str::<Item>(raw.get()) {
            Ok(item) if !time_unread(&item) => {
                state.items.push(item);
                continue;
            }
            Ok(_) => "is set in a zone, or by zone rules, this build's time zone rules don't know",
            Err(_) => "is not one this build reads",
        };
        let id = serde_json::from_str::<Value>(raw.get())
            .ok()
            .and_then(|item| item["id"].as_u64());
        eprintln!(
            "schedule: stored reminder {} {why}; it is kept in the store as it is, for the build that wrote it, and this build does not sound it",
            id.map_or("without an id".to_owned(), |id| id.to_string())
        );
        state.unread.push(Unread {
            id,
            json: raw.get().to_owned(),
        });
    }
    let cache: Cache = serde_json::from_slice(bytes).map_err(storage)?;
    let mut dropped = false;
    match cache.last_receipt {
        Some(CachedReceipt::Known(receipt)) => state.last_receipt = Some(*receipt),
        Some(CachedReceipt::Unreadable(_)) => dropped = true,
        None => {}
    }
    // A future request or reply cannot be used as this build's retry cache.
    // Keep the committed revision: an old guard cannot run the mutation again.
    if state.check().is_err() && state.last_receipt.is_some() {
        state.last_receipt = None;
        dropped = true;
    }
    state.check()?;
    Ok(Read::Current(Box::new(state), dropped))
}

/// An item whose deadline or snooze this build's time zone rules don't
/// agree with: a zone they don't know, or a newer tzdb's rules for one they
/// do (an offset that doesn't match, [`Deadline::zone_rules_differ`]). It
/// can't be told when it rings, so it is never rung wrong (#1680).
fn time_unread(item: &Item) -> bool {
    std::iter::once(&item.deadline)
        .chain(&item.snooze)
        .any(Deadline::zone_rules_differ)
}

/// The stored items this build reads; the others are found again by
/// [`read`], as [`State::unread`].
pub(super) fn readable_items<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<Item>, D::Error> {
    let raw = Vec::<Box<RawValue>>::deserialize(deserializer)?;
    Ok(raw
        .iter()
        .filter_map(|item| serde_json::from_str::<Item>(item.get()).ok())
        .collect())
}

#[derive(Deserialize)]
struct SavedDeadline {
    unix_ms: i64,
    elapsed: Option<SavedElapsed>,
    repeat: Option<SavedRepeat>,
    local: String,
}

#[derive(Deserialize)]
struct SavedElapsed {
    boot_id: String,
    at_boot_ms: u64,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SavedRepeat {
    Daily {
        time: String,
        zone: String,
        date: String,
    },
    Weekly {
        weekday: Weekday,
        time: String,
        zone: String,
        date: String,
    },
}

impl From<SavedDeadline> for Deadline {
    fn from(saved: SavedDeadline) -> Self {
        Self {
            unix_ms: saved.unix_ms,
            local: saved.local,
            elapsed: saved.elapsed.map(|elapsed| Elapsed {
                boot_id: elapsed.boot_id,
                at_boot_ms: elapsed.at_boot_ms,
            }),
            repeat: saved.repeat.map(|repeat| match repeat {
                SavedRepeat::Daily { time, zone, date } => Repeat::Daily { time, zone, date },
                SavedRepeat::Weekly {
                    weekday,
                    time,
                    zone,
                    date,
                } => Repeat::Weekly {
                    weekday,
                    time,
                    zone,
                    date,
                },
            }),
        }
    }
}

pub(super) fn deadline<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Deadline, D::Error> {
    SavedDeadline::deserialize(deserializer).map(Into::into)
}

pub(super) fn snooze<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Deadline>, D::Error> {
    Option::<SavedDeadline>::deserialize(deserializer).map(|saved| saved.map(Into::into))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every value a stored reminder can hold, by `schema`. A new one can't
    /// be read by the build before it, so it raises [`SCHEMA`] and this line
    /// is written again: the `match`es below stop compiling on a new variant,
    /// and the line pins the vocabulary to the schema (#1680).
    #[test]
    fn a_new_stored_value_comes_with_a_schema_bump() {
        use Weekday as W;
        let weekdays = [
            W::Monday,
            W::Tuesday,
            W::Wednesday,
            W::Thursday,
            W::Friday,
            W::Saturday,
            W::Sunday,
        ];
        for value in weekdays {
            match value {
                W::Monday
                | W::Tuesday
                | W::Wednesday
                | W::Thursday
                | W::Friday
                | W::Saturday
                | W::Sunday => {}
            }
        }
        let parts = || {
            (
                "07:00".to_owned(),
                "Etc/UTC".to_owned(),
                "2026-10-19".to_owned(),
            )
        };
        let repeats = [
            {
                let (time, zone, date) = parts();
                Repeat::Daily { time, zone, date }
            },
            {
                let (time, zone, date) = parts();
                Repeat::Weekly {
                    weekday: W::Monday,
                    time,
                    zone,
                    date,
                }
            },
        ];
        for value in &repeats {
            match value {
                Repeat::Daily { .. } | Repeat::Weekly { .. } => {}
            }
        }
        // What this build reads them as, too.
        let saved = SavedRepeat::Daily {
            time: String::new(),
            zone: String::new(),
            date: String::new(),
        };
        match saved {
            SavedRepeat::Daily { .. } | SavedRepeat::Weekly { .. } => {}
        }
        let weekdays: Vec<_> = weekdays
            .iter()
            .map(|day| {
                serde_json::to_value(day)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect();
        let repeats: Vec<_> = repeats
            .iter()
            .map(|repeat| {
                serde_json::to_value(repeat).unwrap()["kind"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect();
        let vocabulary = format!(
            "repeat: {}; weekday: {}",
            repeats.join(" "),
            weekdays.join(" ")
        );
        assert_eq!(
            (SCHEMA, vocabulary.as_str()),
            (
                1,
                "repeat: daily weekly; weekday: monday tuesday wednesday thursday friday saturday sunday"
            ),
            "a new stored value needs a `schema` bump (see SCHEMA): raise it, then write this line again"
        );
    }
}
