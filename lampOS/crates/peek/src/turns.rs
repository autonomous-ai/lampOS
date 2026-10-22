//! The voice's turns, as V1's Flow showed them, timings only: each turn
//! from the end of speech to its final words, the chat's first token and
//! its first audio, then played, or not and why (#2141's second step, the
//! lead on #1711, 20 Oct). Read from the voice's own session records, the
//! newest session's `events-*.jsonl`, read on from where the last read
//! ended. Of a record only the kinds and fields named here are taken: never
//! a `transcript`, never a field that is not named, so no word is read.
use crate::watch::plain;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

/// The most turns kept, newest first on the page.
pub const KEPT: usize = 20;
/// The most of a file read at once: a day's session can be large, and a
/// read is on every push. A file first read when it is longer is read from
/// this far before its end: only the newest turns are kept.
const MOST_READ: u64 = 4 << 20;

/// Why the session records could not be read. Paths and OS error text
/// stay out of the snapshot sent to the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadFailure {
    Permission,
    Missing,
    Other,
}

impl From<io::Error> for ReadFailure {
    fn from(error: io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::PermissionDenied => Self::Permission,
            io::ErrorKind::NotFound => Self::Missing,
            _ => Self::Other,
        }
    }
}

/// Whether the last complete read succeeded, or kept rows may be stale.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReadStatus {
    #[default]
    Pending,
    Current,
    Unavailable(ReadFailure),
    Stale(ReadFailure),
}

/// The rows and their read status travel together through the stream.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub turns: Vec<Turn>,
    pub status: ReadStatus,
}

impl Snapshot {
    pub(crate) fn failed(mut self, why: ReadFailure) -> Self {
        self.status = if self.turns.is_empty() {
            ReadStatus::Unavailable(why)
        } else {
            ReadStatus::Stale(why)
        };
        self
    }
}

/// One turn, as far as its records have come.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Turn {
    pub turn: u64,
    /// Why it was taken: `local_speech` for someone's words, or the lamp's
    /// own (`skill_says`, `lamp_speaks_first`...).
    pub basis: Option<String>,
    /// From the end of speech: the final words, the chat's first event,
    /// the answer's first audio (the second route's `answer_stages`).
    pub words_ms: Option<u64>,
    pub chat_ms: Option<u64>,
    pub audio_ms: Option<u64>,
    /// The answer's first audio after the end-of-speech decision, on either
    /// route (`answer_first_audio`).
    pub after_decision_ms: Option<u64>,
    /// How it ended (`turn_finished`).
    pub outcome: Option<String>,
    /// What held its sound or a sound in it, newest last.
    pub held: Option<String>,
}

/// Which step of a turn took longest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Words,
    Chat,
    Audio,
}

impl Turn {
    /// Each step's own time: to the final words, from them to the chat's
    /// first event, and from that to the first audio. A step whose ends are
    /// not both known is left out.
    pub fn steps(&self) -> [(Step, Option<u64>); 3] {
        let between =
            |from: Option<u64>, to: Option<u64>| from.zip(to).map(|(a, b)| b.saturating_sub(a));
        [
            (Step::Words, self.words_ms),
            (Step::Chat, between(self.words_ms, self.chat_ms)),
            (Step::Audio, between(self.chat_ms, self.audio_ms)),
        ]
    }

    /// The step that took longest, when two or more are known.
    pub fn slowest(&self) -> Option<Step> {
        let known: Vec<(Step, u64)> = self
            .steps()
            .into_iter()
            .filter_map(|(step, ms)| ms.map(|ms| (step, ms)))
            .collect();
        (known.len() > 1)
            .then(|| {
                known
                    .iter()
                    .max_by_key(|(_, ms)| *ms)
                    .map(|(step, _)| *step)
            })
            .flatten()
    }

    /// The first audio from the end of speech, by the second route's own
    /// count or the coordinator's.
    pub fn first_audio_ms(&self) -> Option<u64> {
        self.audio_ms.or(self.after_decision_ms)
    }
}

/// What a sound was held or let go for, by the record's kind.
fn held_for(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "floor_held" => "the busy room held the floor",
        "answer_waits" => "a sound waited for the answer",
        "input_let_go_answer_waits" => "a sound was let go for the answer",
        "input_let_go_own_line" => "a sound was let go for the lamp's own line",
        "input_let_go_body_moving" => "a sound was the body moving",
        "asker_first_answer_held" => "held for the one who asked first",
        "provider_turn_unclear" => "what was heard made no sense",
        "provider_turn_failed" => "the answer failed",
        _ => return None,
    })
}

/// How a turn ended, as the page says it: `turn_finished`'s outcome, or
/// why it was cut.
fn outcome(said: &str) -> String {
    match said {
        "audio_written_unscored" => "played".into(),
        "audio_written_with_playback_gaps_unscored" => "played, with gaps".into(),
        "no_audio_answer" => "no audio".into(),
        "provider_failed" => "failed".into(),
        other => plain(other),
    }
}

/// The kinds read, for a quick look at a line before it is parsed.
const KINDS: [&str; 12] = [
    "\"input_admitted\"",
    "\"answer_stages\"",
    "\"answer_first_audio\"",
    "\"turn_finished\"",
    "\"floor_held\"",
    "\"answer_waits\"",
    "\"input_let_go_answer_waits\"",
    "\"input_let_go_own_line\"",
    "\"input_let_go_body_moving\"",
    "\"asker_first_answer_held\"",
    "\"provider_turn_unclear\"",
    "\"provider_turn_failed\"",
];

/// The turns read so far, and where each file's read ended.
#[derive(Debug, Default)]
pub struct Turns {
    status: ReadStatus,
    /// The session being read.
    session: Option<PathBuf>,
    /// Each events file's bytes read so far.
    read: BTreeMap<PathBuf, u64>,
    /// A line not yet ended, by file.
    partial: BTreeMap<PathBuf, Vec<u8>>,
    turns: BTreeMap<u64, Turn>,
}

impl Turns {
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            turns: self.newest(),
            status: self.status,
        }
    }

    /// The turns kept, newest first.
    pub fn newest(&self) -> Vec<Turn> {
        self.turns.values().rev().cloned().collect()
    }

    /// Read on in the newest session under `sessions`: a newer session
    /// starts the turns again.
    pub fn read_on(&mut self, sessions: &Path) {
        self.status = match self.try_read_on(sessions) {
            Ok(()) => ReadStatus::Current,
            Err(error) if self.turns.is_empty() => ReadStatus::Unavailable(error.into()),
            Err(error) => ReadStatus::Stale(error.into()),
        };
    }

    fn try_read_on(&mut self, sessions: &Path) -> io::Result<()> {
        let Some(newest) = newest_session(sessions)? else {
            *self = Self::default();
            return Ok(());
        };
        if self.session.as_ref() != Some(&newest) {
            // Keep the previous session's rows, marked stale, until a
            // complete first read of the new session is possible.
            let mut next = Self {
                session: Some(newest.clone()),
                ..Self::default()
            };
            next.read_session(&newest)?;
            *self = next;
            return Ok(());
        }
        self.read_session(&newest)
    }

    fn read_session(&mut self, session: &Path) -> io::Result<()> {
        let mut files = Vec::new();
        for entry in fs::read_dir(session.join("metadata"))? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("events-")
                && name.ends_with(".jsonl")
                && entry.file_type()?.is_file()
            {
                files.push(entry.path());
            }
        }
        files.sort();
        for file in files {
            self.read_file(&file)?;
        }
        Ok(())
    }

    fn read_file(&mut self, file: &Path) -> io::Result<()> {
        let previous = self.read.get(file).copied().unwrap_or_default();
        let mut opened = fs::File::open(file)?;
        let length = opened.metadata()?.len();
        // A file shorter than was read is another file of the same name.
        let from = if length < previous { 0 } else { previous };
        // First read, and long: its last part, from the first line begun
        // in it.
        let (from, mid_line) = if from == 0 && length > MOST_READ {
            (length - MOST_READ, true)
        } else {
            (from, false)
        };
        if length == from {
            return Ok(());
        }
        opened.seek(SeekFrom::Start(from))?;
        // Neither the cursor nor the unfinished line advances on a
        // failed read, so recovery retries all the unread bytes.
        let mut added = Vec::new();
        let took = opened.take(MOST_READ).read_to_end(&mut added)?;
        let mut bytes = self.partial.remove(file).unwrap_or_default();
        if from != previous || length < previous {
            bytes.clear();
        };
        bytes.append(&mut added);
        self.read.insert(file.to_owned(), from + took as u64);
        if mid_line {
            let begun = bytes
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(bytes.len(), |at| at + 1);
            bytes.drain(..begun);
        }
        let ended = bytes
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |at| at + 1);
        self.partial.insert(file.to_owned(), bytes.split_off(ended));
        for line in bytes.split(|byte| *byte == b'\n') {
            self.take_line(line);
        }
        Ok(())
    }

    /// One line of the records: taken only when it is one of [`KINDS`].
    pub fn take_line(&mut self, line: &[u8]) {
        let Ok(text) = std::str::from_utf8(line) else {
            return;
        };
        if !KINDS.iter().any(|kind| text.contains(kind)) {
            return;
        }
        let Ok(record) = serde_json::from_str::<Value>(text) else {
            return;
        };
        self.take(&record);
    }

    fn take(&mut self, record: &Value) {
        let Some(kind) = record["kind"].as_str() else {
            return;
        };
        let Some(number) = record["turn"].as_u64() else {
            return;
        };
        let ms = |key: &str| record[key].as_u64();
        let word = |key: &str| record[key].as_str().map(plain);
        // A record of a turn older than every kept one is not kept.
        if self.turns.len() >= KEPT
            && !self.turns.contains_key(&number)
            && self
                .turns
                .keys()
                .next()
                .is_some_and(|oldest| number < *oldest)
        {
            return;
        }
        let turn = self.turns.entry(number).or_insert_with(|| Turn {
            turn: number,
            ..Turn::default()
        });
        match kind {
            "input_admitted" => turn.basis = word("admission_basis"),
            "answer_stages" => {
                turn.words_ms = ms("words_ms");
                turn.chat_ms = ms("chat_first_ms");
                turn.audio_ms = ms("first_answer_audio_ms");
            }
            "answer_first_audio" => turn.after_decision_ms = ms("after_decision_ms"),
            "turn_finished" => turn.outcome = record["outcome"].as_str().map(outcome),
            kind => {
                if let Some(held) = held_for(kind) {
                    turn.held = Some(held.to_owned());
                }
            }
        }
        while self.turns.len() > KEPT {
            self.turns.pop_first();
        }
    }
}

/// The newest session's folder under `sessions`, by when it last changed.
pub fn newest_session(sessions: &Path) -> io::Result<Option<PathBuf>> {
    let mut newest = None;
    for entry in fs::read_dir(sessions)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let modified = entry.metadata()?.modified()?;
        if newest.as_ref().is_none_or(|(at, _)| modified >= *at) {
            newest = Some((modified, entry.path()));
        }
    }
    Ok(newest.map(|(_, path)| path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write, os::unix::fs::PermissionsExt, time::SystemTime};

    /// Restore even after an assertion fails. These tests must run as an
    /// ordinary user: the read refusal itself is asserted, never skipped.
    struct Unreadable(PathBuf, fs::Permissions);

    impl Unreadable {
        fn new(path: &Path) -> Self {
            let old = fs::metadata(path).unwrap().permissions();
            fs::set_permissions(path, fs::Permissions::from_mode(0o0)).unwrap();
            Self(path.to_owned(), old)
        }
    }

    impl Drop for Unreadable {
        fn drop(&mut self) {
            fs::set_permissions(&self.0, self.1.clone()).unwrap();
        }
    }

    fn folder(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("lamp-peek-turns-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn append(file: &Path, text: &str) {
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(file)
            .unwrap()
            .write_all(text.as_bytes())
            .unwrap();
    }

    #[test]
    fn denied_root_metadata_and_file_keep_the_read_status_and_recover() {
        for layer in ["root", "metadata", "file"] {
            let sessions = folder(&format!("denied-{layer}"));
            let metadata = sessions.join("s1/metadata");
            fs::create_dir_all(&metadata).unwrap();
            let file = metadata.join("events-0.jsonl");
            append(
                &file,
                "{\"kind\":\"answer_first_audio\",\"turn\":1,\"after_decision_ms\":117}\n",
            );
            let denied = match layer {
                "root" => &sessions,
                "metadata" => &metadata,
                _ => &file,
            };
            let mut turns = Turns::default();
            assert_eq!(turns.snapshot().status, ReadStatus::Pending);
            let hold = Unreadable::new(denied);
            let error = if layer == "file" {
                fs::File::open(denied).unwrap_err()
            } else {
                fs::read_dir(denied).unwrap_err()
            };
            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
            turns.read_on(&sessions);
            assert_eq!(
                turns.snapshot().status,
                ReadStatus::Unavailable(ReadFailure::Permission),
                "{layer}"
            );
            assert!(turns.newest().is_empty());
            drop(hold);
            turns.read_on(&sessions);
            assert_eq!(turns.snapshot().status, ReadStatus::Current);
            assert_eq!(turns.newest()[0].first_audio_ms(), Some(117));

            // Keep half a line across the refusal. Recovery must not lose
            // it or advance the cursor beyond bytes that were not read.
            append(&file, "{\"kind\":\"turn_finished\",\"turn\":1,");
            turns.read_on(&sessions);
            append(&file, "\"outcome\":\"audio_written_unscored\"}\n");
            let hold = Unreadable::new(denied);
            turns.read_on(&sessions);
            let snapshot = turns.snapshot();
            assert_eq!(snapshot.status, ReadStatus::Stale(ReadFailure::Permission));
            assert_eq!(snapshot.turns.len(), 1);
            assert_eq!(snapshot.turns[0].outcome, None);
            assert_eq!(snapshot.turns[0].first_audio_ms(), Some(117));
            drop(hold);
            turns.read_on(&sessions);
            let snapshot = turns.snapshot();
            assert_eq!(snapshot.status, ReadStatus::Current);
            assert_eq!(snapshot.turns.len(), 1);
            assert_eq!(snapshot.turns[0].outcome.as_deref(), Some("played"));
            fs::remove_dir_all(sessions).unwrap();
        }
    }

    #[test]
    fn a_denied_new_session_keeps_the_old_rows_stale_until_it_can_be_read() {
        let sessions = folder("denied-new");
        let first = sessions.join("s1/metadata");
        fs::create_dir_all(&first).unwrap();
        append(
            &first.join("events-0.jsonl"),
            "{\"kind\":\"answer_first_audio\",\"turn\":19,\"after_decision_ms\":117}\n",
        );
        let mut turns = Turns::default();
        turns.read_on(&sessions);
        assert_eq!(turns.newest()[0].turn, 19);
        let second = sessions.join("s2/metadata");
        fs::create_dir_all(&second).unwrap();
        append(
            &second.join("events-0.jsonl"),
            "{\"kind\":\"answer_first_audio\",\"turn\":1,\"after_decision_ms\":197}\n",
        );
        // An explicit ordering avoids a sleep and filesystem clock ties.
        for (session, seconds) in [("s1", 1), ("s2", 2)] {
            fs::File::open(sessions.join(session))
                .unwrap()
                .set_times(
                    fs::FileTimes::new().set_modified(
                        SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(seconds),
                    ),
                )
                .unwrap();
        }
        let hold = Unreadable::new(&second);
        turns.read_on(&sessions);
        let snapshot = turns.snapshot();
        assert_eq!(snapshot.status, ReadStatus::Stale(ReadFailure::Permission));
        assert_eq!(snapshot.turns.len(), 1);
        assert_eq!(snapshot.turns[0].turn, 19);
        drop(hold);
        turns.read_on(&sessions);
        let snapshot = turns.snapshot();
        assert_eq!(snapshot.status, ReadStatus::Current);
        assert_eq!(snapshot.turns.len(), 1);
        assert_eq!(snapshot.turns[0].turn, 1);
        assert_eq!(snapshot.turns[0].first_audio_ms(), Some(197));
        fs::remove_dir_all(sessions).unwrap();
    }

    #[test]
    fn missing_and_failed_reads_are_distinct_from_an_empty_readable_session() {
        let root = folder("missing");
        let sessions = root.join("sessions");
        let mut turns = Turns::default();
        turns.read_on(&sessions);
        assert_eq!(
            turns.snapshot().status,
            ReadStatus::Unavailable(ReadFailure::Missing)
        );
        fs::create_dir(&sessions).unwrap();
        turns.read_on(&sessions);
        assert_eq!(turns.snapshot().status, ReadStatus::Current);
        assert!(turns.newest().is_empty());
        fs::create_dir(sessions.join("s1")).unwrap();
        turns.read_on(&sessions);
        assert_eq!(
            turns.snapshot().status,
            ReadStatus::Unavailable(ReadFailure::Missing)
        );
        fs::create_dir(sessions.join("s1/metadata")).unwrap();
        turns.read_on(&sessions);
        assert_eq!(turns.snapshot().status, ReadStatus::Current);
        assert!(turns.newest().is_empty());
        let invalid = root.join("not-a-directory");
        fs::write(&invalid, []).unwrap();
        turns.read_on(&invalid);
        assert_eq!(
            turns.snapshot().status,
            ReadStatus::Unavailable(ReadFailure::Other)
        );
        fs::remove_dir_all(root).unwrap();
    }

    /// A turn's steps from its records, each step's own time and the
    /// slowest, its basis and how it ended; a word in a record is never
    /// read, nor a field that isn't named.
    #[test]
    fn a_turns_steps_and_end_are_read_from_its_records_and_nothing_else() {
        let mut turns = Turns::default();
        for line in [
            r#"{"kind":"input_admitted","turn":4,"admission_basis":"local_speech","at_us":1}"#,
            r#"{"kind":"transcript","turn":4,"text":"Ossifrage","at_us":2}"#,
            r#"{"kind":"answer_stages","turn":4,"words_ms":300,"cue_ms":null,"chat_first_ms":900,"first_answer_audio_ms":1400,"note":"Ossifrage"}"#,
            r#"{"kind":"answer_first_audio","turn":4,"after_decision_ms":1450}"#,
            r#"{"kind":"turn_finished","turn":4,"outcome":"audio_written_unscored"}"#,
            r#"{"kind":"input_admitted","turn":5,"admission_basis":"Ossifrage said","at_us":1}"#,
            r#"{"kind":"floor_held","turn":5,"at_us":3}"#,
            r#"{"kind":"turn_finished","turn":5,"outcome":"no_audio_answer"}"#,
        ] {
            turns.take_line(line.as_bytes());
        }
        let newest = turns.newest();
        assert_eq!(newest.len(), 2);
        let (five, four) = (&newest[0], &newest[1]);
        assert_eq!(four.basis.as_deref(), Some("local_speech"));
        assert_eq!(
            four.steps(),
            [
                (Step::Words, Some(300)),
                (Step::Chat, Some(600)),
                (Step::Audio, Some(500))
            ]
        );
        assert_eq!(four.slowest(), Some(Step::Chat));
        assert_eq!(four.first_audio_ms(), Some(1400));
        assert_eq!(four.outcome.as_deref(), Some("played"));
        // A basis that isn't a plain name is not shown.
        assert_eq!(five.basis.as_deref(), Some("?"));
        assert_eq!(five.outcome.as_deref(), Some("no audio"));
        assert_eq!(five.held.as_deref(), Some("the busy room held the floor"));
        assert_eq!(five.slowest(), None);
        assert!(!format!("{newest:?}").contains("Ossifrage"));
    }

    /// Read on from where the last read ended: only new lines, a line not
    /// yet ended waits for its end, and a newer session starts again. At
    /// most [`KEPT`] turns, the newest.
    #[test]
    fn the_records_are_read_on_from_where_the_last_read_ended() {
        let sessions = folder("on");
        let first = sessions.join("s1/metadata");
        fs::create_dir_all(&first).unwrap();
        let file = first.join("events-1.jsonl");
        append(
            &file,
            "{\"kind\":\"answer_first_audio\",\"turn\":1,\"after_decision_ms\":900}\n",
        );
        let mut turns = Turns::default();
        turns.read_on(&sessions);
        assert_eq!(turns.newest()[0].after_decision_ms, Some(900));
        // Half a line: not taken until it ends.
        append(&file, "{\"kind\":\"turn_finished\",\"turn\":1,");
        turns.read_on(&sessions);
        assert_eq!(turns.newest()[0].outcome, None);
        append(&file, "\"outcome\":\"provider_failed\"}\n");
        turns.read_on(&sessions);
        assert_eq!(turns.newest()[0].outcome.as_deref(), Some("failed"));
        // Read again with nothing new: the same.
        turns.read_on(&sessions);
        assert_eq!(turns.newest().len(), 1);
        // More turns than are kept: the newest stay.
        for turn in 2..=30 {
            append(
                &file,
                &format!(
                    "{{\"kind\":\"answer_first_audio\",\"turn\":{turn},\"after_decision_ms\":1}}\n"
                ),
            );
        }
        turns.read_on(&sessions);
        let newest = turns.newest();
        assert_eq!(newest.len(), KEPT);
        assert_eq!((newest[0].turn, newest[KEPT - 1].turn), (30, 11));
        // A long file first read: from near its end, never a line begun
        // before that.
        let long = sessions.join("s1/metadata/events-2.jsonl");
        let filler = format!(
            "{{\"kind\":\"capture_gap\",\"pad\":\"{}\"}}\n",
            "x".repeat(1000)
        );
        let mut text =
            String::from("{\"kind\":\"answer_first_audio\",\"turn\":40,\"after_decision_ms\":5}\n");
        while (text.len() as u64) < MOST_READ + 4096 {
            text.push_str(&filler);
        }
        text.push_str("{\"kind\":\"answer_first_audio\",\"turn\":41,\"after_decision_ms\":2}\n");
        append(&long, &text);
        turns.read_on(&sessions);
        let newest = turns.newest();
        assert_eq!(newest[0].after_decision_ms, Some(2), "the end is read");
        assert!(newest.iter().all(|turn| turn.turn != 40), "{newest:?}");
        // A newer session: its turns only.
        std::thread::sleep(std::time::Duration::from_millis(20));
        let second = sessions.join("s2/metadata");
        fs::create_dir_all(&second).unwrap();
        append(
            &second.join("events-1.jsonl"),
            "{\"kind\":\"answer_first_audio\",\"turn\":1,\"after_decision_ms\":700}\n",
        );
        turns.read_on(&sessions);
        let newest = turns.newest();
        assert_eq!(newest.len(), 1, "{newest:?}");
        assert_eq!(newest[0].after_decision_ms, Some(700));
        let _ = fs::remove_dir_all(&sessions);
    }
}
