//! Local daily windows and a durable ledger of consumed chances.
//!
//! A successful claim is persisted before a caller may ask for any output.
//! Cancellation, refusal or a crash after that claim does not refund the day.
//! This module neither waits for a window nor sends an actuator request.
//! Callers still check freshness, policy and cancellation at the output
//! boundary; a consumed chance does not grant output authority.

use lamp_local_time::LocalTime;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::{
    collections::BTreeSet,
    fmt,
    fs::{self, File, Metadata, OpenOptions, TryLockError},
    io::{self, Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

/// Sanity range for an initialized wall clock: 2020-01-01 UTC through the
/// end of 2099. This rejects the Unix epoch and wild values; it does not
/// establish that the clock has synchronized with a trusted time source.
pub const MIN_UNIX_MS: u64 = 1_577_836_800_000;
pub const MAX_UNIX_MS_EXCLUSIVE: u64 = 4_102_444_800_000;
pub const MAX_KEYS: usize = 128;
pub const MAX_JOURNAL_BYTES: usize = 1024 * 1024;
const SCHEMA: u32 = 2;
// A checked clock plus the allowed +/-24-hour zone and an overnight
// occurrence can reach these dates. No claimed date is ever evicted.
const FIRST_DAY: i64 = (MIN_UNIX_MS / 86_400_000) as i64 - 2;
const LAST_DAY: i64 = (MAX_UNIX_MS_EXCLUSIVE / 86_400_000) as i64;
const DAY_COUNT: usize = (LAST_DAY - FIRST_DAY + 1) as usize;
const DAY_BYTES: usize = DAY_COUNT.div_ceil(8);
const MAX_NAME_BYTES: usize = 64;
const TEMP_ATTEMPTS: usize = 16;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// A live scheduled action needs both a plausible date and positive clock
/// synchronization evidence. `None` is unknown, never permission. Explicit
/// development clock injection supplies `Some(true)` at its call site.
pub fn trusted_unix_ms(unix_ms: u64, synchronized: Option<bool>) -> Option<u64> {
    (synchronized == Some(true) && (MIN_UNIX_MS..MAX_UNIX_MS_EXCLUSIVE).contains(&unix_ms))
        .then_some(unix_ms)
}

/// A nonempty half-open local window: `from` is included, `to` is excluded.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Window {
    from: u16,
    to: u16,
}

impl Window {
    /// Exact 24-hour `HH:MM` times. Equal endpoints are refused rather than
    /// interpreted as either an empty window or a whole day.
    pub fn parse(from: &str, to: &str) -> io::Result<Self> {
        fn minute(text: &str) -> io::Result<u16> {
            let bytes = text.as_bytes();
            if bytes.len() != 5
                || bytes[2] != b':'
                || ![bytes[0], bytes[1], bytes[3], bytes[4]]
                    .iter()
                    .all(u8::is_ascii_digit)
            {
                return Err(invalid("daily window times must be HH:MM"));
            }
            let hour = u16::from(bytes[0] - b'0') * 10 + u16::from(bytes[1] - b'0');
            let minute = u16::from(bytes[3] - b'0') * 10 + u16::from(bytes[4] - b'0');
            if hour >= 24 || minute >= 60 {
                return Err(invalid("daily window time is out of range"));
            }
            Ok(hour * 60 + minute)
        }
        let window = Self {
            from: minute(from)?,
            to: minute(to)?,
        };
        if window.from == window.to {
            return Err(invalid("daily window endpoints must differ"));
        }
        Ok(window)
    }

    pub fn contains(&self, local_minute: u16) -> bool {
        local_minute < 1440
            && if self.from < self.to {
                (self.from..self.to).contains(&local_minute)
            } else {
                local_minute >= self.from || local_minute < self.to
            }
    }

    /// The date on which this occurrence began. After midnight, the early
    /// portion of an overnight window belongs to the previous local date.
    pub fn occurrence_day(&self, local_day: i64, local_minute: u16) -> Option<i64> {
        if !self.contains(local_minute) {
            None
        } else if self.from > self.to && local_minute < self.to {
            local_day.checked_sub(1)
        } else {
            Some(local_day)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Moment {
    /// Local days since 1970-01-01, not a count of elapsed 24-hour periods.
    pub day: i64,
    pub minute: u16,
}

impl Moment {
    pub fn at(local: &LocalTime, unix_ms: u64) -> io::Result<Self> {
        if !(MIN_UNIX_MS..MAX_UNIX_MS_EXCLUSIVE).contains(&unix_ms) {
            return Err(invalid("daily windows require an initialized wall clock"));
        }
        // Sample a system zone just once: a zone-file replacement must not
        // supply the day from one offset and the minute from another.
        let offset = local.offset_minutes_at(unix_ms);
        if !(-1440..=1440).contains(&offset) {
            return Err(invalid("daily window UTC offset is out of range"));
        }
        let fixed = LocalTime::Offset(offset);
        Ok(Self {
            day: fixed.day(unix_ms),
            minute: fixed.minute_of_day(unix_ms),
        })
    }
}

// These are persisted records, not requests. Added fields can be ignored;
// a new required field or a changed meaning requires a schema bump. There
// are no enum-valued fields or fallback variants in this ledger.
/// Fixed date-domain bitmap. Hex keeps the on-disk representation bounded
/// even for 128 keys that each claim every supported day. Unlike a high-water
/// date, a mistaken future claim cannot consume the intervening real dates.
#[derive(Clone, Debug)]
struct Days(Vec<u8>);

impl Days {
    fn index(day: i64) -> io::Result<usize> {
        if !(FIRST_DAY..=LAST_DAY).contains(&day) {
            return Err(invalid(
                "daily journal day is outside the supported calendar",
            ));
        }
        Ok((day - FIRST_DAY) as usize)
    }

    fn one(day: i64) -> io::Result<Self> {
        let mut days = Self(vec![0; DAY_BYTES]);
        days.insert(day)?;
        Ok(days)
    }

    fn contains(&self, day: i64) -> io::Result<bool> {
        let index = Self::index(day)?;
        Ok(self.0[index / 8] & (1 << (index % 8)) != 0)
    }

    fn insert(&mut self, day: i64) -> io::Result<()> {
        let index = Self::index(day)?;
        self.0[index / 8] |= 1 << (index % 8);
        Ok(())
    }

    fn through(day: i64) -> io::Result<Self> {
        let last = Self::index(day)?;
        let mut days = Self(vec![0; DAY_BYTES]);
        days.0[..last / 8].fill(u8::MAX);
        days.0[last / 8] = u8::MAX >> (7 - last % 8);
        Ok(days)
    }
}

impl Serialize for Days {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut text = String::with_capacity(DAY_BYTES * 2);
        for byte in &self.0 {
            text.push(char::from(HEX[usize::from(byte >> 4)]));
            text.push(char::from(HEX[usize::from(byte & 15)]));
        }
        serializer.serialize_str(&text)
    }
}

impl<'de> Deserialize<'de> for Days {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Bitmap;
        impl de::Visitor<'_> for Bitmap {
            type Value = Days;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "a {}-byte lowercase hex daily bitmap", DAY_BYTES * 2)
            }

            fn visit_str<E: de::Error>(self, text: &str) -> Result<Days, E> {
                if text.len() != DAY_BYTES * 2 {
                    return Err(E::custom("daily bitmap has the wrong length"));
                }
                let nibble = |byte| match byte {
                    b'0'..=b'9' => Ok(byte - b'0'),
                    b'a'..=b'f' => Ok(byte - b'a' + 10),
                    _ => Err(E::custom("daily bitmap is not lowercase hex")),
                };
                let mut bytes = Vec::with_capacity(DAY_BYTES);
                for pair in text.as_bytes().chunks_exact(2) {
                    bytes.push((nibble(pair[0])? << 4) | nibble(pair[1])?);
                }
                let last_mask = u8::MAX >> (DAY_BYTES * 8 - DAY_COUNT);
                if bytes[DAY_BYTES - 1] & !last_mask != 0 || bytes.iter().all(|byte| *byte == 0) {
                    return Err(E::custom("daily bitmap has invalid or no claimed days"));
                }
                Ok(Days(bytes))
            }
        }
        deserializer.deserialize_str(Bitmap)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Consumed {
    skill: String,
    trigger: String,
    days: Days,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct State {
    schema: u32,
    #[serde(deserialize_with = "consumed")]
    consumed: Vec<Consumed>,
}

#[derive(Deserialize)]
struct LegacyConsumed {
    skill: String,
    trigger: String,
    day: i64,
}

#[derive(Deserialize)]
struct LegacyState {
    #[serde(deserialize_with = "consumed")]
    consumed: Vec<LegacyConsumed>,
}

fn consumed<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Entries<T>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>> de::Visitor<'de> for Entries<T> {
        type Value = Vec<T>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "at most {MAX_KEYS} daily ledger entries")
        }

        fn visit_seq<A: de::SeqAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
            let mut entries = Vec::new();
            while entries.len() < MAX_KEYS {
                let Some(entry) = access.next_element()? else {
                    return Ok(entries);
                };
                entries.push(entry);
            }
            if access.next_element::<de::IgnoredAny>()?.is_some() {
                return Err(de::Error::custom("too many daily ledger entries"));
            }
            Ok(entries)
        }
    }
    deserializer.deserialize_seq(Entries(std::marker::PhantomData))
}

fn name(text: &str, skill: bool) -> bool {
    (1..=MAX_NAME_BYTES).contains(&text.len())
        && text.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || byte == b'_'
                || (skill && byte == b'-')
        })
}

impl State {
    fn decode(bytes: &[u8]) -> io::Result<Self> {
        #[derive(Deserialize)]
        struct Version {
            schema: u32,
        }
        let version: Version =
            serde_json::from_slice(bytes).map_err(|_| invalid("daily journal is malformed"))?;
        let state: Self = match version.schema {
            SCHEMA => {
                serde_json::from_slice(bytes).map_err(|_| invalid("daily journal is malformed"))?
            }
            1 => {
                let legacy: LegacyState = serde_json::from_slice(bytes)
                    .map_err(|_| invalid("daily journal is malformed"))?;
                // V1 discarded earlier individual dates. Preserve its whole
                // consumed prefix: guessing which dates were unused could
                // replay real output. Opening alone never rewrites the file.
                let consumed = legacy
                    .consumed
                    .into_iter()
                    .map(|entry| {
                        Ok(Consumed {
                            skill: entry.skill,
                            trigger: entry.trigger,
                            days: Days::through(entry.day)?,
                        })
                    })
                    .collect::<io::Result<_>>()?;
                Self {
                    schema: SCHEMA,
                    consumed,
                }
            }
            _ => return Err(invalid("unsupported daily journal schema")),
        };
        let mut seen = BTreeSet::new();
        for entry in &state.consumed {
            if !name(&entry.skill, true)
                || !name(&entry.trigger, false)
                || !seen.insert((&entry.skill, &entry.trigger))
            {
                return Err(invalid("daily journal has invalid or duplicate keys"));
            }
        }
        Ok(state)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Stamp {
    device: u64,
    inode: u64,
    size: u64,
    modified_s: i64,
    modified_ns: i64,
    changed_s: i64,
    changed_ns: i64,
}

impl From<&Metadata> for Stamp {
    fn from(meta: &Metadata) -> Self {
        Self {
            device: meta.dev(),
            inode: meta.ino(),
            size: meta.len(),
            modified_s: meta.mtime(),
            modified_ns: meta.mtime_nsec(),
            changed_s: meta.ctime(),
            changed_ns: meta.ctime_nsec(),
        }
    }
}

fn same_file(left: &Metadata, right: &Metadata) -> bool {
    left.dev() == right.dev() && left.ino() == right.ino()
}

fn private_directory(meta: &Metadata) -> io::Result<()> {
    if !meta.is_dir() || meta.mode() & 0o7777 != 0o700 {
        return Err(invalid(
            "daily journal requires a private mode 0700 directory",
        ));
    }
    Ok(())
}

fn regular(meta: &Metadata, owner: u32) -> io::Result<()> {
    if !meta.is_file() || meta.uid() != owner || meta.mode() & 0o7777 != 0o600 || meta.nlink() != 1
    {
        return Err(invalid(
            "daily journal must be an owned, singly linked mode 0600 file",
        ));
    }
    if meta.len() > MAX_JOURNAL_BYTES as u64 {
        return Err(invalid("daily journal exceeds its byte bound"));
    }
    Ok(())
}

/// One lifetime owner of a journal in an existing, private directory. The
/// directory is locked without waiting; another owner in it is refused.
/// Parent aliases are resolved once. Directory/file replacement is checked
/// before each write, but this is not a sandbox against an uncooperative
/// process with the same user or root privileges.
#[derive(Debug)]
pub struct Journal {
    path: PathBuf,
    directory: File,
    expected: Option<Stamp>,
    state: State,
    poisoned: bool,
    #[cfg(test)]
    failure: Option<Failure>,
}

impl Journal {
    /// A missing file begins empty. Malformed, newer-schema, linked,
    /// oversized or unsafe existing files are refused without rewriting.
    /// The caller provisions the dedicated private parent directory.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        let filename = path
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|value| value.len() <= 128 && !value.chars().any(char::is_control))
            .ok_or_else(|| invalid("daily journal needs a bounded filename"))?;
        let parent = path
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let parent = fs::canonicalize(parent)?;
        let before = fs::symlink_metadata(&parent)?;
        private_directory(&before)?;
        let directory = File::open(&parent)?;
        if !same_file(&before, &directory.metadata()?) {
            return Err(invalid("daily journal directory changed during open"));
        }
        directory.try_lock().map_err(|error| match error {
            TryLockError::WouldBlock => io::Error::new(
                io::ErrorKind::WouldBlock,
                "daily journal directory is already owned",
            ),
            TryLockError::Error(error) => error,
        })?;
        let mut journal = Self {
            path: parent.join(filename),
            directory,
            expected: None,
            state: State {
                schema: SCHEMA,
                consumed: Vec::new(),
            },
            poisoned: false,
            #[cfg(test)]
            failure: None,
        };
        journal.check_directory()?;
        if let Some(expected) = journal.current()? {
            let mut file = File::open(&journal.path)?;
            regular(&file.metadata()?, journal.directory.metadata()?.uid())?;
            if Stamp::from(&file.metadata()?) != expected {
                return Err(invalid("daily journal changed during open"));
            }
            let mut bytes = Vec::with_capacity(expected.size as usize);
            (&mut file)
                .take(MAX_JOURNAL_BYTES as u64 + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() > MAX_JOURNAL_BYTES
                || Stamp::from(&file.metadata()?) != expected
                || journal.current()?.as_ref() != Some(&expected)
            {
                return Err(invalid(
                    "daily journal changed during read or exceeded its bound",
                ));
            }
            journal.state = State::decode(&bytes)?;
            // Reopening after an uncertain directory sync may see the new
            // complete file. Establish its durability before using it.
            file.sync_all()?;
            journal.directory.sync_all()?;
            journal.expected = Some(expected);
        }
        journal.verify_current()?;
        Ok(journal)
    }

    /// Consume this exact day for a stable skill/trigger-name pair. A date
    /// correction can use an unclaimed day, but cannot replay a claimed one
    /// after reload/restart. No key or day is evicted to grant another chance.
    /// Use an occurrence day obtained from a checked [`Moment`] and [`Window`].
    /// This performs synchronous local disk I/O, including durability syncs;
    /// keep it outside capture and immediate-feedback paths.
    pub fn claim(&mut self, skill: &str, trigger: &str, day: i64) -> io::Result<bool> {
        if self.poisoned {
            return Err(invalid(
                "daily journal is unavailable; reopen before retrying",
            ));
        }
        if !name(skill, true) || !name(trigger, false) {
            return Err(invalid("daily journal key is invalid or too long"));
        }
        Days::index(day)?;
        if let Err(error) = self.verify_current() {
            self.poisoned = true;
            return Err(error);
        }
        let index = self
            .state
            .consumed
            .iter()
            .position(|entry| entry.skill == skill && entry.trigger == trigger);
        if let Some(index) = index
            && self.state.consumed[index].days.contains(day)?
        {
            return Ok(false);
        }
        let mut next = self.state.clone();
        if let Some(index) = index {
            next.consumed[index].days.insert(day)?;
        } else {
            if next.consumed.len() == MAX_KEYS {
                return Err(invalid("daily journal key bound reached"));
            }
            next.consumed.push(Consumed {
                skill: skill.into(),
                trigger: trigger.into(),
                days: Days::one(day)?,
            });
        }
        let bytes = serde_json::to_vec(&next).map_err(io::Error::other)?;
        if bytes.len() > MAX_JOURNAL_BYTES {
            return Err(invalid("daily journal exceeds its byte bound"));
        }
        match self.save(&bytes) {
            Ok(stamp) => {
                self.expected = Some(stamp);
                self.state = next;
                Ok(true)
            }
            Err(error) => {
                // Before rename the previous file remains whole. After
                // rename a failed sync is indeterminate: never return true,
                // restore possibly old evidence or keep claiming in memory.
                self.poisoned = true;
                Err(error)
            }
        }
    }

    fn check_directory(&self) -> io::Result<()> {
        let named = fs::symlink_metadata(self.path.parent().unwrap())?;
        let held = self.directory.metadata()?;
        private_directory(&named)?;
        private_directory(&held)?;
        if !same_file(&named, &held) || named.uid() != held.uid() {
            return Err(invalid("daily journal directory was replaced"));
        }
        Ok(())
    }

    fn current(&self) -> io::Result<Option<Stamp>> {
        match fs::symlink_metadata(&self.path) {
            Ok(meta) => {
                regular(&meta, self.directory.metadata()?.uid())?;
                Ok(Some(Stamp::from(&meta)))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn verify_current(&self) -> io::Result<()> {
        self.check_directory()?;
        if self.current()? != self.expected {
            return Err(invalid("daily journal was changed outside its owner"));
        }
        Ok(())
    }

    fn temporary(&self) -> io::Result<Temporary> {
        for _ in 0..TEMP_ATTEMPTS {
            let sequence = NEXT_TEMP
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
                .map_err(|_| invalid("daily journal temporary sequence is exhausted"))?;
            let path = self.path.with_file_name(format!(
                ".{}.daily-{}-{sequence}.tmp",
                self.path.file_name().unwrap().to_str().unwrap(),
                std::process::id()
            ));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path)
            {
                Ok(file) => return Ok(Temporary { path, file }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(invalid("daily journal temporary names are occupied"))
    }

    fn save(&mut self, bytes: &[u8]) -> io::Result<Stamp> {
        self.verify_current()?;
        let mut temporary = self.temporary()?;
        regular(
            &temporary.file.metadata()?,
            self.directory.metadata()?.uid(),
        )?;
        #[cfg(test)]
        self.fail_at(Failure::Write)?;
        temporary.file.write_all(bytes)?;
        #[cfg(test)]
        self.fail_at(Failure::FileSync)?;
        temporary.file.sync_all()?;
        self.verify_current()?;
        let written = temporary.file.metadata()?;
        regular(&written, self.directory.metadata()?.uid())?;
        if !same_file(&written, &fs::symlink_metadata(&temporary.path)?) {
            return Err(invalid("daily journal temporary was replaced"));
        }
        #[cfg(test)]
        self.fail_at(Failure::Rename)?;
        fs::rename(&temporary.path, &self.path)?;
        #[cfg(test)]
        self.fail_at(Failure::DirectorySync)?;
        self.directory.sync_all()?;
        self.check_directory()?;
        let installed = self
            .current()?
            .ok_or_else(|| invalid("daily journal disappeared after save"))?;
        if installed != Stamp::from(&temporary.file.metadata()?) {
            return Err(invalid("daily journal changed after save"));
        }
        Ok(installed)
    }

    #[cfg(test)]
    fn fail_at(&mut self, stage: Failure) -> io::Result<()> {
        if self.failure == Some(stage) {
            self.failure = None;
            return Err(io::Error::other("injected daily journal write failure"));
        }
        Ok(())
    }
}

struct Temporary {
    path: PathBuf,
    file: File,
}

impl Drop for Temporary {
    fn drop(&mut self) {
        // Never remove another writer's replacement or a crash leftover.
        // Successful rename has already removed this temporary name.
        if let (Ok(named), Ok(held)) = (fs::symlink_metadata(&self.path), self.file.metadata())
            && same_file(&named, &held)
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Failure {
    Write,
    FileSync,
    Rename,
    DirectorySync,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let path = directory.path().join("daily.json");
        (directory, path)
    }

    fn write(path: &Path, bytes: &[u8]) {
        fs::write(path, bytes).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }

    #[test]
    fn windows_have_exact_half_open_and_overnight_boundaries() {
        let day = Window::parse("07:30", "09:00").unwrap();
        assert_eq!(day.occurrence_day(20_000, 449), None);
        assert_eq!(day.occurrence_day(20_000, 450), Some(20_000));
        assert_eq!(day.occurrence_day(20_000, 539), Some(20_000));
        assert_eq!(day.occurrence_day(20_000, 540), None);
        let night = Window::parse("22:00", "06:00").unwrap();
        assert_eq!(night.occurrence_day(20_000, 1319), None);
        assert_eq!(night.occurrence_day(20_000, 1320), Some(20_000));
        assert_eq!(night.occurrence_day(20_000, 1439), Some(20_000));
        assert_eq!(night.occurrence_day(20_001, 0), Some(20_000));
        assert_eq!(night.occurrence_day(20_001, 359), Some(20_000));
        assert_eq!(night.occurrence_day(20_001, 360), None);
        assert_eq!(night.occurrence_day(i64::MIN, 0), None);
        assert!(!night.contains(1440));
        assert!(!night.contains(u16::MAX));
        let midnight = Window::parse("23:00", "00:00").unwrap();
        assert!(midnight.contains(1439));
        assert!(!midnight.contains(0));
    }

    #[test]
    fn windows_refuse_ambiguous_or_malformed_times() {
        for bad in [
            "", "7:00", "07:0", "24:00", "09:60", " 07:00", "07:00 ", "07:00:00", "aa:bb", "０7:00",
        ] {
            assert!(Window::parse(bad, "08:00").is_err(), "{bad:?}");
            assert!(Window::parse("07:00", bad).is_err(), "{bad:?}");
        }
        assert!(Window::parse("07:00", "07:00").is_err());
        assert!(Window::parse("00:00", "00:00").is_err());
    }

    #[test]
    fn moment_rejects_unset_clock_and_observes_the_local_midnight() {
        let local = LocalTime::Offset(420);
        for invalid in [0, MIN_UNIX_MS - 1, MAX_UNIX_MS_EXCLUSIVE, u64::MAX] {
            assert!(Moment::at(&local, invalid).is_err());
        }
        assert!(Moment::at(&LocalTime::Offset(i32::MAX), MIN_UNIX_MS).is_err());
        let before = Moment::at(&local, MIN_UNIX_MS + 17 * 3_600_000 - 1).unwrap();
        let after = Moment::at(&local, MIN_UNIX_MS + 17 * 3_600_000).unwrap();
        assert_eq!(before.minute, 1439);
        assert_eq!(after.minute, 0);
        assert_eq!(before.day + 1, after.day);
        assert!(Moment::at(&local, MAX_UNIX_MS_EXCLUSIVE - 1).is_ok());
    }

    #[test]
    fn new_york_dst_gap_does_not_invent_a_window_and_repeated_hour_claims_once() {
        let local = LocalTime::zone("America/New_York").unwrap();
        // 2026-03-08 06:59 and 07:00 UTC: 01:59 EST then 03:00 EDT.
        let before = Moment::at(&local, 1_772_953_140_000).unwrap();
        let after = Moment::at(&local, 1_772_953_200_000).unwrap();
        assert_eq!((before.minute, after.minute), (119, 180));
        assert_eq!(before.day, after.day);
        let skipped = Window::parse("02:00", "03:00").unwrap();
        assert!(!skipped.contains(before.minute));
        assert!(!skipped.contains(after.minute));
        // 2026-11-01 05:30 and 06:30 UTC are both local 01:30.
        let first = Moment::at(&local, 1_793_511_000_000).unwrap();
        let repeated = Moment::at(&local, 1_793_514_600_000).unwrap();
        assert_eq!(first, repeated);
        let window = Window::parse("01:00", "02:00").unwrap();
        let (_directory, path) = fixture();
        let mut journal = Journal::open(&path).unwrap();
        assert!(
            journal
                .claim(
                    "desk-routine",
                    "wake",
                    window.occurrence_day(first.day, first.minute).unwrap()
                )
                .unwrap()
        );
        drop(journal);
        let mut journal = Journal::open(&path).unwrap();
        assert!(
            !journal
                .claim(
                    "desk-routine",
                    "wake",
                    window
                        .occurrence_day(repeated.day, repeated.minute)
                        .unwrap()
                )
                .unwrap()
        );
        let tomorrow = Moment::at(&local, 1_793_601_000_000).unwrap();
        assert_eq!(tomorrow.day, first.day + 1);
        assert!(journal.claim("desk-routine", "wake", tomorrow.day).unwrap());
    }

    #[test]
    fn forward_claim_and_date_correction_preserve_exact_days_across_restart() {
        let (_directory, path) = fixture();
        let mut journal = Journal::open(&path).unwrap();
        assert!(!path.exists());
        assert!(journal.claim("desk-routine", "night", 20_000).unwrap());
        assert!(journal.claim("desk-routine", "morning", 20_000).unwrap());
        assert!(journal.claim("desk-routine", "night", 40_000).unwrap());
        let saved = fs::read(&path).unwrap();
        assert!(!journal.claim("desk-routine", "night", 20_000).unwrap());
        assert!(!journal.claim("desk-routine", "night", 40_000).unwrap());
        assert_eq!(fs::read(&path).unwrap(), saved);
        drop(journal);
        let mut journal = Journal::open(&path).unwrap();
        // Reordering named triggers cannot reset a claim. The corrected
        // next real date remains usable despite the mistaken future date.
        assert!(!journal.claim("desk-routine", "morning", 20_000).unwrap());
        assert!(journal.claim("desk-routine", "night", 20_001).unwrap());
        assert!(!journal.claim("desk-routine", "night", 20_000).unwrap());
        assert!(!journal.claim("desk-routine", "night", 40_000).unwrap());
        assert!(journal.claim("another-routine", "night", 20_000).unwrap());
        drop(journal);
        let mut journal = Journal::open(&path).unwrap();
        assert!(!journal.claim("desk-routine", "night", 20_001).unwrap());
        assert!(journal.claim("desk-routine", "night", 20_002).unwrap());
    }

    #[test]
    fn legacy_high_water_migrates_conservatively_only_after_a_new_claim() {
        let (_directory, path) = fixture();
        let legacy =
            br#"{"schema":1,"consumed":[{"skill":"routine","trigger":"night","day":20000}]}"#;
        write(&path, legacy);
        let mut journal = Journal::open(&path).unwrap();
        for day in [FIRST_DAY, 19_999, 20_000] {
            assert!(!journal.claim("routine", "night", day).unwrap());
        }
        assert_eq!(fs::read(&path).unwrap(), legacy);
        assert!(journal.claim("routine", "night", 20_002).unwrap());
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&fs::read(&path).unwrap()).unwrap()["schema"],
            2
        );
        drop(journal);
        let mut journal = Journal::open(&path).unwrap();
        assert!(!journal.claim("routine", "night", 19_999).unwrap());
        assert!(!journal.claim("routine", "night", 20_002).unwrap());
        // Only legacy evidence is a prefix: later exact claims do not fill
        // an intervening date that the journal knows has never been claimed.
        assert!(journal.claim("routine", "night", 20_001).unwrap());
    }

    #[test]
    fn the_full_supported_calendar_and_all_maximum_keys_fit_without_eviction() {
        let days = Days::through(LAST_DAY).unwrap();
        let mut state = State {
            schema: SCHEMA,
            consumed: Vec::new(),
        };
        for index in 0..MAX_KEYS {
            state.consumed.push(Consumed {
                skill: "s".repeat(MAX_NAME_BYTES),
                trigger: format!("{:0>64}", index),
                days: days.clone(),
            });
        }
        let bytes = serde_json::to_vec(&state).unwrap();
        assert!(bytes.len() < MAX_JOURNAL_BYTES, "{} bytes", bytes.len());
        let decoded = State::decode(&bytes).unwrap();
        for entry in decoded.consumed {
            for day in FIRST_DAY..=LAST_DAY {
                assert!(entry.days.contains(day).unwrap());
            }
        }
        let (_directory, path) = fixture();
        let mut journal = Journal::open(path).unwrap();
        for day in [i64::MIN, FIRST_DAY - 1, LAST_DAY + 1, i64::MAX] {
            assert!(journal.claim("routine", "night", day).is_err());
        }
        assert!(journal.claim("routine", "night", FIRST_DAY).unwrap());
        assert!(journal.claim("routine", "night", LAST_DAY).unwrap());
    }

    #[test]
    fn a_second_owner_is_refused_until_the_first_is_dropped() {
        let (_directory, path) = fixture();
        let owner = Journal::open(&path).unwrap();
        assert_eq!(
            Journal::open(&path).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        drop(owner);
        Journal::open(&path).unwrap();
    }

    #[test]
    fn pre_rename_failures_preserve_bytes_and_poison_the_owner_without_consuming_a_day() {
        for failure in [Failure::Write, Failure::FileSync, Failure::Rename] {
            let (directory, path) = fixture();
            let mut journal = Journal::open(&path).unwrap();
            assert!(journal.claim("routine", "night", 20_000).unwrap());
            let saved = fs::read(&path).unwrap();
            journal.failure = Some(failure);
            assert!(journal.claim("routine", "night", 20_001).is_err());
            assert_eq!(fs::read(&path).unwrap(), saved);
            assert!(journal.state.consumed[0].days.contains(20_000).unwrap());
            assert!(!journal.state.consumed[0].days.contains(20_001).unwrap());
            assert!(journal.claim("routine", "night", 20_001).is_err());
            assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
            drop(journal);
            assert!(
                Journal::open(&path)
                    .unwrap()
                    .claim("routine", "night", 20_001)
                    .unwrap()
            );
        }
    }

    #[test]
    fn failed_directory_sync_never_grants_a_claim_or_rolls_back_installed_evidence() {
        let (_directory, path) = fixture();
        let mut journal = Journal::open(&path).unwrap();
        assert!(journal.claim("routine", "night", 20_000).unwrap());
        journal.failure = Some(Failure::DirectorySync);
        assert!(journal.claim("routine", "night", 20_001).is_err());
        assert!(journal.claim("routine", "night", 20_002).is_err());
        assert!(
            State::decode(&fs::read(&path).unwrap()).unwrap().consumed[0]
                .days
                .contains(20_001)
                .unwrap()
        );
        drop(journal);
        assert!(
            !Journal::open(&path)
                .unwrap()
                .claim("routine", "night", 20_001)
                .unwrap()
        );
    }

    #[test]
    fn malformed_newer_or_duplicate_state_is_never_replaced() {
        let (directory, path) = fixture();
        for bytes in [
            b"not JSON".as_slice(),
            br#"{"schema":3,"consumed":[]}"#,
            br#"{"schema":0,"consumed":[]}"#,
            br#"{"schema":"1","consumed":[]}"#,
            br#"{"schema":1,"consumed":[{"skill":"routine","trigger":"night","day":1.5}]}"#,
            br#"{"schema":1,"consumed":[{"skill":"routine","trigger":"night","day":20000},{"skill":"routine","trigger":"night","day":19999}]}"#,
            br#"{"schema":1,"consumed":[{"skill":"routine","trigger":{"kind":"later"},"day":20}]}"#,
        ] {
            write(&path, bytes);
            assert!(Journal::open(&path).is_err());
            assert_eq!(fs::read(&path).unwrap(), bytes);
            assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
        }
    }

    #[test]
    fn added_fields_keep_known_days_and_maximum_length_keys_survive_a_save() {
        let (_directory, path) = fixture();
        write(&path, br#"{"schema":1,"later":{"kind":"extra"},"consumed":[{"skill":"routine","trigger":"night","day":20000,"later":true}]}"#);
        let mut journal = Journal::open(&path).unwrap();
        assert!(!journal.claim("routine", "night", 20_000).unwrap());
        let skill = "s".repeat(MAX_NAME_BYTES);
        let trigger = "t".repeat(MAX_NAME_BYTES);
        assert!(journal.claim(&skill, &trigger, 20_000).unwrap());
        drop(journal);
        let mut journal = Journal::open(&path).unwrap();
        assert!(!journal.claim(&skill, &trigger, 20_000).unwrap());
        assert!(!journal.claim("routine", "night", 20_000).unwrap());
        let written: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert!(written.get("later").is_none());
        assert!(written["consumed"][0].get("later").is_none());
    }

    #[test]
    fn current_bitmap_values_and_duplicate_keys_are_refused_without_repair() {
        let (_directory, path) = fixture();
        let good = serde_json::to_value(State {
            schema: SCHEMA,
            consumed: vec![Consumed {
                skill: "routine".into(),
                trigger: "night".into(),
                days: Days::one(20_000).unwrap(),
            }],
        })
        .unwrap();
        let mut padding = "0".repeat(DAY_BYTES * 2);
        padding.replace_range(padding.len() - 2.., "80");
        for bad in [
            serde_json::json!(null),
            serde_json::json!({"kind":"future"}),
            serde_json::json!("0".repeat(DAY_BYTES * 2)),
            serde_json::json!("f".repeat(DAY_BYTES * 2 - 1)),
            serde_json::json!("f".repeat(DAY_BYTES * 2 + 1)),
            serde_json::json!("F".repeat(DAY_BYTES * 2)),
            serde_json::json!(padding),
        ] {
            let mut value = good.clone();
            value["consumed"][0]["days"] = bad;
            let bytes = serde_json::to_vec(&value).unwrap();
            write(&path, &bytes);
            assert!(Journal::open(&path).is_err());
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
        let mut duplicate = good.clone();
        duplicate["consumed"]
            .as_array_mut()
            .unwrap()
            .push(good["consumed"][0].clone());
        write(&path, &serde_json::to_vec(&duplicate).unwrap());
        assert!(Journal::open(&path).is_err());

        let mut additive = good;
        additive["later"] = serde_json::json!({"kind":"future"});
        additive["consumed"][0]["later"] = serde_json::json!(true);
        let bytes = serde_json::to_vec(&additive).unwrap();
        write(&path, &bytes);
        let mut journal = Journal::open(&path).unwrap();
        assert!(!journal.claim("routine", "night", 20_000).unwrap());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert!(journal.claim("routine", "night", 19_999).unwrap());
    }

    #[test]
    fn synchronization_is_required_even_for_a_plausible_wall_clock() {
        for synchronized in [None, Some(false)] {
            assert_eq!(trusted_unix_ms(MIN_UNIX_MS, synchronized), None);
            assert_eq!(
                trusted_unix_ms(MAX_UNIX_MS_EXCLUSIVE - 1, synchronized),
                None
            );
        }
        assert_eq!(trusted_unix_ms(MIN_UNIX_MS, Some(true)), Some(MIN_UNIX_MS));
        assert_eq!(
            trusted_unix_ms(MAX_UNIX_MS_EXCLUSIVE - 1, Some(true)),
            Some(MAX_UNIX_MS_EXCLUSIVE - 1)
        );
        for invalid in [0, MIN_UNIX_MS - 1, MAX_UNIX_MS_EXCLUSIVE, u64::MAX] {
            assert_eq!(trusted_unix_ms(invalid, Some(true)), None);
        }
    }

    #[test]
    fn key_and_file_bounds_never_evict_existing_days() {
        let (_directory, path) = fixture();
        let mut state = State {
            schema: SCHEMA,
            consumed: (0..MAX_KEYS)
                .map(|index| Consumed {
                    skill: "routine".into(),
                    trigger: format!("named_{index}"),
                    days: Days::one(20_000).unwrap(),
                })
                .collect(),
        };
        let mut bytes = serde_json::to_vec(&state).unwrap();
        bytes.resize(MAX_JOURNAL_BYTES, b' ');
        write(&path, &bytes);
        let mut journal = Journal::open(&path).unwrap();
        assert!(journal.claim("routine", "extra", 20_000).is_err());
        for bad in ["", "../escape", "Upper", "a-b", "two words", "é"] {
            assert!(journal.claim("routine", bad, 20_001).is_err());
        }
        assert!(journal.claim(&"s".repeat(65), "night", 20_001).is_err());
        assert!(journal.claim("routine", &"t".repeat(65), 20_001).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert!(journal.claim("routine", "named_0", 20_001).unwrap());
        drop(journal);
        state.consumed.push(Consumed {
            skill: "routine".into(),
            trigger: "extra".into(),
            days: Days::one(20_000).unwrap(),
        });
        let excess = serde_json::to_vec(&state).unwrap();
        write(&path, &excess);
        assert!(Journal::open(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), excess);
        bytes.push(b' ');
        write(&path, &bytes);
        assert!(Journal::open(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn unsafe_files_and_replacement_outside_the_owner_are_refused() {
        let (directory, path) = fixture();
        let outside = directory.path().join("outside.json");
        write(&outside, br#"{"schema":1,"consumed":[]}"#);
        symlink(&outside, &path).unwrap();
        assert!(Journal::open(&path).is_err());
        fs::remove_file(&path).unwrap();
        fs::hard_link(&outside, &path).unwrap();
        assert!(Journal::open(&path).is_err());
        fs::remove_file(&path).unwrap();
        let mut journal = Journal::open(&path).unwrap();
        assert!(journal.claim("routine", "night", 20_000).unwrap());
        fs::rename(&outside, &path).unwrap();
        let replaced = fs::read(&path).unwrap();
        assert!(journal.claim("routine", "night", 20_001).is_err());
        assert_eq!(fs::read(&path).unwrap(), replaced);
        drop(journal);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(Journal::open(&path).is_err());
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o755)).unwrap();
        assert!(Journal::open(&path).is_err());
    }

    #[test]
    fn unique_temporaries_do_not_overwrite_or_remove_other_files() {
        let (_directory, path) = fixture();
        let journal = Journal::open(&path).unwrap();
        let first = journal.temporary().unwrap();
        let second = journal.temporary().unwrap();
        assert_ne!(first.path, second.path);
        let first_path = first.path.clone();
        fs::remove_file(&first.path).unwrap();
        symlink(&second.path, &first.path).unwrap();
        drop(first);
        assert!(fs::symlink_metadata(first_path).unwrap().is_symlink());
        assert!(second.path.exists());
    }
}
