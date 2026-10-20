//! Durable, source-owned restrictions, separate from a person's settings.
//!
//! Success means the restriction was committed, never that a mixer or ring
//! applied it. The caller applies the effective minimum and reports hardware
//! confirmation separately. Releasing a restriction supplies no saved volume
//! or light setting to restore.

use lamp_schedule::store::Store;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fmt, io, path::Path};

pub const MAX_SOURCES: usize = 8;
pub const MAX_STATE_BYTES: usize = 16_384;
pub const MAX_AGE_MS: u64 = 5_000;
pub const MAX_DURATION_MS: u64 = 24 * 60 * 60 * 1_000;
/// An initialized clock, not evidence that it is synchronized. The caller
/// supplies `None` whenever it cannot trust its wall-clock sample.
pub const MIN_UNIX_MS: u64 = 1_577_836_800_000;
pub const MAX_UNIX_MS_EXCLUSIVE: u64 = 4_102_444_800_000;
const SCHEMA: u32 = 1;

/// A new restriction or a replacement for the same source. A replacement
/// needs a different token and a strictly later issue time. Tokens are opaque
/// identifiers, not credentials; callers must not reuse them for another put.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Put {
    pub source: String,
    pub token: String,
    pub issued_unix_ms: u64,
    pub valid_until_unix_ms: u64,
    pub light_channel_max: Option<u16>,
    pub volume_raw_max: Option<u8>,
}

impl Put {
    fn check(&self) -> Result<(), Error> {
        identity(&self.source, &self.token)?;
        if self
            .light_channel_max
            .is_some_and(|n| !(12..=120).contains(&n))
        {
            return Err(Error::Invalid("light ceiling must be from 12 to 120"));
        }
        if self.volume_raw_max.is_some_and(|n| !(1..=100).contains(&n)) {
            return Err(Error::Invalid("volume ceiling must be from 1 to 100"));
        }
        if self.light_channel_max.is_none() && self.volume_raw_max.is_none() {
            return Err(Error::Invalid("at least one ceiling is required"));
        }
        if initialized(Some(self.issued_unix_ms)).is_none()
            || initialized(Some(self.valid_until_unix_ms)).is_none()
            || !self
                .valid_until_unix_ms
                .checked_sub(self.issued_unix_ms)
                .is_some_and(|duration| (1..=MAX_DURATION_MS).contains(&duration))
        {
            return Err(Error::Invalid(
                "deadline must be after issue time and at most 24 hours later",
            ));
        }
        Ok(())
    }
}

fn initialized(now: Option<u64>) -> Option<u64> {
    now.filter(|now| (MIN_UNIX_MS..MAX_UNIX_MS_EXCLUSIVE).contains(now))
}

fn identity(source: &str, token: &str) -> Result<(), Error> {
    if !(1..=64).contains(&source.len())
        || !source
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-_".contains(&b))
    {
        return Err(Error::Invalid(
            "source must be a plain skill name of 1 to 64 bytes",
        ));
    }
    if !(1..=64).contains(&token.len())
        || !token
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-_".contains(&b))
    {
        return Err(Error::Invalid(
            "token must be a lowercase opaque identifier of 1 to 64 bytes",
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Effective {
    pub light_channel_max: Option<u16>,
    pub volume_raw_max: Option<u8>,
}

/// The committed restrictions that the caller may publish to output owners.
/// Call `expire` with the latest trusted time before taking a new snapshot.
/// Unknown time keeps every still-active record, including across a reboot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Snapshot {
    pub revision: u64,
    pub active: Vec<Put>,
    pub effective: Effective,
}

/// Durable acceptance only. `active` is false for a retry of an already
/// cancelled or expired put; that retry never starts it again.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Accepted {
    pub revision: u64,
    pub active: bool,
    pub effective: Effective,
}

#[derive(Debug)]
pub enum Error {
    Invalid(&'static str),
    UnknownClock,
    ChangedRetry,
    Stale,
    Full,
    UnknownSource,
    TokenMismatch,
    Schema(u32),
    Corrupt,
    /// A write may have committed before its final sync failed. No acceptance
    /// may be reported; drop this owner and reopen before deciding its state.
    Storage(io::Error),
    RecoveryRequired,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(why) => f.write_str(why),
            Self::UnknownClock => f.write_str("a trusted initialized wall clock is required"),
            Self::ChangedRetry => f.write_str("the token already names a different restriction"),
            Self::Stale => f.write_str("a replacement must have a later issue time"),
            Self::Full => f.write_str("all eight source identities are already reserved"),
            Self::UnknownSource => f.write_str("the source has no restriction to remove"),
            Self::TokenMismatch => {
                f.write_str("the token does not match the source's latest restriction")
            }
            Self::Schema(schema) => write!(f, "unsupported cap state schema {schema}"),
            Self::Corrupt => {
                f.write_str("cap state is invalid; the stored document was left intact")
            }
            Self::Storage(error) => write!(f, "cap storage: {error}"),
            Self::RecoveryRequired => {
                f.write_str("cap storage requires reopen after a failed commit")
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Storage(error) => Some(error),
            _ => None,
        }
    }
}

// Stored records may gain additive fields under this schema. Keep the live
// Put strict, validate every known stored value, and require a schema bump
// for a new field that changes the meaning of a restriction.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct SavedPut {
    source: String,
    token: String,
    issued_unix_ms: u64,
    valid_until_unix_ms: u64,
    light_channel_max: Option<u16>,
    volume_raw_max: Option<u8>,
}

impl From<Put> for SavedPut {
    fn from(put: Put) -> Self {
        Self {
            source: put.source,
            token: put.token,
            issued_unix_ms: put.issued_unix_ms,
            valid_until_unix_ms: put.valid_until_unix_ms,
            light_channel_max: put.light_channel_max,
            volume_raw_max: put.volume_raw_max,
        }
    }
}

impl SavedPut {
    fn request(&self) -> Put {
        Put {
            source: self.source.clone(),
            token: self.token.clone(),
            issued_unix_ms: self.issued_unix_ms,
            valid_until_unix_ms: self.valid_until_unix_ms,
            light_channel_max: self.light_channel_max,
            volume_raw_max: self.volume_raw_max,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Status {
    Active,
    Cancelled,
    Expired,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct Entry {
    request: SavedPut,
    status: Status,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct State {
    schema: u32,
    revision: u64,
    sources: Vec<Entry>,
}

impl State {
    fn empty() -> Self {
        Self {
            schema: SCHEMA,
            revision: 0,
            sources: Vec::new(),
        }
    }

    fn decode(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > MAX_STATE_BYTES {
            return Err(Error::Corrupt);
        }
        #[derive(Deserialize)]
        struct Version {
            schema: u32,
        }
        let version: Version = serde_json::from_slice(bytes).map_err(|_| Error::Corrupt)?;
        if version.schema != SCHEMA {
            return Err(Error::Schema(version.schema));
        }
        let state: Self = serde_json::from_slice(bytes).map_err(|_| Error::Corrupt)?;
        if state.sources.len() > MAX_SOURCES || (state.revision == 0 && !state.sources.is_empty()) {
            return Err(Error::Corrupt);
        }
        let mut names = BTreeSet::new();
        for entry in &state.sources {
            entry
                .request
                .request()
                .check()
                .map_err(|_| Error::Corrupt)?;
            if !names.insert(&entry.request.source) {
                return Err(Error::Corrupt);
            }
        }
        Ok(state)
    }

    fn expire(&mut self, now: Option<u64>) {
        if let Some(now) = initialized(now) {
            for entry in &mut self.sources {
                if entry.status == Status::Active && now >= entry.request.valid_until_unix_ms {
                    entry.status = Status::Expired;
                }
            }
        }
    }

    fn snapshot(&self) -> Snapshot {
        let active: Vec<Put> = self
            .sources
            .iter()
            .filter(|entry| entry.status == Status::Active)
            .map(|entry| entry.request.request())
            .collect();
        let effective = Effective {
            light_channel_max: active.iter().filter_map(|put| put.light_channel_max).min(),
            volume_raw_max: active.iter().filter_map(|put| put.volume_raw_max).min(),
        };
        Snapshot {
            revision: self.revision,
            active,
            effective,
        }
    }
}

/// One owner of up to eight stable source identities, including tombstones.
/// Retiring an identity never silently frees it: its highest issue time and
/// latest token still refuse delayed puts and removals after restart.
///
/// The existing opaque Schedule store owns locking, sync and recovery. Its
/// internal `schedule.sqlite3` belongs in a dedicated caps directory, never
/// in the reminder owner's directory. This module supplies only bounded JSON.
pub struct Controller {
    store: Store,
    state: State,
}

impl Controller {
    /// Restore restrictions before output starts. Known-expired entries are
    /// retired durably; an unknown clock keeps them. Corrupt or newer-schema
    /// snapshots are refused without replacement or an empty-state fallback.
    pub fn open(directory: &Path, now_unix_ms: Option<u64>) -> Result<Self, Error> {
        let mut store = Store::open(directory).map_err(Error::Storage)?;
        let state = match store.load().map_err(Error::Storage)? {
            Some(bytes) => State::decode(&bytes)?,
            None => {
                let state = State::empty();
                store
                    .commit(&serde_json::to_vec(&state).map_err(|_| Error::Corrupt)?)
                    .map_err(Error::Storage)?;
                state
            }
        };
        let mut controller = Self { store, state };
        controller.expire(now_unix_ms)?;
        Ok(controller)
    }

    fn healthy(&self) -> Result<(), Error> {
        if self.store.poisoned() {
            Err(Error::RecoveryRequired)
        } else {
            Ok(())
        }
    }

    fn commit(&mut self, mut next: State) -> Result<(), Error> {
        self.healthy()?;
        if next == self.state {
            return Ok(());
        }
        next.revision = self
            .state
            .revision
            .checked_add(1)
            .ok_or(Error::Invalid("cap revision is exhausted"))?;
        let bytes = serde_json::to_vec(&next).map_err(|_| Error::Corrupt)?;
        if bytes.len() > MAX_STATE_BYTES {
            return Err(Error::Invalid("cap state exceeds its byte bound"));
        }
        self.store.commit(&bytes).map_err(Error::Storage)?;
        self.state = next;
        Ok(())
    }

    fn accepted(&self, source: &str) -> Accepted {
        Accepted {
            revision: self.state.revision,
            active: self
                .state
                .sources
                .iter()
                .any(|entry| entry.request.source == source && entry.status == Status::Active),
            effective: self.state.snapshot().effective,
        }
    }

    /// Commit a fresh put. An exact retry only observes its current status;
    /// it may succeed after five seconds or with an unknown clock because it
    /// neither starts a new restriction nor extends the old deadline.
    pub fn put(&mut self, put: Put, now_unix_ms: Option<u64>) -> Result<Accepted, Error> {
        self.healthy()?;
        put.check()?;
        let index = self
            .state
            .sources
            .iter()
            .position(|entry| entry.request.source == put.source);
        if let Some(index) = index {
            let previous = &self.state.sources[index].request;
            if previous.token == put.token {
                if previous.request() != put {
                    return Err(Error::ChangedRetry);
                }
                self.expire(now_unix_ms)?;
                return Ok(self.accepted(&put.source));
            }
            if put.issued_unix_ms <= previous.issued_unix_ms {
                return Err(Error::Stale);
            }
        } else if self.state.sources.len() >= MAX_SOURCES {
            return Err(Error::Full);
        }
        let now = initialized(now_unix_ms).ok_or(Error::UnknownClock)?;
        if now
            .checked_sub(put.issued_unix_ms)
            .is_none_or(|age| age > MAX_AGE_MS)
        {
            return Err(Error::Invalid(
                "issue time must be no more than five seconds old and not in the future",
            ));
        }
        if put.valid_until_unix_ms <= now {
            return Err(Error::Invalid("the restriction has already expired"));
        }
        let mut next = self.state.clone();
        next.expire(Some(now));
        let entry = Entry {
            request: put.clone().into(),
            status: Status::Active,
        };
        match index {
            Some(index) => next.sources[index] = entry,
            None => next.sources.push(entry),
        }
        self.commit(next)?;
        Ok(self.accepted(&put.source))
    }

    /// Remove only the latest matching token. Already-removed tokens succeed
    /// without another commit. Explicit release is allowed with unknown time;
    /// it never contains a previous output setting to restore.
    pub fn remove(
        &mut self,
        source: &str,
        token: &str,
        now_unix_ms: Option<u64>,
    ) -> Result<Accepted, Error> {
        self.healthy()?;
        identity(source, token)?;
        let index = self
            .state
            .sources
            .iter()
            .position(|entry| entry.request.source == source)
            .ok_or(Error::UnknownSource)?;
        if self.state.sources[index].request.token != token {
            return Err(Error::TokenMismatch);
        }
        let mut next = self.state.clone();
        next.expire(now_unix_ms);
        if next.sources[index].status == Status::Active {
            next.sources[index].status = Status::Cancelled;
        }
        self.commit(next)?;
        Ok(self.accepted(source))
    }

    /// Retire all due entries in one commit. Once retired, clock rollback
    /// cannot revive them. Unknown time makes no change and performs no write.
    pub fn expire(&mut self, now_unix_ms: Option<u64>) -> Result<Snapshot, Error> {
        self.healthy()?;
        let mut next = self.state.clone();
        next.expire(now_unix_ms);
        self.commit(next)?;
        self.snapshot()
    }

    /// This is domain state, not a bus message or hardware receipt. Following
    /// a storage failure, even a snapshot requires reopening the owner first.
    pub fn snapshot(&self) -> Result<Snapshot, Error> {
        self.healthy()?;
        Ok(self.state.snapshot())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000_000;

    fn put(source: &str, token: &str, issued: u64) -> Put {
        Put {
            source: source.into(),
            token: token.into(),
            issued_unix_ms: issued,
            valid_until_unix_ms: issued + 60_000,
            light_channel_max: Some(30),
            volume_raw_max: Some(40),
        }
    }

    fn controller() -> (tempfile::TempDir, Controller) {
        let root = tempfile::tempdir().unwrap();
        let controller = Controller::open(&root.path().join("caps"), Some(NOW)).unwrap();
        (root, controller)
    }

    fn saved(root: &Path, bytes: &[u8]) {
        Store::open(&root.join("caps"))
            .unwrap()
            .commit(bytes)
            .unwrap();
    }

    fn bytes(root: &Path) -> Vec<u8> {
        Store::open(&root.join("caps"))
            .unwrap()
            .load()
            .unwrap()
            .unwrap()
    }

    #[test]
    fn independent_caps_compose_and_removing_one_does_not_remove_another() {
        let (_root, mut caps) = controller();
        let first = put("bedtime", "one", NOW);
        let mut second = put("quiet", "two", NOW);
        second.light_channel_max = None;
        second.volume_raw_max = Some(20);
        let mut third = put("dim", "three", NOW);
        third.light_channel_max = Some(12);
        third.volume_raw_max = None;
        assert_eq!(caps.put(first.clone(), Some(NOW)).unwrap().revision, 1);
        caps.put(second, Some(NOW)).unwrap();
        let accepted = caps.put(third, Some(NOW)).unwrap();
        assert_eq!(
            accepted.effective,
            Effective {
                light_channel_max: Some(12),
                volume_raw_max: Some(20)
            }
        );
        assert!(accepted.active);
        let removed = caps.remove("quiet", "two", Some(NOW)).unwrap();
        assert!(!removed.active);
        assert_eq!(
            removed.effective,
            Effective {
                light_channel_max: Some(12),
                volume_raw_max: Some(40)
            }
        );
        caps.remove("dim", "three", Some(NOW)).unwrap();
        assert_eq!(caps.snapshot().unwrap().active, [first]);
    }

    #[test]
    fn exact_retries_are_idempotent_but_changed_retries_are_refused() {
        let (root, mut caps) = controller();
        let original = put("bedtime", "one", NOW);
        let accepted = caps.put(original.clone(), Some(NOW)).unwrap();
        assert_eq!(
            caps.put(original.clone(), Some(NOW + 10_000)).unwrap(),
            accepted
        );
        assert_eq!(caps.put(original.clone(), None).unwrap(), accepted);
        let mut changed = original.clone();
        changed.volume_raw_max = Some(30);
        assert!(matches!(
            caps.put(changed, Some(NOW)),
            Err(Error::ChangedRetry)
        ));
        drop(caps);
        let mut reopened = Controller::open(&root.path().join("caps"), None).unwrap();
        assert_eq!(reopened.put(original, None).unwrap(), accepted);
    }

    #[test]
    fn replacement_needs_newer_issue_time_and_old_tokens_cannot_remove_it() {
        let (_root, mut caps) = controller();
        let old = put("bedtime", "one", NOW);
        caps.put(old.clone(), Some(NOW)).unwrap();
        assert!(matches!(
            caps.put(put("bedtime", "two", NOW), Some(NOW)),
            Err(Error::Stale)
        ));
        let mut newer = put("bedtime", "two", NOW + 1);
        newer.volume_raw_max = Some(25);
        let accepted = caps.put(newer.clone(), Some(NOW + 1)).unwrap();
        assert!(matches!(caps.put(old, Some(NOW + 1)), Err(Error::Stale)));
        assert!(matches!(
            caps.remove("bedtime", "one", Some(NOW + 1)),
            Err(Error::TokenMismatch)
        ));
        assert_eq!(caps.snapshot().unwrap().active, [newer]);
        assert_eq!(caps.snapshot().unwrap().revision, accepted.revision);
    }

    #[test]
    fn cancellation_survives_restart_and_late_puts_cannot_revive_it() {
        let (root, mut caps) = controller();
        let original = put("bedtime", "one", NOW);
        caps.put(original.clone(), Some(NOW)).unwrap();
        let removed = caps.remove("bedtime", "one", None).unwrap();
        assert_eq!(removed.effective, Effective::default());
        assert!(!removed.active);
        assert_eq!(caps.remove("bedtime", "one", None).unwrap(), removed);
        assert_eq!(caps.put(original.clone(), Some(NOW + 1)).unwrap(), removed);
        drop(caps);
        let mut reopened = Controller::open(&root.path().join("caps"), None).unwrap();
        assert_eq!(reopened.put(original, None).unwrap(), removed);
        assert!(matches!(
            reopened.put(put("bedtime", "old", NOW - 1), Some(NOW)),
            Err(Error::Stale)
        ));
        assert!(
            reopened
                .put(put("bedtime", "next", NOW + 1), Some(NOW + 1))
                .unwrap()
                .active
        );
    }

    #[test]
    fn source_budget_includes_tombstones_and_never_evicts_old_identities() {
        let (_root, mut caps) = controller();
        for n in 0..MAX_SOURCES {
            let source = format!("source-{n}");
            caps.put(put(&source, "one", NOW), Some(NOW)).unwrap();
            caps.remove(&source, "one", Some(NOW)).unwrap();
        }
        let before = caps.snapshot().unwrap();
        assert!(matches!(
            caps.put(put("ninth", "one", NOW), Some(NOW)),
            Err(Error::Full)
        ));
        assert_eq!(caps.snapshot().unwrap(), before);
        assert!(
            caps.put(put("source-0", "two", NOW + 1), Some(NOW + 1))
                .unwrap()
                .active
        );
    }

    #[test]
    fn expiry_is_durable_and_clock_rollback_does_not_revive_a_retired_cap() {
        let (root, mut caps) = controller();
        let request = put("bedtime", "one", NOW);
        caps.put(request.clone(), Some(NOW)).unwrap();
        assert_eq!(
            caps.expire(Some(NOW + 59_999)).unwrap().active.as_slice(),
            std::slice::from_ref(&request)
        );
        let expired = caps.expire(Some(NOW + 60_000)).unwrap();
        assert!(expired.active.is_empty());
        assert_eq!(caps.expire(Some(NOW + 60_001)).unwrap(), expired);
        assert!(
            !caps
                .put(request.clone(), Some(NOW + 60_000))
                .unwrap()
                .active
        );
        drop(caps);
        let mut reopened = Controller::open(&root.path().join("caps"), Some(NOW)).unwrap();
        assert_eq!(reopened.snapshot().unwrap(), expired);
        assert!(!reopened.put(request, Some(NOW)).unwrap().active);
    }

    #[test]
    fn restore_retires_known_expired_entries_but_keeps_protection_when_time_is_unknown() {
        let (root, mut caps) = controller();
        caps.put(put("bedtime", "one", NOW), Some(NOW)).unwrap();
        drop(caps);
        let mut unknown = Controller::open(&root.path().join("caps"), None).unwrap();
        let kept = unknown.snapshot().unwrap();
        for now in [
            None,
            Some(0),
            Some(MIN_UNIX_MS - 1),
            Some(MAX_UNIX_MS_EXCLUSIVE),
        ] {
            assert_eq!(unknown.expire(now).unwrap(), kept);
            assert!(matches!(
                unknown.put(put("new", "one", NOW), now),
                Err(Error::UnknownClock)
            ));
            assert!(matches!(
                unknown.put(put("bedtime", "two", NOW + 1), now),
                Err(Error::UnknownClock)
            ));
        }
        drop(unknown);
        let expired = Controller::open(&root.path().join("caps"), Some(NOW + 60_000)).unwrap();
        assert!(expired.snapshot().unwrap().active.is_empty());
        assert_eq!(expired.snapshot().unwrap().revision, kept.revision + 1);
    }

    #[test]
    fn invalid_names_caps_and_clock_bounds_do_not_mutate_state() {
        let (_root, mut caps) = controller();
        let base = put("bedtime", "one", NOW);
        let before = caps.snapshot().unwrap();
        let mut refused = Vec::new();
        for source in ["", "skill:bedtime", "../bedtime", "BEDTIME", "two words"] {
            let mut request = base.clone();
            request.source = source.into();
            refused.push(request);
        }
        for token in ["", "two words", "../token", "UPPER"] {
            let mut request = base.clone();
            request.token = token.into();
            refused.push(request);
        }
        let mut long = base.clone();
        long.source = "a".repeat(65);
        refused.push(long);
        let mut long = base.clone();
        long.token = "a".repeat(65);
        refused.push(long);
        for light in [0, 11, 121, 255] {
            let mut request = base.clone();
            request.light_channel_max = Some(light);
            refused.push(request);
        }
        for volume in [0, 101, 255] {
            let mut request = base.clone();
            request.volume_raw_max = Some(volume);
            refused.push(request);
        }
        let mut neither = base.clone();
        neither.light_channel_max = None;
        neither.volume_raw_max = None;
        refused.push(neither);
        for issued in [NOW + 1, NOW - MAX_AGE_MS - 1, u64::MIN, u64::MAX] {
            let mut request = base.clone();
            request.issued_unix_ms = issued;
            refused.push(request);
        }
        for until in [NOW, NOW - 1, NOW + MAX_DURATION_MS + 1, u64::MAX] {
            let mut request = base.clone();
            request.valid_until_unix_ms = until;
            refused.push(request);
        }
        for request in refused {
            assert!(caps.put(request, Some(NOW)).is_err());
            assert_eq!(caps.snapshot().unwrap(), before);
        }
        let mut old = base.clone();
        old.issued_unix_ms = NOW - MAX_AGE_MS;
        assert!(caps.put(old, Some(NOW)).is_ok());
        let mut longest = put("long", "one", NOW);
        longest.valid_until_unix_ms = NOW + MAX_DURATION_MS;
        longest.light_channel_max = Some(120);
        longest.volume_raw_max = Some(100);
        assert!(caps.put(longest, Some(NOW)).is_ok());
    }

    #[test]
    fn removing_unknown_or_wrong_tokens_changes_nothing() {
        let (_root, mut caps) = controller();
        caps.put(put("bedtime", "one", NOW), Some(NOW)).unwrap();
        let before = caps.snapshot().unwrap();
        assert!(matches!(
            caps.remove("other", "one", Some(NOW)),
            Err(Error::UnknownSource)
        ));
        assert!(matches!(
            caps.remove("bedtime", "wrong", Some(NOW)),
            Err(Error::TokenMismatch)
        ));
        assert!(matches!(
            caps.remove("bedtime", "../one", Some(NOW)),
            Err(Error::Invalid(_))
        ));
        assert_eq!(caps.snapshot().unwrap(), before);
    }

    #[test]
    fn live_puts_refuse_unknown_and_duplicate_fields() {
        let request = put("bedtime", "one", NOW);
        let json = serde_json::to_string(&request).unwrap();
        let extra = format!("{{\"extra\":true,{}", &json[1..]);
        assert!(serde_json::from_str::<Put>(&extra).is_err());
        let duplicate = format!("{{\"token\":\"other\",{}", &json[1..]);
        assert!(serde_json::from_str::<Put>(&duplicate).is_err());
    }

    #[test]
    fn saved_additive_fields_are_tolerated_and_known_values_remain_validated() {
        let (root, mut caps) = controller();
        let request = put("bedtime", "one", NOW);
        caps.put(request.clone(), Some(NOW)).unwrap();
        drop(caps);
        let mut value: serde_json::Value = serde_json::from_slice(&bytes(root.path())).unwrap();
        value["future_document_note"] = true.into();
        value["sources"][0]["future_entry_note"] = true.into();
        value["sources"][0]["request"]["future_request_note"] = true.into();
        saved(root.path(), &serde_json::to_vec(&value).unwrap());
        let caps = Controller::open(&root.path().join("caps"), None).unwrap();
        assert_eq!(caps.snapshot().unwrap().active, [request]);
        drop(caps);
        value["sources"][0]["request"]["volume_raw_max"] = 101.into();
        let bad = serde_json::to_vec(&value).unwrap();
        saved(root.path(), &bad);
        assert!(matches!(
            Controller::open(&root.path().join("caps"), None),
            Err(Error::Corrupt)
        ));
        assert_eq!(bytes(root.path()), bad);
    }

    #[test]
    fn newer_schema_corruption_and_unknown_status_are_never_overwritten() {
        let (root, mut caps) = controller();
        caps.put(put("bedtime", "one", NOW), Some(NOW)).unwrap();
        drop(caps);
        let good: serde_json::Value = serde_json::from_slice(&bytes(root.path())).unwrap();
        let future = b"{\"schema\":2,\"entirely_new_state\":true}";
        saved(root.path(), future);
        assert!(matches!(
            Controller::open(&root.path().join("caps"), None),
            Err(Error::Schema(2))
        ));
        assert_eq!(bytes(root.path()), future);
        let mut unknown = good.clone();
        unknown["sources"][0]["status"] = "future_status".into();
        let mut duplicate = good.clone();
        duplicate["sources"]
            .as_array_mut()
            .unwrap()
            .push(good["sources"][0].clone());
        let mut oversized = good.clone();
        oversized["ignored"] = "a".repeat(MAX_STATE_BYTES).into();
        for invalid in [
            b"{".to_vec(),
            serde_json::to_vec(&unknown).unwrap(),
            serde_json::to_vec(&duplicate).unwrap(),
            serde_json::to_vec(&oversized).unwrap(),
        ] {
            saved(root.path(), &invalid);
            assert!(matches!(
                Controller::open(&root.path().join("caps"), None),
                Err(Error::Corrupt)
            ));
            assert_eq!(bytes(root.path()), invalid);
        }
    }

    #[test]
    fn a_failed_store_commit_is_not_accepted_and_requires_recovery() {
        let (root, mut caps) = controller();
        let original = put("bedtime", "one", NOW);
        caps.put(original.clone(), Some(NOW)).unwrap();
        // The real Store checks that the owned directory is still in place.
        // Moving it deterministically fails before commit without a fake fsync.
        let moved = root.path().join("moved");
        std::fs::rename(root.path().join("caps"), &moved).unwrap();
        assert!(matches!(
            caps.put(put("bedtime", "two", NOW + 1), Some(NOW + 1)),
            Err(Error::Storage(_))
        ));
        assert!(matches!(caps.snapshot(), Err(Error::RecoveryRequired)));
        assert!(matches!(
            caps.remove("bedtime", "one", None),
            Err(Error::RecoveryRequired)
        ));
        drop(caps);
        let recovered = Controller::open(&moved, None).unwrap();
        assert_eq!(recovered.snapshot().unwrap().active, [original]);
    }
}
