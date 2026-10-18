//! Tolerant reads belong only to persisted state. Shared time and request
//! types stay strict for live messages and delivery-ledger validation.
use super::{Error, Receipt, SCHEMA, State, storage};
use crate::time::{Deadline, Elapsed, Repeat, Weekday};
use serde::{Deserialize, Deserializer, de::IgnoredAny};

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
