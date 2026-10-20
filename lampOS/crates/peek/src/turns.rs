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
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

/// The most turns kept, newest first on the page.
pub const KEPT: usize = 20;
/// The most of a file read at once: a day's session can be large, and a
/// read is on every push. A file first read when it is longer is read from
/// this far before its end: only the newest turns are kept.
const MOST_READ: u64 = 4 << 20;

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
    /// The session being read.
    session: Option<PathBuf>,
    /// Each events file's bytes read so far.
    read: BTreeMap<PathBuf, u64>,
    /// A line not yet ended, by file.
    partial: BTreeMap<PathBuf, Vec<u8>>,
    turns: BTreeMap<u64, Turn>,
}

impl Turns {
    /// The turns kept, newest first.
    pub fn newest(&self) -> Vec<Turn> {
        self.turns.values().rev().cloned().collect()
    }

    /// Read on in the newest session under `sessions`: a newer session
    /// starts the turns again.
    pub fn read_on(&mut self, sessions: &Path) {
        let Some(newest) = newest_session(sessions) else {
            return;
        };
        if self.session.as_ref() != Some(&newest) {
            *self = Self {
                session: Some(newest.clone()),
                ..Self::default()
            };
        }
        let Ok(entries) = fs::read_dir(newest.join("metadata")) else {
            return;
        };
        let mut files: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .filter(|entry| {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                name.starts_with("events-")
                    && name.ends_with(".jsonl")
                    && entry.file_type().is_ok_and(|kind| kind.is_file())
            })
            .map(|entry| entry.path())
            .collect();
        files.sort();
        for file in files {
            self.read_file(&file);
        }
    }

    fn read_file(&mut self, file: &Path) {
        let from = self.read.get(file).copied().unwrap_or_default();
        let Ok(mut opened) = fs::File::open(file) else {
            return;
        };
        let Ok(length) = opened.metadata().map(|meta| meta.len()) else {
            return;
        };
        // A file shorter than was read is another file of the same name.
        let from = if length < from { 0 } else { from };
        // First read, and long: its last part, from the first line begun
        // in it.
        let (from, mid_line) = if from == 0 && length > MOST_READ {
            (length - MOST_READ, true)
        } else {
            (from, false)
        };
        if length == from || opened.seek(SeekFrom::Start(from)).is_err() {
            return;
        }
        let mut bytes = self.partial.remove(file).unwrap_or_default();
        let Ok(took) = opened.take(MOST_READ).read_to_end(&mut bytes) else {
            return;
        };
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
pub fn newest_session(sessions: &Path) -> Option<PathBuf> {
    fs::read_dir(sessions)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .max_by_key(|entry| entry.metadata().and_then(|meta| meta.modified()).ok())
        .map(|entry| entry.path())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

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
