//! The lamp's memory as it lies on the lamp's own disk.
//!
//! One directory for each person, and in it two kinds of file. A journal for
//! each local day (`journal-YYYY-MM-DD.jsonl`) holds every exchange the lamp
//! took part in: what it was asked, as the provider wrote it down, and what
//! it answered. `kept.jsonl` holds what it was asked to keep. Both are text,
//! one JSON object to a line, and nothing else is ever written here: no
//! audio, no picture.
//!
//! The disk is the only copy. Nothing of the text is held in this process
//! between requests, so a process that is killed has nothing to lose but the
//! line it was writing, and whoever deletes a file by hand has deleted it.
//! The one thing kept in memory is the next id for each person.
//!
//! A line is added with one `write` and then made durable, so a power cut
//! leaves whole lines and at most one cut-off line at the end of one file.
//! Such a line, or any other that does not read as an entry, is skipped and
//! counted and the rest of the file is used. A removal writes the file anew
//! beside the old one and renames it into place, so a reader sees the file
//! before or after and never part of either.

/// Where the lamp is in time (#820).
pub use lamp_local_time::LocalTime;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    cmp::Reverse,
    collections::BTreeMap,
    fmt,
    fs::{self, DirBuilder, File, OpenOptions, TryLockError},
    io::{self, BufRead, BufReader, Read, Write},
    os::unix::fs::{DirBuilderExt, FileExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

/// The most that is kept of what a person said in one exchange, of what the
/// lamp answered, and of one thing it was asked to keep. Longer text is cut,
/// never refused.
pub const HEARD_BYTES: usize = 600;
pub const SAID_BYTES: usize = 800;
pub const TEXT_BYTES: usize = 300;
/// What one person can ask the lamp to keep. The oldest goes when one more
/// is added.
pub const KEPT_ENTRIES: usize = 200;
/// The words of a question that are looked for. A question is a few words;
/// this bounds the work one request can ask for.
pub const QUERY_WORDS: usize = 16;

const KEPT_FILE: &str = "kept.jsonl";
const DAY_MS: i128 = 86_400_000;

/// Whose memory. Only these ids exist, so that an id can be a directory's
/// name without any of it being read as a path: `[a-z0-9_-]{1,32}`.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Person(String);

impl Person {
    /// Whose memory it is until the lamp can tell people apart.
    pub const EVERYONE: &'static str = "everyone";

    pub fn new(id: &str) -> Option<Self> {
        let plain = (1..=32).contains(&id.len())
            && id.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-'
            });
        plain.then(|| Self(id.into()))
    }

    pub fn everyone() -> Self {
        Self(Self::EVERYONE.into())
    }

    /// The scope beside this person's that holds the rules they gave the
    /// lamp (#813): `r-<hex>` for `p-<hex>`, `r-everyone` for the room's.
    /// An exchange may name rules of this scope only among its sources.
    pub fn rules(&self) -> Option<Self> {
        Self::new(&format!(
            "r-{}",
            self.0.strip_prefix("p-").unwrap_or(&self.0)
        ))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A day where the lamp stands, counted from 1970-01-01. Only days from then
/// to the end of the year 9999 are days here: the journal's file names have
/// four digits for the year.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Day(i64);

/// 9999-12-31.
const LAST_DAY: i64 = 2_932_896;

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    // The year begins in March, so that the leap day is its last.
    let year = if month <= 2 { year - 1 } else { year };
    let (era, of_era) = (year.div_euclid(400), year.rem_euclid(400));
    let of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    era * 146_097 + of_era * 365 + of_era / 4 - of_era / 100 + of_year - 719_468
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let (era, of_era) = (days.div_euclid(146_097), days.rem_euclid(146_097));
    let year = (of_era - of_era / 1_460 + of_era / 36_524 - of_era / 146_096) / 365;
    let of_year = of_era - (365 * year + year / 4 - year / 100);
    let month = (5 * of_year + 2) / 153;
    let day = of_year - (153 * month + 2) / 5 + 1;
    let month = if month < 10 { month + 3 } else { month - 9 };
    (year + era * 400 + i64::from(month <= 2), month, day)
}

/// The day it is at this moment for a clock this many minutes east of UTC,
/// whatever the moment.
fn local_day(unix_ms: u64, utc_offset_minutes: i32) -> i64 {
    let local = i128::from(unix_ms) + i128::from(utc_offset_minutes) * 60_000;
    local.div_euclid(DAY_MS) as i64
}

impl Day {
    /// `YYYY-MM-DD`, written exactly so, of a day the calendar has.
    pub fn parse(text: &str) -> Option<Self> {
        let bytes = text.as_bytes();
        if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
            return None;
        }
        let number = |from: usize, to: usize| {
            bytes[from..to].iter().try_fold(0i64, |sum, byte| {
                byte.is_ascii_digit()
                    .then(|| sum * 10 + i64::from(byte - b'0'))
            })
        };
        let date = (number(0, 4)?, number(5, 7)?, number(8, 10)?);
        let day = days_from_civil(date.0, date.1, date.2);
        ((0..=LAST_DAY).contains(&day) && civil_from_days(day) == date).then_some(Self(day))
    }

    /// The local day of a moment, if it is one a journal can be named for.
    pub fn of(at_unix_ms: u64, utc_offset_minutes: i32) -> Option<Self> {
        let day = local_day(at_unix_ms, utc_offset_minutes);
        (0..=LAST_DAY).contains(&day).then_some(Self(day))
    }

    /// The day this many calendar days before this one, if a journal can be
    /// named for it: retention counts local days, not 24-hour spans, which a
    /// day of summer time's change is not (#844).
    pub fn before(self, days: u32) -> Option<Self> {
        let day = self.0 - i64::from(days);
        (0..=LAST_DAY).contains(&day).then_some(Self(day))
    }

    /// [`Self::of`], with the offset the lamp's local time has at that
    /// moment: a zone's summer time counts on the days it is kept (#820).
    pub fn at(at_unix_ms: u64, local: &LocalTime) -> Option<Self> {
        Self::of(at_unix_ms, local.offset_minutes_at(at_unix_ms))
    }
}

impl fmt::Display for Day {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (year, month, day) = civil_from_days(self.0);
        write!(formatter, "{year:04}-{month:02}-{day:02}")
    }
}

/// One exchange, and its line in a journal.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Exchange {
    pub id: u64,
    pub at_unix_ms: u64,
    pub heard: String,
    pub said: String,
}

/// One thing the lamp was asked to keep, and its line in `kept.jsonl`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Kept {
    pub id: u64,
    pub at_unix_ms: u64,
    pub text: String,
}

/// Something recalled.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Entry {
    Exchange(Exchange),
    Kept(Kept),
}

impl Entry {
    pub fn id(&self) -> u64 {
        match self {
            Self::Exchange(exchange) => exchange.id,
            Self::Kept(kept) => kept.id,
        }
    }

    pub fn at_unix_ms(&self) -> u64 {
        match self {
            Self::Exchange(exchange) => exchange.at_unix_ms,
            Self::Kept(kept) => kept.at_unix_ms,
        }
    }

    /// The bytes of its text: what `max_bytes` counts.
    pub fn bytes(&self) -> usize {
        match self {
            Self::Exchange(exchange) => exchange.heard.len() + exchange.said.len(),
            Self::Kept(kept) => kept.text.len(),
        }
    }

    fn texts(&self) -> [&str; 2] {
        match self {
            Self::Exchange(exchange) => [&exchange.heard, &exchange.said],
            Self::Kept(kept) => [&kept.text, ""],
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Exchange,
    Kept,
}

/// What was forgotten: which entry, never what it said.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct Removed {
    pub kind: Kind,
    pub id: u64,
    pub at_unix_ms: u64,
}

/// What one person's memory holds, in numbers.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Status {
    pub person: String,
    pub exchanges: u64,
    pub kept: u64,
    pub bytes: u64,
    pub oldest_day: Option<String>,
    pub newest_day: Option<String>,
    pub skipped_lines: u64,
}

/// What opening the store found, for the line the process logs as it starts.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Found {
    pub people: usize,
    pub skipped_lines: u64,
    /// Journal days deleted for their age.
    pub expired_days: usize,
    /// Copies left by a removal that was cut off.
    pub unfinished_files: usize,
}

/// Why nothing was added.
#[derive(Debug)]
pub enum Refused {
    /// Nothing was left of the text once its whitespace was collapsed.
    Empty,
    /// The moment given is before 1970 or after the year 9999 on the lamp's
    /// clock.
    Time,
    Disk(io::Error),
}

impl From<io::Error> for Refused {
    fn from(error: io::Error) -> Self {
        Self::Disk(error)
    }
}

/// What to recall.
#[derive(Clone, Copy, Debug)]
pub struct Ask<'a> {
    pub query: Option<&'a str>,
    pub day: Option<Day>,
    pub limit: usize,
    pub max_bytes: usize,
}

pub struct Options {
    pub dir: PathBuf,
    /// A journal day is deleted once it is more than this many days before
    /// today.
    pub keep_days: u32,
    /// Where the day turns: minutes east of UTC (`--utc-offset-minutes`) or
    /// a zone (`--zone`), whose offset is taken at each moment (#820).
    pub local: LocalTime,
}

pub struct Store {
    dir: PathBuf,
    keep_days: i64,
    local: LocalTime,
    /// The latest local day this store has been told it is.
    today: i64,
    /// The id the next entry of each person gets.
    next: BTreeMap<Person, u64>,
    found: Found,
    /// The directory, held locked for as long as the store is open.
    _held: File,
}

/// Text of at most `most` bytes, with every run of whitespace or control
/// characters one space and none at either end. A character that does not
/// fit whole ends the text.
pub fn tidy(text: &str, most: usize) -> String {
    let mut tidy = String::with_capacity(text.len().min(most));
    for c in text.chars() {
        let c = if c.is_control() || c.is_whitespace() {
            ' '
        } else {
            c
        };
        if c == ' ' && (tidy.is_empty() || tidy.ends_with(' ')) {
            continue;
        }
        if tidy.len() + c.len_utf8() > most {
            break;
        }
        tidy.push(c);
    }
    if tidy.ends_with(' ') {
        tidy.pop();
    }
    tidy
}

/// The words of a question: lower case, two characters or more, each once.
fn words_of(query: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    for word in query.to_lowercase().split(|c: char| !c.is_alphanumeric()) {
        if word.chars().count() >= 2
            && words.len() < QUERY_WORDS
            && !words.iter().any(|w| w == word)
        {
            words.push(word.into());
        }
    }
    words
}

/// How many of the words this entry has in it, as whole words.
fn matched(words: &[String], entry: &Entry) -> u32 {
    let mut seen = 0u32;
    for text in entry.texts() {
        for word in text.to_lowercase().split(|c: char| !c.is_alphanumeric()) {
            if let Some(at) = words.iter().position(|w| w == word) {
                seen |= 1 << at;
            }
        }
    }
    seen.count_ones()
}

/// Make the directory, private, or refuse one that others can enter.
pub fn private_dir(dir: &Path) -> io::Result<()> {
    match fs::metadata(dir) {
        Ok(found) if !found.is_dir() => Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            format!("{} is not a directory", dir.display()),
        )),
        Ok(found) if found.permissions().mode() & 0o077 != 0 => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} can be entered by its group or by everyone (mode {:03o}); what the lamp remembers is kept only where its owner alone can read: chmod 700 it",
                dir.display(),
                found.permissions().mode() & 0o777
            ),
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            DirBuilder::new().recursive(true).mode(0o700).create(dir)
        }
        Err(error) => Err(error),
    }
}

/// Make a new or removed name in this directory outlast a power cut. Not
/// every filesystem can; the entry's own bytes are already safe.
fn sync_dir(dir: &Path) {
    let _ = File::open(dir).and_then(|dir| dir.sync_all());
}

fn journal_name(day: Day) -> String {
    format!("journal-{day}.jsonl")
}

fn blank(line: &[u8]) -> bool {
    line.iter().all(u8::is_ascii_whitespace)
}

trait Line: DeserializeOwned {
    fn id(&self) -> u64;
    /// Within the limits whatever the file held: a file can have been
    /// written by hand or by a build with other limits.
    fn tidied(self) -> Self;
}

impl Line for Exchange {
    fn id(&self) -> u64 {
        self.id
    }
    fn tidied(self) -> Self {
        Self {
            heard: tidy(&self.heard, HEARD_BYTES),
            said: tidy(&self.said, SAID_BYTES),
            ..self
        }
    }
}

impl Line for Kept {
    fn id(&self) -> u64 {
        self.id
    }
    fn tidied(self) -> Self {
        Self {
            text: tidy(&self.text, TEXT_BYTES),
            ..self
        }
    }
}

struct Lines<T> {
    entries: Vec<T>,
    skipped: u64,
    bytes: u64,
}

/// Every line of the file that reads as an entry, in the file's order. A
/// file that is not there has none.
fn read<T: Line>(path: &Path) -> io::Result<Lines<T>> {
    let mut lines = Lines {
        entries: Vec::new(),
        skipped: 0,
        bytes: 0,
    };
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(lines),
        Err(error) => return Err(error),
    };
    lines.bytes = bytes.len() as u64;
    for line in bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !blank(line))
    {
        match serde_json::from_slice::<T>(line) {
            Ok(entry) => lines.entries.push(entry.tidied()),
            Err(_) => lines.skipped += 1,
        }
    }
    Ok(lines)
}

/// Add one line to the end of the file, with one `write`, and make it
/// durable.
fn add_line(dir: &Path, path: &Path, line: &str) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .read(true)
        .append(true)
        .create(true)
        .mode(0o600)
        .open(path)?;
    let before = file.metadata()?.len();
    let mut bytes = Vec::with_capacity(line.len() + 2);
    if before > 0 {
        // A line cut off by a power cut has no end. Without one the new
        // line would be joined to it and lost with it.
        let mut last = [0u8];
        file.read_exact_at(&mut last, before - 1)?;
        if last != *b"\n" {
            bytes.push(b'\n');
        }
    }
    bytes.extend_from_slice(line.as_bytes());
    bytes.push(b'\n');
    let wrote = loop {
        match file.write(&bytes) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            wrote => break wrote,
        }
    };
    if !matches!(wrote, Ok(count) if count == bytes.len()) {
        // A full disk takes part of a line. Take it back out.
        let _ = file.set_len(before);
        return Err(wrote.err().unwrap_or_else(|| {
            io::Error::new(io::ErrorKind::WriteZero, "the line was not written whole")
        }));
    }
    file.sync_data()?;
    if before == 0 {
        sync_dir(dir);
    }
    Ok(())
}

/// Put these lines in the file's place, or take the file away when there
/// are none.
fn replace(dir: &Path, path: &Path, lines: &[&[u8]]) -> io::Result<()> {
    if lines.is_empty() {
        fs::remove_file(path)?;
        sync_dir(dir);
        return Ok(());
    }
    let mut writing = path.as_os_str().to_owned();
    writing.push(".tmp");
    let writing = PathBuf::from(writing);
    let mut bytes = Vec::with_capacity(lines.iter().map(|line| line.len() + 1).sum());
    for line in lines {
        bytes.extend_from_slice(line);
        bytes.push(b'\n');
    }
    let written = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&writing)
        .and_then(|mut file| {
            file.write_all(&bytes)?;
            file.sync_all()
        })
        .and_then(|()| fs::rename(&writing, path));
    if written.is_err() {
        // The copy holds what the file holds. It is not left lying.
        let _ = fs::remove_file(&writing);
    }
    sync_dir(dir);
    written
}

/// The file without the entries of these ids, and with one more line at its
/// end if one is given. Lines that do not read as entries stay as they are:
/// what cannot be read is not this store's to throw away.
fn rewrite<T: Line>(dir: &Path, path: &Path, without: &[u64], add: Option<&str>) -> io::Result<()> {
    let bytes = fs::read(path)?;
    let mut lines: Vec<&[u8]> = bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| !blank(line))
        .filter(|line| {
            !matches!(serde_json::from_slice::<T>(line), Ok(entry) if without.contains(&entry.id()))
        })
        .collect();
    lines.extend(add.map(str::as_bytes));
    replace(dir, path, &lines)
}

/// The days this directory has a journal for, oldest first.
fn days(dir: &Path) -> io::Result<Vec<Day>> {
    let mut days = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(days),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let day = name
            .to_str()
            .and_then(|name| name.strip_prefix("journal-"))
            .and_then(|name| name.strip_suffix(".jsonl"))
            .and_then(Day::parse);
        if let Some(day) = day
            && entry.file_type()?.is_file()
        {
            days.push(day);
        }
    }
    days.sort();
    Ok(days)
}

fn people(dir: &Path) -> io::Result<Vec<Person>> {
    let mut people = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if let Some(person) = entry.file_name().to_str().and_then(Person::new)
            && entry.file_type()?.is_dir()
        {
            people.push(person);
        }
    }
    people.sort();
    Ok(people)
}

const HEALTH_LINE_BYTES: usize = 64 * 1024;

fn health_file_count<T: DeserializeOwned>(path: &Path) -> io::Result<u64> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.file_type().is_file() => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "a memory count probe requires a regular file",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    }
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "a memory count probe requires a regular file",
        ));
    }
    let mut reader = BufReader::new(file);
    let mut line = Vec::with_capacity(HEALTH_LINE_BYTES + 1);
    let mut count = 0u64;
    loop {
        line.clear();
        let read = (&mut reader)
            .take((HEALTH_LINE_BYTES + 1) as u64)
            .read_until(b'\n', &mut line)?;
        if read == 0 {
            return Ok(count);
        }
        if line.last() == Some(&b'\n') {
            line.pop();
        }
        if line.len() > HEALTH_LINE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "a memory count probe line exceeds 64 KiB",
            ));
        }
        if !blank(&line) && serde_json::from_slice::<T>(&line).is_ok() {
            count = count
                .checked_add(1)
                .ok_or_else(|| io::Error::other("the memory count overflowed"))?;
        }
    }
}

/// Read-only counts for the reporting worker. Retains at most one 64 KiB line
/// (excluding its newline); an oversized line makes counts unknown with an
/// error. Uses the store's valid person/day names and serde entry semantics,
/// including a valid final unterminated line. Does not follow person or journal
/// symlinks; refuses a nonregular or symlinked kept file. A mutation can race
/// this multi-file snapshot, so the caller must discard a raced sample.
pub(crate) fn health_counts(dir: &Path) -> io::Result<crate::health_counts::Counts> {
    let mut counts = crate::health_counts::Counts::default();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_name().to_str().and_then(Person::new).is_none()
            || !entry.file_type()?.is_dir()
        {
            continue;
        }
        let dir = entry.path();
        counts.kept = counts
            .kept
            .checked_add(health_file_count::<Kept>(&dir.join(KEPT_FILE))?)
            .ok_or_else(|| io::Error::other("the kept count overflowed"))?;
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name();
            let day = name
                .to_str()
                .and_then(|name| name.strip_prefix("journal-"))
                .and_then(|name| name.strip_suffix(".jsonl"))
                .and_then(Day::parse);
            if day.is_some() && entry.file_type()?.is_file() {
                counts.exchanges = counts
                    .exchanges
                    .checked_add(health_file_count::<Exchange>(&entry.path())?)
                    .ok_or_else(|| io::Error::other("the exchange count overflowed"))?;
            }
        }
    }
    Ok(counts)
}

/// The entries chosen for an answer: the best first, until the count or the
/// bytes asked for are reached.
struct Chosen {
    entries: Vec<Entry>,
    limit: usize,
    room: usize,
}

impl Chosen {
    /// Whether there was room. The first entry that does not fit ends the
    /// choosing: a smaller, worse one is not taken in its place.
    fn take(&mut self, entry: Entry) -> bool {
        if self.entries.len() >= self.limit || entry.bytes() > self.room {
            return false;
        }
        self.room -= entry.bytes();
        self.entries.push(entry);
        true
    }

    fn oldest_first(mut self) -> Vec<Entry> {
        self.entries
            .sort_by_key(|entry| (entry.at_unix_ms(), entry.id()));
        self.entries
    }
}

impl Store {
    /// Open the memory in this directory, making the directory if it is not
    /// there. Journal days too old to keep are deleted now.
    pub fn open(options: Options, now_unix_ms: u64) -> io::Result<Self> {
        private_dir(&options.dir)?;
        // Two processes on one directory would each count ids for
        // themselves. The lock goes with the process, however it ends.
        let held = File::open(&options.dir)?;
        if let Err(TryLockError::WouldBlock) = held.try_lock() {
            return Err(io::Error::new(
                io::ErrorKind::ResourceBusy,
                format!("{} is open in another lamp-memory", options.dir.display()),
            ));
        }
        // Check under the same directory lock used by the opt-in importer.
        // Otherwise a completed migration could race this legacy opener.
        if options.dir.join(crate::days::DATABASE).try_exists()? {
            return Err(io::Error::other(
                "this directory uses memory across days; reopen it with --memory-days",
            ));
        }
        let today = options.local.day(now_unix_ms);
        let mut store = Self {
            dir: options.dir,
            keep_days: i64::from(options.keep_days),
            local: options.local,
            today,
            next: BTreeMap::new(),
            found: Found::default(),
            _held: held,
        };
        for person in store.people()? {
            let dir = store.person_dir(&person);
            // A removal that was cut off leaves its copy. The file it was
            // to replace is whole.
            for entry in fs::read_dir(&dir)? {
                let entry = entry?;
                if entry.file_name().to_string_lossy().ends_with(".tmp")
                    && entry.file_type()?.is_file()
                {
                    fs::remove_file(entry.path())?;
                    store.found.unfinished_files += 1;
                }
            }
            store.found.expired_days += store.expire(&person)?;
            let (highest, skipped) = store.highest(&person)?;
            store.found.skipped_lines += skipped;
            store.next.insert(person, highest.saturating_add(1));
        }
        store.found.people = store.people()?.len();
        Ok(store)
    }

    pub fn found(&self) -> Found {
        self.found
    }

    fn person_dir(&self, person: &Person) -> PathBuf {
        self.dir.join(person.as_str())
    }

    /// Everyone with a directory here, in the order of their ids. Anything
    /// else in the directory is not this store's and is left alone.
    fn people(&self) -> io::Result<Vec<Person>> {
        people(&self.dir)
    }

    /// The highest id this person has on the disk, and the lines skipped in
    /// finding it.
    fn highest(&self, person: &Person) -> io::Result<(u64, u64)> {
        let dir = self.person_dir(person);
        let kept = read::<Kept>(&dir.join(KEPT_FILE))?;
        let mut highest = kept.entries.iter().map(|kept| kept.id).max().unwrap_or(0);
        let mut skipped = kept.skipped;
        for day in days(&dir)? {
            let journal = read::<Exchange>(&dir.join(journal_name(day)))?;
            highest = highest.max(journal.entries.iter().map(|e| e.id).max().unwrap_or(0));
            skipped += journal.skipped;
        }
        Ok((highest, skipped))
    }

    /// The id for this person's next entry. It is spent whether or not the
    /// entry then reaches the disk: an id is never given twice by one
    /// process, and after a restart the count goes on from the highest id
    /// still on the disk.
    fn take_id(&mut self, person: &Person) -> io::Result<u64> {
        let id = match self.next.get(person) {
            Some(id) => *id,
            None => self.highest(person)?.0.saturating_add(1),
        };
        let after = id
            .checked_add(1)
            .ok_or_else(|| io::Error::other("this person's ids are used up"))?;
        self.next.insert(person.clone(), after);
        Ok(id)
    }

    /// This person's directory, made if it is not there.
    fn made_dir(&self, person: &Person) -> io::Result<PathBuf> {
        let dir = self.person_dir(person);
        match DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => sync_dir(&self.dir),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        Ok(dir)
    }

    /// Delete this person's journal days that are too old to keep.
    fn expire(&self, person: &Person) -> io::Result<usize> {
        let dir = self.person_dir(person);
        let mut gone = 0;
        for day in days(&dir)? {
            if self.today - day.0 > self.keep_days {
                fs::remove_file(dir.join(journal_name(day)))?;
                gone += 1;
            }
        }
        if gone > 0 {
            sync_dir(&dir);
            // Someone whose every day has gone and who asked for nothing
            // to be kept has no memory here.
            let _ = fs::remove_dir(&dir);
        }
        Ok(gone)
    }

    /// Tell the store what time it is. On the first moment of a new local
    /// day the journal days now too old are deleted; the number of them is
    /// returned. The day is taken from this clock only, never from the time
    /// an entry claims: one entry dated a year ahead must not empty the
    /// journal. A day that could not be deleted is tried again the next day
    /// and when the store is next opened.
    pub fn turn_to(&mut self, now_unix_ms: u64) -> usize {
        let today = self.local.day(now_unix_ms);
        if today <= self.today {
            return 0;
        }
        self.today = today;
        let people = self.people().unwrap_or_default();
        people
            .iter()
            .map(|person| self.expire(person).unwrap_or(0))
            .sum()
    }

    /// Add an exchange to the journal of the day it happened on.
    pub fn append(
        &mut self,
        person: &Person,
        at_unix_ms: u64,
        heard: &str,
        said: &str,
        now_unix_ms: u64,
    ) -> Result<u64, Refused> {
        self.turn_to(now_unix_ms);
        let day = Day::at(at_unix_ms, &self.local).ok_or(Refused::Time)?;
        let (heard, said) = (tidy(heard, HEARD_BYTES), tidy(said, SAID_BYTES));
        if heard.is_empty() && said.is_empty() {
            return Err(Refused::Empty);
        }
        let id = self.take_id(person)?;
        let line = serde_json::to_string(&Exchange {
            id,
            at_unix_ms,
            heard,
            said,
        })
        .map_err(io::Error::other)?;
        let dir = self.made_dir(person)?;
        add_line(&dir, &dir.join(journal_name(day)), &line)?;
        Ok(id)
    }

    /// Keep this until the lamp is told to forget it.
    pub fn remember(
        &mut self,
        person: &Person,
        at_unix_ms: u64,
        text: &str,
    ) -> Result<u64, Refused> {
        Day::at(at_unix_ms, &self.local).ok_or(Refused::Time)?;
        let text = tidy(text, TEXT_BYTES);
        if text.is_empty() {
            return Err(Refused::Empty);
        }
        let id = self.take_id(person)?;
        let line = serde_json::to_string(&Kept {
            id,
            at_unix_ms,
            text,
        })
        .map_err(io::Error::other)?;
        let dir = self.made_dir(person)?;
        let path = dir.join(KEPT_FILE);
        let mut ids: Vec<u64> = read::<Kept>(&path)?
            .entries
            .iter()
            .map(|kept| kept.id)
            .collect();
        if ids.len() < KEPT_ENTRIES {
            add_line(&dir, &path, &line)?;
        } else {
            // Full: the oldest go and the new one arrives in one renamed
            // file, so the list is never seen without either.
            ids.sort_unstable();
            ids.truncate(ids.len() + 1 - KEPT_ENTRIES);
            rewrite::<Kept>(&dir, &path, &ids, Some(&line))?;
        }
        Ok(id)
    }

    /// The newest of what this person asked to be kept that fit in
    /// `max_bytes` of text, oldest first.
    pub fn kept(&self, person: &Person, max_bytes: usize) -> io::Result<Vec<Kept>> {
        let mut kept = read::<Kept>(&self.person_dir(person).join(KEPT_FILE))?.entries;
        kept.sort_by_key(|kept| Reverse(kept.id));
        let mut room = max_bytes;
        let fit = kept
            .iter()
            .take_while(|kept| {
                let fits = kept.text.len() <= room;
                room = room.saturating_sub(kept.text.len());
                fits
            })
            .count();
        kept.truncate(fit);
        kept.reverse();
        Ok(kept)
    }

    /// Look something up in this person's memory.
    ///
    /// With a day: that day's exchanges. With words: the exchanges and kept
    /// things that have at least one of the words, those with more of them
    /// first and then the newer. With both: that day's exchanges that have
    /// one of the words. With neither: the newest exchanges. Whichever it
    /// is, the best are taken until the count or the bytes are reached, and
    /// given oldest first.
    pub fn recall(&self, person: &Person, ask: &Ask<'_>) -> io::Result<Vec<Entry>> {
        let dir = self.person_dir(person);
        // Newest day first.
        let journals = match ask.day {
            Some(day) => vec![day],
            None => {
                let mut days = days(&dir)?;
                days.reverse();
                days
            }
        };
        let mut chosen = Chosen {
            entries: Vec::new(),
            limit: ask.limit,
            room: ask.max_bytes,
        };
        let newest_first = |entry: &Entry| Reverse((entry.at_unix_ms(), entry.id()));
        // A question with nothing in it is no question.
        let Some(words) = ask.query.filter(|query| !query.trim().is_empty()) else {
            // A newer day's exchanges are all newer than an older day's, so
            // reading stops at the day that fills the answer.
            for day in journals {
                let mut entries: Vec<Entry> = read::<Exchange>(&dir.join(journal_name(day)))?
                    .entries
                    .into_iter()
                    .map(Entry::Exchange)
                    .collect();
                entries.sort_by_key(newest_first);
                for entry in entries {
                    if !chosen.take(entry) {
                        return Ok(chosen.oldest_first());
                    }
                }
            }
            return Ok(chosen.oldest_first());
        };
        let words = words_of(words);
        if words.is_empty() {
            // "?" or "a": something was asked and no word of it can be
            // looked for. Nothing has it; the newest exchanges would be
            // taken for an answer to it.
            return Ok(Vec::new());
        }
        // Only the best `limit` can be chosen, so only they are held while
        // the files are read.
        let mut best: Vec<(u32, Entry)> = Vec::new();
        let consider = |best: &mut Vec<(u32, Entry)>, entries: Vec<Entry>| {
            best.extend(
                entries
                    .into_iter()
                    .map(|entry| (matched(&words, &entry), entry))
                    .filter(|(count, _)| *count > 0),
            );
            best.sort_by_key(|(count, entry)| (Reverse(*count), newest_first(entry)));
            best.truncate(ask.limit);
        };
        if ask.day.is_none() {
            let kept = read::<Kept>(&dir.join(KEPT_FILE))?.entries;
            consider(&mut best, kept.into_iter().map(Entry::Kept).collect());
        }
        for day in journals {
            let journal = read::<Exchange>(&dir.join(journal_name(day)))?.entries;
            consider(
                &mut best,
                journal.into_iter().map(Entry::Exchange).collect(),
            );
        }
        for (_, entry) in best {
            if !chosen.take(entry) {
                break;
            }
        }
        Ok(chosen.oldest_first())
    }

    /// Remove the entry with this person's highest id, an exchange or a
    /// kept thing, and say which it was. With `only`, the highest of that
    /// kind: "forget what I asked you to remember" means the last kept
    /// thing, whatever has been said since.
    pub fn forget_last(
        &mut self,
        person: &Person,
        only: Option<Kind>,
    ) -> io::Result<Option<Removed>> {
        let dir = self.person_dir(person);
        let kept_path = dir.join(KEPT_FILE);
        let kept = match only {
            Some(Kind::Exchange) => Vec::new(),
            _ => read::<Kept>(&kept_path)?.entries,
        };
        let mut last: Option<(Removed, PathBuf)> =
            kept.iter().max_by_key(|kept| kept.id).map(|kept| {
                let removed = Removed {
                    kind: Kind::Kept,
                    id: kept.id,
                    at_unix_ms: kept.at_unix_ms,
                };
                (removed, kept_path.clone())
            });
        // An exchange is filed under the day it claims, which need not be
        // the newest day's file.
        let days = match only {
            Some(Kind::Kept) => Vec::new(),
            _ => days(&dir)?,
        };
        for day in days {
            let path = dir.join(journal_name(day));
            let highest = read::<Exchange>(&path)?
                .entries
                .into_iter()
                .max_by_key(|exchange| exchange.id);
            if let Some(exchange) = highest
                && last.as_ref().is_none_or(|(last, _)| exchange.id > last.id)
            {
                let removed = Removed {
                    kind: Kind::Exchange,
                    id: exchange.id,
                    at_unix_ms: exchange.at_unix_ms,
                };
                last = Some((removed, path));
            }
        }
        let Some((removed, path)) = last else {
            return Ok(None);
        };
        match removed.kind {
            Kind::Exchange => rewrite::<Exchange>(&dir, &path, &[removed.id], None)?,
            Kind::Kept => rewrite::<Kept>(&dir, &path, &[removed.id], None)?,
        }
        // With the last file gone there is no memory of this person here.
        if fs::remove_dir(&dir).is_ok() {
            sync_dir(&self.dir);
        }
        Ok(Some(removed))
    }

    /// Empty one person's memory, or with `None` everyone's. Every file in
    /// the directory goes, read or not, and the directory with them. The
    /// count is of the entries that could be read.
    pub fn forget_all(&mut self, person: Option<&Person>) -> io::Result<u64> {
        let people = match person {
            Some(person) => vec![person.clone()],
            None => self.people()?,
        };
        let mut removed = 0;
        for person in people {
            let dir = self.person_dir(&person);
            // Counted first, and never a reason not to forget.
            let counted = self
                .status_of(&person)
                .map_or(0, |status| status.exchanges + status.kept);
            let entries = match fs::read_dir(&dir) {
                Ok(entries) => entries,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            for entry in entries {
                let entry = entry?;
                if !entry.file_type()?.is_dir() {
                    fs::remove_file(entry.path())?;
                }
            }
            sync_dir(&dir);
            let _ = fs::remove_dir(&dir);
            removed += counted;
        }
        sync_dir(&self.dir);
        Ok(removed)
    }

    fn status_of(&self, person: &Person) -> io::Result<Status> {
        let dir = self.person_dir(person);
        let kept = read::<Kept>(&dir.join(KEPT_FILE))?;
        let mut status = Status {
            person: person.as_str().into(),
            exchanges: 0,
            kept: kept.entries.len() as u64,
            bytes: kept.bytes,
            oldest_day: None,
            newest_day: None,
            skipped_lines: kept.skipped,
        };
        for day in days(&dir)? {
            let journal = read::<Exchange>(&dir.join(journal_name(day)))?;
            status.exchanges += journal.entries.len() as u64;
            status.bytes += journal.bytes;
            status.skipped_lines += journal.skipped;
            if !journal.entries.is_empty() {
                status.oldest_day.get_or_insert_with(|| day.to_string());
                status.newest_day = Some(day.to_string());
            }
        }
        Ok(status)
    }

    /// What one person's memory holds, or with `None` each person's.
    /// Someone with no directory here is not listed.
    pub fn status(&self, person: Option<&Person>) -> io::Result<Vec<Status>> {
        let people = match person {
            Some(person) if self.person_dir(person).is_dir() => vec![person.clone()],
            Some(_) => Vec::new(),
            None => self.people()?,
        };
        people.iter().map(|person| self.status_of(person)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;

    /// A directory of this test's own, removed when the test ends.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("lm-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            Self(dir)
        }

        fn open(&self, now_unix_ms: u64) -> Store {
            self.open_with(30, 0, now_unix_ms)
        }

        fn open_with(&self, keep_days: u32, utc_offset_minutes: i32, now_unix_ms: u64) -> Store {
            Store::open(
                Options {
                    dir: self.0.clone(),
                    keep_days,
                    local: LocalTime::Offset(utc_offset_minutes),
                },
                now_unix_ms,
            )
            .unwrap()
        }

        fn names(&self, person: &str) -> Vec<String> {
            let mut names: Vec<String> = fs::read_dir(self.0.join(person))
                .unwrap()
                .map(|entry| entry.unwrap().file_name().into_string().unwrap())
                .collect();
            names.sort();
            names
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Midnight UTC at the start of this day, and so many minutes on.
    fn at(day: &str, minutes: u64) -> u64 {
        Day::parse(day).unwrap().0 as u64 * 86_400_000 + minutes * 60_000
    }

    fn ask<'a>(query: Option<&'a str>, day: Option<&str>) -> Ask<'a> {
        Ask {
            query,
            day: day.map(|day| Day::parse(day).unwrap()),
            limit: 8,
            max_bytes: 4096,
        }
    }

    fn ids(entries: &[Entry]) -> Vec<u64> {
        entries.iter().map(Entry::id).collect()
    }

    fn everyone() -> Person {
        Person::everyone()
    }

    #[test]
    fn only_a_day_the_calendar_has_is_a_day_and_it_is_written_back_the_same() {
        for day in ["1970-01-01", "2024-02-29", "2026-10-13", "9999-12-31"] {
            assert_eq!(Day::parse(day).unwrap().to_string(), day);
        }
        assert_eq!(Day::parse("1970-01-01"), Some(Day(0)));
        assert_eq!(Day::parse("2026-10-13"), Some(Day(20_739)));
        assert_eq!(Day::parse("9999-12-31"), Some(Day(LAST_DAY)));
        for not in [
            "2026-02-29",
            "2026-13-01",
            "2026-00-10",
            "2026-10-00",
            "2026-10-32",
            "1969-12-31",
            "2026-1-01",
            "2026/10/13",
            "2026-10-13 ",
            "../../../etc",
            "20२6-10-13",
            "",
        ] {
            assert_eq!(Day::parse(not), None, "{not}");
        }
        // Every day of four centuries comes back as itself.
        for day in 0..146_097 {
            let (year, month, of_month) = civil_from_days(day);
            assert_eq!(days_from_civil(year, month, of_month), day);
        }
        assert_eq!(Day::of(1_791_849_600_000, 0), Day::parse("2026-10-13"));
        assert_eq!(Day::of(0, -1), None);
        assert_eq!(Day::of(u64::MAX, 0), None);
    }

    #[test]
    fn an_exchange_is_recalled_by_its_day_by_its_words_and_as_the_newest() {
        let scratch = Scratch::new("recall");
        let now = at("2026-10-13", 600);
        let mut store = scratch.open(now);
        let me = everyone();
        let first = store
            .append(
                &me,
                at("2026-10-12", 540),
                "Is it going to rain?",
                "Not today.",
                now,
            )
            .unwrap();
        let second = store
            .append(
                &me,
                at("2026-10-13", 545),
                "What is on my desk?",
                "A cup and a book.",
                now,
            )
            .unwrap();
        let third = store
            .append(&me, at("2026-10-13", 550), "", "Good morning.", now)
            .unwrap();
        assert_eq!((first, second, third), (1, 2, 3));
        assert_eq!(
            scratch.names("everyone"),
            ["journal-2026-10-12.jsonl", "journal-2026-10-13.jsonl"]
        );

        let day = store.recall(&me, &ask(None, Some("2026-10-13"))).unwrap();
        assert_eq!(ids(&day), [2, 3]);
        assert_eq!(
            day[0],
            Entry::Exchange(Exchange {
                id: 2,
                at_unix_ms: at("2026-10-13", 545),
                heard: "What is on my desk?".into(),
                said: "A cup and a book.".into(),
            })
        );
        let empty = store.recall(&me, &ask(None, Some("2026-10-11"))).unwrap();
        assert!(empty.is_empty());

        let words = store
            .recall(&me, &ask(Some("RAIN tomorrow?"), None))
            .unwrap();
        assert_eq!(ids(&words), [1]);
        let none = store.recall(&me, &ask(Some("umbrella"), None)).unwrap();
        assert!(none.is_empty());

        let newest = store.recall(&me, &ask(None, None)).unwrap();
        assert_eq!(ids(&newest), [1, 2, 3]);
        let two = Ask {
            limit: 2,
            ..ask(None, None)
        };
        assert_eq!(ids(&store.recall(&me, &two).unwrap()), [2, 3]);
        // A question that is only blanks is no question.
        assert_eq!(
            ids(&store.recall(&me, &ask(Some("  "), None)).unwrap()),
            [1, 2, 3]
        );

        // Words and a day together: that day's exchanges with the words.
        let both = store
            .recall(&me, &ask(Some("desk rain"), Some("2026-10-13")))
            .unwrap();
        assert_eq!(ids(&both), [2]);
    }

    #[test]
    fn more_of_the_words_rank_first_and_then_the_newer() {
        let scratch = Scratch::new("ranking");
        let now = at("2026-10-13", 600);
        let mut store = scratch.open(now);
        let me = everyone();
        let day = |day: &str| at(day, 60);
        store
            .append(&me, day("2026-10-10"), "Order coffee beans", "Done.", now)
            .unwrap();
        store
            .append(&me, day("2026-10-11"), "I like tea", "Noted.", now)
            .unwrap();
        store
            .remember(&me, day("2026-10-11"), "takes coffee black, no sugar")
            .unwrap();
        store
            .append(
                &me,
                day("2026-10-12"),
                "Coffee again?",
                "Black coffee, as ever.",
                now,
            )
            .unwrap();
        store
            .append(&me, day("2026-10-13"), "Is the army here", "No.", now)
            .unwrap();

        // Two of the words beat one; among equals the newer is first.
        let one = Ask {
            limit: 1,
            ..ask(Some("black coffee"), None)
        };
        assert_eq!(ids(&store.recall(&me, &one).unwrap()), [4]);
        let two = Ask {
            limit: 2,
            ..ask(Some("black coffee"), None)
        };
        let found = store.recall(&me, &two).unwrap();
        assert_eq!(ids(&found), [3, 4]);
        assert!(matches!(found[0], Entry::Kept(_)));
        assert!(matches!(found[1], Entry::Exchange(_)));
        // All that match, oldest first.
        let all = store.recall(&me, &ask(Some("black coffee"), None)).unwrap();
        assert_eq!(ids(&all), [1, 3, 4]);
        // A word said twice counts once: one word of two is still one.
        let twice = Ask {
            limit: 1,
            ..ask(Some("coffee, coffee and tea"), None)
        };
        assert_eq!(ids(&store.recall(&me, &twice).unwrap()), [4]);

        // Whole words only, and a word of one letter is not looked for.
        assert!(
            store
                .recall(&me, &ask(Some("my"), None))
                .unwrap()
                .is_empty()
        );
        assert!(
            store
                .recall(&me, &ask(Some("a i ?"), None))
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            ids(&store.recall(&me, &ask(Some("ARMY"), None)).unwrap()),
            [5]
        );
        // Only so many words are looked for.
        let many: Vec<String> = (0..QUERY_WORDS).map(|n| format!("word{n}")).collect();
        let long = format!("{} coffee", many.join(" "));
        assert!(
            store
                .recall(&me, &ask(Some(&long), None))
                .unwrap()
                .is_empty()
        );
        assert_eq!(words_of(&long).len(), QUERY_WORDS);
    }

    #[test]
    fn text_is_cut_at_a_character_and_its_blanks_made_one() {
        assert_eq!(
            tidy("  hello \t\r\n  there\u{0}friend\u{a0} ", 100),
            "hello there friend"
        );
        assert_eq!(tidy("\n\t  ", 100), "");
        // Thirteen characters in nineteen bytes: five of two bytes, one of
        // three.
        let coffee = "cà phê sữa đá";
        assert_eq!((coffee.chars().count(), coffee.len()), (13, 19));
        assert_eq!(tidy(coffee, 19), coffee);
        assert_eq!(tidy(coffee, 18), "cà phê sữa đ");
        assert_eq!(tidy(coffee, 17), "cà phê sữa đ");
        assert_eq!(tidy(coffee, 16), "cà phê sữa");
        assert_eq!(tidy(coffee, 12), "cà phê s");
        assert_eq!(tidy(coffee, 9), "cà phê");
        assert_eq!(tidy(coffee, 2), "c");
        assert_eq!(tidy(coffee, 0), "");
        for most in 0..=coffee.len() {
            let cut = tidy(coffee, most);
            assert!(cut.len() <= most && coffee.starts_with(&cut));
        }

        let scratch = Scratch::new("limits");
        let now = at("2026-10-13", 600);
        let mut store = scratch.open(now);
        let me = everyone();
        let long = "cà phê sữa đá ".repeat(100);
        store.append(&me, now, &long, &long, now).unwrap();
        store.remember(&me, now, &long).unwrap();
        let Entry::Exchange(exchange) = &store.recall(&me, &ask(None, None)).unwrap()[0] else {
            panic!("not an exchange");
        };
        assert!(exchange.heard.len() <= HEARD_BYTES && exchange.heard.len() > HEARD_BYTES - 4);
        assert!(exchange.said.len() <= SAID_BYTES && exchange.said.len() > SAID_BYTES - 4);
        assert!(long.starts_with(&exchange.heard) && long.starts_with(&exchange.said));
        let kept = store.kept(&me, 4096).unwrap();
        assert!(kept[0].text.len() <= TEXT_BYTES && kept[0].text.len() > TEXT_BYTES - 4);
        assert!(long.starts_with(&kept[0].text));
        // What is on the disk is the cut text, a line each.
        let journal =
            fs::read_to_string(scratch.0.join("everyone/journal-2026-10-13.jsonl")).unwrap();
        assert_eq!(journal.lines().count(), 1);
        assert!(journal.len() < HEARD_BYTES + SAID_BYTES + 80);

        // Nothing but blanks is nothing to remember, and takes no id.
        assert!(matches!(
            store.append(&me, now, " \n", "\t", now),
            Err(Refused::Empty)
        ));
        assert!(matches!(
            store.remember(&me, now, "\u{1}"),
            Err(Refused::Empty)
        ));
        // A moment no journal can be named for.
        assert!(matches!(
            store.append(&me, u64::MAX, "when", "never", now),
            Err(Refused::Time)
        ));
        assert!(matches!(
            store.remember(&me, u64::MAX, "never"),
            Err(Refused::Time)
        ));
        assert_eq!(
            store
                .append(&me, now, "one side is enough", "", now)
                .unwrap(),
            3
        );
    }

    #[test]
    fn the_local_day_turns_at_midnight_where_the_lamp_stands() {
        let scratch = Scratch::new("offset");
        // Seven hours east: local midnight is 17:00 UTC.
        let before = at("2026-10-13", 17 * 60) - 1;
        let midnight = at("2026-10-13", 17 * 60);
        let mut store = scratch.open_with(30, 420, midnight);
        let me = everyone();
        store
            .append(&me, before, "late", "It is.", midnight)
            .unwrap();
        store
            .append(&me, midnight, "early", "It is.", midnight)
            .unwrap();
        assert_eq!(
            scratch.names("everyone"),
            ["journal-2026-10-13.jsonl", "journal-2026-10-14.jsonl"]
        );
        assert_eq!(
            ids(&store.recall(&me, &ask(None, Some("2026-10-13"))).unwrap()),
            [1]
        );
        assert_eq!(
            ids(&store.recall(&me, &ask(None, Some("2026-10-14"))).unwrap()),
            [2]
        );
        let status = &store.status(None).unwrap()[0];
        assert_eq!(status.oldest_day.as_deref(), Some("2026-10-13"));
        assert_eq!(status.newest_day.as_deref(), Some("2026-10-14"));
        assert_eq!(Day::of(before, 420), Day::parse("2026-10-13"));
        assert_eq!(Day::of(before, 0), Day::parse("2026-10-13"));
        assert_eq!(Day::of(midnight, 420), Day::parse("2026-10-14"));
        // West of UTC the day turns later.
        assert_eq!(
            Day::of(at("2026-10-13", 60), -300),
            Day::parse("2026-10-12")
        );
    }

    #[test]
    fn with_a_zone_the_day_turns_at_local_midnight_in_winter_and_in_summer() {
        // #820: New York keeps summer time. Midnight is 05:00 UTC in January
        // and 04:00 UTC in July; a fixed 300 minutes west is an hour off in July.
        let scratch = Scratch::new("zone");
        let winter = at("2026-01-15", 5 * 60);
        let summer = at("2026-07-15", 4 * 60);
        let mut store = Store::open(
            Options {
                dir: scratch.0.clone(),
                keep_days: 365,
                local: LocalTime::zone("America/New_York").unwrap(),
            },
            winter,
        )
        .unwrap();
        let me = everyone();
        for (moment, heard) in [
            (winter - 1, "late"),
            (winter, "early"),
            (summer - 1, "late"),
            (summer, "early"),
        ] {
            store.append(&me, moment, heard, "It is.", moment).unwrap();
        }
        assert_eq!(
            scratch.names("everyone"),
            [
                "journal-2026-01-14.jsonl",
                "journal-2026-01-15.jsonl",
                "journal-2026-07-14.jsonl",
                "journal-2026-07-15.jsonl"
            ]
        );
        assert_eq!(Day::of(summer, -300), Day::parse("2026-07-14"));
    }

    #[test]
    fn ids_go_on_rising_across_both_files_after_the_store_is_opened_again() {
        let scratch = Scratch::new("reopen");
        let now = at("2026-10-13", 600);
        let me = everyone();
        let mut store = scratch.open(now);
        assert_eq!(store.append(&me, now, "one", "1", now).unwrap(), 1);
        assert_eq!(store.remember(&me, now, "two").unwrap(), 2);
        assert_eq!(
            store
                .append(&me, at("2026-10-01", 0), "an old day", "3", now)
                .unwrap(),
            3
        );
        drop(store);
        let mut store = scratch.open(now);
        assert_eq!(store.remember(&me, now, "four").unwrap(), 4);
        drop(store);
        // The highest id is in the kept file now, and the journal's newest
        // day does not have the journal's highest.
        let mut store = scratch.open(now);
        assert_eq!(store.append(&me, now, "five", "5", now).unwrap(), 5);
        // Someone new starts at one.
        let other = Person::new("p-2").unwrap();
        assert_eq!(store.append(&other, now, "one", "1", now).unwrap(), 1);
        drop(store);
        let mut store = scratch.open(now);
        assert_eq!(store.remember(&other, now, "two").unwrap(), 2);
        assert_eq!(store.remember(&me, now, "six").unwrap(), 6);
        assert_eq!(store.found().people, 2);
    }

    #[test]
    fn a_cut_off_last_line_and_a_garbage_line_are_skipped_and_counted_and_the_rest_loads() {
        let scratch = Scratch::new("corrupt");
        let now = at("2026-10-13", 600);
        let me = everyone();
        let mut store = scratch.open(now);
        for n in 1..=3 {
            store
                .append(&me, now, &format!("question {n}"), "answer", now)
                .unwrap();
            store.remember(&me, now, &format!("fact {n}")).unwrap();
        }
        drop(store);
        let journal = scratch.0.join("everyone/journal-2026-10-13.jsonl");
        let kept = scratch.0.join("everyone/kept.jsonl");
        // Garbage in the middle of one file, bytes that are not text in the
        // other, and both cut off in their last line as by a power cut.
        let text = fs::read_to_string(&journal).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        let cut = &lines[2][..lines[2].len() - 9];
        fs::write(
            &journal,
            format!("{}\n}}{{ not json\n{}\n{cut}", lines[0], lines[1]),
        )
        .unwrap();
        let mut bytes = fs::read(&kept).unwrap();
        bytes.truncate(bytes.len() - 5);
        bytes.splice(0..0, [0xff, 0xfe, b'\n', b'\n', b'[', b']', b'\n']);
        fs::write(&kept, bytes).unwrap();

        let mut store = scratch.open(now);
        assert_eq!(store.found().skipped_lines, 5);
        let status = &store.status(None).unwrap()[0];
        assert_eq!(
            (status.exchanges, status.kept, status.skipped_lines),
            (2, 2, 5)
        );
        assert_eq!(ids(&store.recall(&me, &ask(None, None)).unwrap()), [1, 3]);
        assert_eq!(
            ids(&store
                .recall(&me, &ask(Some("question fact"), None))
                .unwrap()),
            [1, 2, 3, 4]
        );
        let kept_ids: Vec<u64> = store
            .kept(&me, 4096)
            .unwrap()
            .iter()
            .map(|k| k.id)
            .collect();
        assert_eq!(kept_ids, [2, 4]);

        // The next entries take ids above what could be read, and start
        // lines of their own after the cut-off ones.
        assert_eq!(
            store
                .append(&me, now, "after the cut", "still here", now)
                .unwrap(),
            5
        );
        assert_eq!(store.remember(&me, now, "kept after the cut").unwrap(), 6);
        assert_eq!(
            ids(&store.recall(&me, &ask(None, None)).unwrap()),
            [1, 3, 5]
        );
        let status = &store.status(None).unwrap()[0];
        assert_eq!(
            (status.exchanges, status.kept, status.skipped_lines),
            (3, 3, 5)
        );
        // Forgetting rewrites a file and leaves what it could not read.
        assert_eq!(store.forget_last(&me, None).unwrap().unwrap().id, 6);
        assert_eq!(store.forget_last(&me, None).unwrap().unwrap().id, 5);
        let status = &store.status(None).unwrap()[0];
        assert_eq!(
            (status.exchanges, status.kept, status.skipped_lines),
            (2, 2, 5)
        );
        assert!(
            fs::read_to_string(&journal)
                .unwrap()
                .contains("}{ not json\n")
        );
    }

    #[test]
    fn forgetting_the_last_of_one_kind_leaves_the_other_kind_alone() {
        let scratch = Scratch::new("forget-kind");
        let now = at("2026-10-13", 600);
        let me = everyone();
        let mut store = scratch.open(now);
        store.remember(&me, now, "takes coffee black").unwrap();
        store
            .append(&me, now + 1, "and the weather?", "Dry.", now)
            .unwrap();
        store
            .append(&me, now + 2, "thanks", "Any time.", now)
            .unwrap();
        assert_eq!(
            store
                .forget_last(&me, Some(Kind::Kept))
                .unwrap()
                .unwrap()
                .id,
            1
        );
        assert_eq!(store.forget_last(&me, Some(Kind::Kept)).unwrap(), None);
        assert!(store.kept(&me, 4096).unwrap().is_empty());
        // What was said since is all still there.
        assert_eq!(ids(&store.recall(&me, &ask(None, None)).unwrap()), [2, 3]);
        store.remember(&me, now + 3, "sits by the window").unwrap();
        assert_eq!(
            store
                .forget_last(&me, Some(Kind::Exchange))
                .unwrap()
                .unwrap()
                .id,
            3
        );
        assert_eq!(store.kept(&me, 4096).unwrap().len(), 1);
        assert_eq!(ids(&store.recall(&me, &ask(None, None)).unwrap()), [2]);
    }

    #[test]
    fn forgetting_the_last_thing_takes_the_highest_id_of_either_kind() {
        let scratch = Scratch::new("forget-last");
        let now = at("2026-10-13", 600);
        let me = everyone();
        let mut store = scratch.open(now);
        assert_eq!(store.forget_last(&me, None).unwrap(), None);
        store.append(&me, now, "first", "1", now).unwrap();
        store.remember(&me, now + 1, "second").unwrap();
        store.append(&me, now + 2, "third", "3", now).unwrap();
        // Filed under an older day, and still the last thing added.
        let old = at("2026-10-02", 30);
        store.append(&me, old, "fourth", "4", now).unwrap();
        store.remember(&me, now + 4, "fifth").unwrap();

        let removed = |kind, id, at_unix_ms| {
            Some(Removed {
                kind,
                id,
                at_unix_ms,
            })
        };
        assert_eq!(
            store.forget_last(&me, None).unwrap(),
            removed(Kind::Kept, 5, now + 4)
        );
        assert_eq!(
            store.forget_last(&me, None).unwrap(),
            removed(Kind::Exchange, 4, old)
        );
        // The older day's only line went, and its file with it.
        assert_eq!(
            scratch.names("everyone"),
            ["journal-2026-10-13.jsonl", "kept.jsonl"]
        );
        assert_eq!(ids(&store.recall(&me, &ask(None, None)).unwrap()), [1, 3]);
        assert_eq!(
            store.forget_last(&me, None).unwrap(),
            removed(Kind::Exchange, 3, now + 2)
        );
        assert_eq!(ids(&store.recall(&me, &ask(None, None)).unwrap()), [1]);
        // An id that was forgotten is not given again by this process.
        assert_eq!(store.append(&me, now, "sixth", "6", now).unwrap(), 6);
        assert_eq!(
            store.forget_last(&me, None).unwrap(),
            removed(Kind::Exchange, 6, now)
        );
        assert_eq!(
            store.forget_last(&me, None).unwrap(),
            removed(Kind::Kept, 2, now + 1)
        );
        assert_eq!(scratch.names("everyone"), ["journal-2026-10-13.jsonl"]);
        assert_eq!(
            store.forget_last(&me, None).unwrap(),
            removed(Kind::Exchange, 1, now)
        );
        assert_eq!(store.forget_last(&me, None).unwrap(), None);
        // Nothing left: no directory, and nobody listed.
        assert!(!scratch.0.join("everyone").exists());
        assert!(store.status(None).unwrap().is_empty());
    }

    #[test]
    fn forgetting_everything_for_one_person_leaves_another_untouched() {
        let scratch = Scratch::new("forget-all");
        let now = at("2026-10-13", 600);
        let (me, other) = (everyone(), Person::new("mai").unwrap());
        let mut store = scratch.open(now);
        for person in [&me, &other] {
            store.append(person, now, "hello", "hello", now).unwrap();
            store
                .append(person, at("2026-10-12", 0), "yesterday", "yes", now)
                .unwrap();
            store.remember(person, now, "likes tea").unwrap();
        }
        // A line that cannot be read is forgotten with the rest.
        let kept = scratch.0.join("everyone/kept.jsonl");
        fs::write(
            &kept,
            [fs::read(&kept).unwrap(), b"a secret cut sho".to_vec()].concat(),
        )
        .unwrap();
        let before = store.status(Some(&other)).unwrap();

        assert_eq!(store.forget_all(Some(&me)).unwrap(), 3);
        assert!(!scratch.0.join("everyone").exists());
        assert!(store.recall(&me, &ask(None, None)).unwrap().is_empty());
        assert!(store.kept(&me, 4096).unwrap().is_empty());
        assert_eq!(store.status(Some(&other)).unwrap(), before);
        assert_eq!(
            ids(&store.recall(&other, &ask(None, None)).unwrap()),
            [2, 1]
        );
        assert_eq!(store.kept(&other, 4096).unwrap().len(), 1);
        assert_eq!(store.forget_all(Some(&me)).unwrap(), 0);

        // With nobody named, everyone's goes.
        store.append(&me, now, "again", "again", now).unwrap();
        assert_eq!(store.forget_all(None).unwrap(), 4);
        assert!(store.status(None).unwrap().is_empty());
        assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 0);
    }

    #[test]
    fn one_persons_memory_is_never_given_for_another() {
        let scratch = Scratch::new("apart");
        let now = at("2026-10-13", 600);
        let (me, an, binh) = (
            everyone(),
            Person::new("an").unwrap(),
            Person::new("binh").unwrap(),
        );
        let mut store = scratch.open(now);
        store
            .append(
                &an,
                now,
                "my password is tulip",
                "I will not repeat it.",
                now,
            )
            .unwrap();
        store.remember(&an, now, "tulip is the password").unwrap();
        store
            .append(
                &binh,
                now,
                "what is the password",
                "I do not know one.",
                now,
            )
            .unwrap();

        for asked in [
            ask(Some("tulip password"), None),
            ask(None, None),
            ask(None, Some("2026-10-13")),
        ] {
            let binhs = store.recall(&binh, &asked).unwrap();
            assert_eq!(ids(&binhs), [1]);
            assert!(!format!("{binhs:?}").contains("tulip"));
            assert!(store.recall(&me, &asked).unwrap().is_empty());
        }
        assert!(store.kept(&binh, 4096).unwrap().is_empty());
        assert!(store.kept(&me, 4096).unwrap().is_empty());
        assert_eq!(
            ids(&store.recall(&an, &ask(Some("tulip"), None)).unwrap()),
            [1, 2]
        );
        // Forgetting is one person's too.
        assert_eq!(store.forget_last(&binh, None).unwrap().unwrap().id, 1);
        assert_eq!(store.forget_last(&binh, None).unwrap(), None);
        assert_eq!(store.forget_last(&me, None).unwrap(), None);
        assert_eq!(store.kept(&an, 4096).unwrap().len(), 1);
        let people: Vec<String> = store
            .status(None)
            .unwrap()
            .into_iter()
            .map(|s| s.person)
            .collect();
        assert_eq!(people, ["an"]);
    }

    #[test]
    fn only_a_plain_short_lower_case_id_is_a_person() {
        for id in ["everyone", "a", "p-2", "mai_01", "-", &"x".repeat(32)] {
            assert_eq!(Person::new(id).unwrap().as_str(), id);
        }
        for id in [
            "",
            "../x",
            "..",
            ".",
            "a/b",
            "Everyone",
            "mai lan",
            "mai\n",
            "x\0",
            "đá",
            "a.b",
            &"x".repeat(33),
        ] {
            assert_eq!(Person::new(id), None, "{id:?}");
        }
    }

    #[test]
    fn journal_days_too_old_to_keep_are_deleted_on_opening_and_on_a_new_day() {
        let scratch = Scratch::new("retention");
        let me = everyone();
        let noon = |day: &str| at(day, 720);
        let mut store = scratch.open_with(3, 0, noon("2026-10-10"));
        for day in [
            "2026-10-06",
            "2026-10-07",
            "2026-10-08",
            "2026-10-09",
            "2026-10-10",
        ] {
            store
                .append(&me, noon(day), day, "noted", noon("2026-10-10"))
                .unwrap();
        }
        store
            .remember(&me, noon("2026-10-01"), "kept for good")
            .unwrap();
        // Written while it was still the tenth: nothing is deleted between
        // days, not even a day that was too old when it was written.
        assert_eq!(scratch.names("everyone").len(), 6);

        // The same day later, or a clock that went back: nothing.
        assert_eq!(store.turn_to(noon("2026-10-10") + 1), 0);
        assert_eq!(store.turn_to(noon("2026-10-09")), 0);
        // An append on the eleventh: the sixth and seventh are more than
        // three days before it.
        store
            .append(
                &me,
                noon("2026-10-11"),
                "2026-10-11",
                "noted",
                noon("2026-10-11"),
            )
            .unwrap();
        assert_eq!(
            scratch.names("everyone"),
            [
                "journal-2026-10-08.jsonl",
                "journal-2026-10-09.jsonl",
                "journal-2026-10-10.jsonl",
                "journal-2026-10-11.jsonl",
                "kept.jsonl"
            ]
        );
        // An entry dated far ahead deletes nothing: the day is the clock's.
        store
            .append(
                &me,
                noon("2027-10-11"),
                "from the future",
                "odd",
                noon("2026-10-11"),
            )
            .unwrap();
        assert_eq!(scratch.names("everyone").len(), 6);
        drop(store);

        // Opened three days on: the eighth to the tenth have gone.
        let store = scratch.open_with(3, 0, noon("2026-10-14"));
        assert_eq!(store.found().expired_days, 3);
        assert_eq!(
            scratch.names("everyone"),
            [
                "journal-2026-10-11.jsonl",
                "journal-2027-10-11.jsonl",
                "kept.jsonl"
            ]
        );
        // What it was asked to keep has no age.
        assert_eq!(store.kept(&me, 4096).unwrap()[0].text, "kept for good");
        drop(store);

        // The day turns where the lamp stands: at 17:00 UTC seven hours east.
        let mut store = scratch.open_with(3, 420, at("2026-10-14", 16 * 60 + 59));
        assert_eq!(store.found().expired_days, 0);
        assert_eq!(store.turn_to(at("2026-10-14", 17 * 60)), 1);
        assert_eq!(
            scratch.names("everyone"),
            ["journal-2027-10-11.jsonl", "kept.jsonl"]
        );
    }

    #[test]
    fn the_kept_list_holds_two_hundred_and_drops_the_oldest() {
        let scratch = Scratch::new("cap");
        let now = at("2026-10-13", 600);
        let me = everyone();
        let mut store = scratch.open(now);
        for n in 1..=KEPT_ENTRIES as u64 {
            assert_eq!(
                store.remember(&me, now + n, &format!("fact {n}")).unwrap(),
                n
            );
        }
        let all = store.kept(&me, usize::MAX).unwrap();
        assert_eq!(all.len(), 200);
        assert_eq!((all[0].id, all[199].id), (1, 200));

        assert_eq!(store.remember(&me, now + 201, "fact 201").unwrap(), 201);
        let all = store.kept(&me, usize::MAX).unwrap();
        assert_eq!(all.len(), 200);
        assert_eq!((all[0].id, all[199].id), (2, 201));
        assert_eq!(all[199].text, "fact 201");
        assert_eq!(scratch.names("everyone"), ["kept.jsonl"]);
        assert_eq!(store.status(None).unwrap()[0].kept, 200);
        // And again after opening, where the count is read from the disk.
        drop(store);
        let mut store = scratch.open(now);
        assert_eq!(store.remember(&me, now + 202, "fact 202").unwrap(), 202);
        let all = store.kept(&me, usize::MAX).unwrap();
        assert_eq!((all.len(), all[0].id, all[199].id), (200, 3, 202));
        // The one dropped is gone from the file, not only from the answer.
        let file = fs::read_to_string(scratch.0.join("everyone/kept.jsonl")).unwrap();
        assert!(!file.contains("\"fact 2\"") && file.contains("\"fact 3\""));
        assert_eq!(file.lines().count(), 200);
    }

    #[test]
    fn kept_gives_the_newest_that_fit_and_recall_stops_at_the_count_and_the_bytes() {
        let scratch = Scratch::new("fit");
        let now = at("2026-10-13", 600);
        let me = everyone();
        let mut store = scratch.open(now);
        for n in 0..6u64 {
            // Ten bytes of text each.
            store
                .remember(&me, now + n, &format!("kept {n}...."))
                .unwrap();
            store
                .append(&me, now + n, &format!("ask {n}"), &format!("say {n}"), now)
                .unwrap();
        }
        let kept = |max_bytes| -> Vec<u64> {
            store
                .kept(&me, max_bytes)
                .unwrap()
                .iter()
                .map(|kept| kept.id)
                .collect()
        };
        assert_eq!(kept(usize::MAX), [1, 3, 5, 7, 9, 11]);
        assert_eq!(kept(30), [7, 9, 11]);
        assert_eq!(kept(29), [9, 11]);
        assert_eq!(kept(10), [11]);
        assert_eq!(kept(9), [0u64; 0]);

        let recall = |limit, max_bytes| {
            let asked = Ask {
                limit,
                max_bytes,
                ..ask(None, None)
            };
            ids(&store.recall(&me, &asked).unwrap())
        };
        assert_eq!(recall(50, 16_384), [2, 4, 6, 8, 10, 12]);
        assert_eq!(recall(2, 16_384), [10, 12]);
        assert_eq!(recall(0, 16_384), [0u64; 0]);
        // Ten bytes of text each here too.
        assert_eq!(recall(50, 30), [8, 10, 12]);
        assert_eq!(recall(50, 19), [12]);
        assert_eq!(recall(50, 9), [0u64; 0]);
        let words = Ask {
            limit: 3,
            max_bytes: 25,
            ..ask(Some("kept say"), None)
        };
        assert_eq!(ids(&store.recall(&me, &words).unwrap()), [11, 12]);
        let day = Ask {
            limit: 2,
            ..ask(None, Some("2026-10-13"))
        };
        assert_eq!(ids(&store.recall(&me, &day).unwrap()), [10, 12]);
    }

    #[test]
    fn directories_and_files_are_their_owners_alone() {
        let scratch = Scratch::new("modes");
        let now = at("2026-10-13", 600);
        let me = everyone();
        let nested = Scratch(scratch.0.join("made/for/it"));
        let mut store = nested.open(now);
        // One file written by adding lines, one put in place by a rename.
        store.append(&me, now, "hello", "hello", now).unwrap();
        store.remember(&me, now, "a fact").unwrap();
        store.remember(&me, now, "another, to forget").unwrap();
        store.forget_last(&me, None).unwrap();
        let mode = |path: &Path| fs::metadata(path).unwrap().mode() & 0o7777;
        assert_eq!(mode(&nested.0), 0o700);
        assert_eq!(mode(&nested.0.join("everyone")), 0o700);
        assert_eq!(mode(&nested.0.join("everyone/kept.jsonl")), 0o600);
        assert_eq!(
            mode(&nested.0.join("everyone/journal-2026-10-13.jsonl")),
            0o600
        );
        assert_eq!(
            nested.names("everyone"),
            ["journal-2026-10-13.jsonl", "kept.jsonl"]
        );
        drop(store);

        // A directory others can enter is refused, and so is a file.
        for open in [0o750, 0o705, 0o701, 0o770] {
            fs::set_permissions(&nested.0, fs::Permissions::from_mode(open)).unwrap();
            let refused = Store::open(
                Options {
                    dir: nested.0.clone(),
                    keep_days: 30,
                    local: LocalTime::Offset(0),
                },
                now,
            )
            .err()
            .unwrap();
            assert_eq!(refused.kind(), io::ErrorKind::PermissionDenied, "{open:o}");
            assert!(refused.to_string().contains(&format!("mode {open:03o}")));
        }
        fs::set_permissions(&nested.0, fs::Permissions::from_mode(0o700)).unwrap();
        let file = nested.0.join("everyone/kept.jsonl");
        let refused = Store::open(
            Options {
                dir: file,
                keep_days: 30,
                local: LocalTime::Offset(0),
            },
            now,
        );
        assert_eq!(refused.err().unwrap().kind(), io::ErrorKind::NotADirectory);
    }

    #[test]
    fn a_removal_leaves_no_copy_and_one_left_by_a_cut_off_removal_is_cleared_on_opening() {
        let scratch = Scratch::new("copies");
        let now = at("2026-10-13", 600);
        let me = everyone();
        let mut store = scratch.open(now);
        store.append(&me, now, "one", "1", now).unwrap();
        store.append(&me, now, "two", "2", now).unwrap();
        store.remember(&me, now, "three").unwrap();
        store.remember(&me, now, "four").unwrap();
        store.forget_last(&me, None).unwrap();
        assert_eq!(
            scratch.names("everyone"),
            ["journal-2026-10-13.jsonl", "kept.jsonl"]
        );
        drop(store);
        // As a power cut between writing the copy and renaming it leaves it.
        let dir = scratch.0.join("everyone");
        fs::write(
            dir.join("kept.jsonl.tmp"),
            "{\"id\":9,\"at_unix_ms\":1,\"text\":\"half\"}\n",
        )
        .unwrap();
        fs::write(dir.join("journal-2026-10-13.jsonl.tmp"), "{\"id\":9").unwrap();
        // Not this store's: left where they are.
        fs::write(scratch.0.join("notes.txt"), "an operator's file").unwrap();
        fs::create_dir(scratch.0.join("Not-A-Person")).unwrap();

        let mut store = scratch.open(now);
        assert_eq!(store.found().unfinished_files, 2);
        assert_eq!(store.found().people, 1);
        assert_eq!(
            scratch.names("everyone"),
            ["journal-2026-10-13.jsonl", "kept.jsonl"]
        );
        // Four was forgotten before the store was opened again, so the
        // count goes on from three.
        assert_eq!(store.remember(&me, now, "four again").unwrap(), 4);
        assert_eq!(store.forget_all(None).unwrap(), 4);
        assert!(scratch.0.join("notes.txt").exists());
        assert!(scratch.0.join("Not-A-Person").exists());
    }

    #[test]
    fn status_counts_each_person_and_a_directory_is_one_stores_at_a_time() {
        let scratch = Scratch::new("status");
        let now = at("2026-10-13", 600);
        let (me, other) = (everyone(), Person::new("an").unwrap());
        let mut store = scratch.open(now);
        assert!(store.status(None).unwrap().is_empty());
        assert!(store.status(Some(&me)).unwrap().is_empty());
        store
            .append(&me, at("2026-10-11", 0), "one", "1", now)
            .unwrap();
        store.append(&me, now, "two", "2", now).unwrap();
        store.remember(&me, now, "three").unwrap();
        store.remember(&other, now, "only kept").unwrap();

        let status = store.status(None).unwrap();
        assert_eq!(status.len(), 2);
        assert_eq!(status[0].person, "an");
        assert_eq!((status[0].exchanges, status[0].kept), (0, 1));
        assert_eq!(
            (&status[0].oldest_day, &status[0].newest_day),
            (&None, &None)
        );
        assert_eq!(status[1].person, "everyone");
        assert_eq!(
            (status[1].exchanges, status[1].kept, status[1].skipped_lines),
            (2, 1, 0)
        );
        assert_eq!(status[1].oldest_day.as_deref(), Some("2026-10-11"));
        assert_eq!(status[1].newest_day.as_deref(), Some("2026-10-13"));
        let on_disk: u64 = scratch
            .names("everyone")
            .iter()
            .map(|name| {
                fs::metadata(scratch.0.join("everyone").join(name))
                    .unwrap()
                    .len()
            })
            .sum();
        assert_eq!(status[1].bytes, on_disk);
        assert_eq!(store.status(Some(&other)).unwrap(), status[..1]);

        // A second store on the directory would count ids for itself.
        let second = Store::open(
            Options {
                dir: scratch.0.clone(),
                keep_days: 30,
                local: LocalTime::Offset(0),
            },
            now,
        );
        assert_eq!(second.err().unwrap().kind(), io::ErrorKind::ResourceBusy);
        drop(store);
        scratch.open(now);
    }
}
