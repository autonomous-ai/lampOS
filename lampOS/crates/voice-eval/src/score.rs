//! A table scoring the voice routes from the lamp's journal alone, one column
//! an arm of a played test (the first builder on #1722, 19 Oct 18:44:
//! tonight's two-voice run, Deepgram, Deepgram with Flux and the prepared
//! round, and Live). Each arm is a stretch of the journal by its time of day.
//! Only the voice's own lines are read, and of them only their counts,
//! reasons and times: no words a person said are ever printed.
//!
//! The journal is `journalctl -u lamp-voice` in any of its usual forms
//! (`short`, `short-iso`, `short-precise`): a line's time of day is the first
//! `HH:MM:SS` in it.
//!
//! The played test's own files add rows ([`Files`]): its scene files
//! (`scenario <name>  <ISO>Z  …`, one event a line) and its battery summaries
//! (`battery <round>-<arm>-<HHMM> <HH:MM:SS>`, a played line and the lamp's
//! answer a line), each counted into the arm whose stretch holds its start,
//! by their marks and columns alone (Fourth's shapes on #1722, 18:52). They
//! are placed by their day as well as their time (Jarvis on #1899): a scene
//! by its own date, a battery by when its file was written.
//!
//! Each row is printed for the arms whose route logs it, and "—" for the
//! rest: the second route logs every row, Live only some ([`second_only`]).
//! A request is known by its process and number, since numbers start again
//! when the service does.

use std::collections::BTreeMap;

/// One arm: its name and its stretch of the journal, by time of day in
/// seconds, `from` included and `to` not (`to` before `from` runs past
/// midnight).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Arm {
    pub name: String,
    pub from: u32,
    pub to: u32,
}

impl Arm {
    /// An arm from its name and two times of day, `HH:MM` or `HH:MM:SS`.
    pub fn parse(name: &str, from: &str, to: &str) -> Result<Self, String> {
        Ok(Self {
            name: name.to_owned(),
            from: clock(from).ok_or_else(|| format!("{from:?} is no HH:MM or HH:MM:SS"))?,
            to: clock(to).ok_or_else(|| format!("{to:?} is no HH:MM or HH:MM:SS"))?,
        })
    }

    fn holds(&self, at: u32) -> bool {
        if self.from <= self.to {
            (self.from..self.to).contains(&at)
        } else {
            at >= self.from || at < self.to
        }
    }

    /// Whether the arm holds `moment`, seconds since 1970 by the lamp's
    /// clock, in a run whose first arm begins at `first` on `day`: an arm
    /// that begins earlier in the day than the first is on the day after,
    /// and one whose `to` is before its `from` ends on the day after it
    /// begins.
    fn holds_moment(&self, moment: i64, day: i64, first: u32) -> bool {
        let day = day + i64::from(self.from < first);
        let begins = day * 86_400 + i64::from(self.from);
        let mut ends = day * 86_400 + i64::from(self.to);
        if self.to < self.from {
            ends += 86_400;
        }
        (begins..ends).contains(&moment)
    }
}

/// `YYYY-MM-DD` as days since 1970-01-01.
pub fn day(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    let digits = |range: std::ops::Range<usize>| bytes[range].iter().all(u8::is_ascii_digit);
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !(digits(0..4) && digits(5..7) && digits(8..10))
    {
        return None;
    }
    let year: i64 = text[..4].parse().ok()?;
    let month: i64 = text[5..7].parse().ok().filter(|m| (1..=12).contains(m))?;
    let date: i64 = text[8..10].parse().ok()?;
    let first = days_from_civil(year, month, 1);
    let next = if month == 12 {
        days_from_civil(year + 1, 1, 1)
    } else {
        days_from_civil(year, month + 1, 1)
    };
    (1..=next - first)
        .contains(&date)
        .then_some(first + date - 1)
}

/// Days since 1970-01-01 of a date in the Gregorian calendar (Howard
/// Hinnant's `days_from_civil`).
fn days_from_civil(year: i64, month: i64, date: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let of_era = year - era * 400;
    let of_year = (153 * ((month + 9) % 12) + 2) / 5 + date - 1;
    let of_cycle = of_era * 365 + of_era / 4 - of_era / 100 + of_year;
    era * 146_097 + of_cycle - 719_468
}

/// `HH:MM` or `HH:MM:SS` as seconds of the day.
fn clock(text: &str) -> Option<u32> {
    let parts: Vec<&str> = text.split(':').collect();
    let number = |part: &str, most: u32| {
        (part.len() == 2 && part.bytes().all(|b| b.is_ascii_digit()))
            .then(|| part.parse::<u32>().ok())
            .flatten()
            .filter(|value| *value <= most)
    };
    match parts.as_slice() {
        [h, m] => Some(number(h, 23)? * 3600 + number(m, 59)? * 60),
        [h, m, s] => Some(number(h, 23)? * 3600 + number(m, 59)? * 60 + number(s, 59)?),
        _ => None,
    }
}

/// The first `HH:MM:SS` in a journal line, as seconds of the day.
fn time_of_day(line: &str) -> Option<u32> {
    let bytes = line.as_bytes();
    (0..bytes.len().saturating_sub(7)).find_map(|at| {
        let window = &bytes[at..at + 8];
        let digits = [0, 1, 3, 4, 6, 7]
            .iter()
            .all(|i| window[*i].is_ascii_digit());
        (digits && window[2] == b':' && window[5] == b':')
            .then(|| clock(std::str::from_utf8(window).ok()?))
            .flatten()
    })
}

/// The number after `before` in `line`.
fn number_after(line: &str, before: &str) -> Option<u64> {
    let rest = &line[line.find(before)? + before.len()..];
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// What let a line out, by its reason in the journal.
fn let_out_by(reason: &str) -> &'static str {
    let has = |part: &str| reason.contains(part);
    if has("only as the conversation going on") || has("conversation with the lamp is going on") {
        "let out by a conversation going on"
    } else if has("shows it was called")
        || has("name was heard")
        || has("its name")
        || has("tapped its head")
        || has("follows the lamp's name")
    {
        "let out by its name or words"
    } else if has("turned to the lamp") {
        "let out by a face"
    } else if has("after the lamp's own news")
        || has("about its own work")
        || has("lamp's own work")
    {
        "let out by the lamp's news or work"
    } else if has("heard enough to be trusted") {
        "let out by a trusted session"
    } else if has("not known to be busy") {
        "let out in a room not known to be busy"
    } else {
        "let out for another reason"
    }
}

/// Why a line was held, by its reason in the journal.
fn held_for(reason: &str) -> &'static str {
    let has = |part: &str| reason.contains(part);
    if has("nobody spoke to the lamp") {
        "held: nobody spoke to the lamp"
    } else if has("busy room") {
        "held: a busy room, no call"
    } else if has("short sound") {
        "held: a short sound"
    } else if has("a word or two") {
        "held: a word or two"
    } else if has("someone it calls by name") || has("answering someone") {
        "held: said to someone else"
    } else {
        "held: another reason"
    }
}

/// The rows, in the order printed: counts first, then times.
const COUNTS: &[&str] = &[
    "lines judged",
    "let out",
    "let out by its name or words",
    "let out by a face",
    "let out by a conversation going on",
    "let out by the lamp's news or work",
    "let out by a trusted session",
    "let out in a room not known to be busy",
    "let out for another reason",
    "held back",
    "held: nobody spoke to the lamp",
    "held: a busy room, no call",
    "held: a short sound",
    "held: a word or two",
    "held: said to someone else",
    "held: another reason",
    "answered (first answer audio; Live: first sound, not held)",
    "declined by the model (not_for_me)",
    "unanswered in time",
    "chat failed",
    "voice tries failed to start a sentence (each asked once more)",
    "lines with no words",
    "sound carried from the line before",
    "words cut short, answered in the held line's place",
];
const TIMES: &[&str] = &[
    "line end to words done, ms",
    "chat's first event, ms",
    "words to first piece to the voice, ms",
    "words to first answer audio, ms",
    "line end to first answer audio, ms",
];

/// Whether only the second route logs `row`: Live has no chat rounds, no
/// sentences to a voice, no word counts and no carried sound, and its
/// times are of other moments.
fn second_only(row: &str) -> bool {
    TIMES.contains(&row)
        || [
            "chat failed",
            "voice tries failed to start a sentence (each asked once more)",
            "lines with no words",
            "sound carried from the line before",
            "words cut short, answered in the held line's place",
        ]
        .contains(&row)
}

/// A request: its process, from `lamp-voice[PID]`, and its number.
type Request = (u64, u64);

/// One arm's counts and times.
#[derive(Default)]
struct Tally {
    counts: BTreeMap<&'static str, u64>,
    times: BTreeMap<&'static str, Vec<u64>>,
    /// Rows from the played test's own files, named as they come.
    file_counts: BTreeMap<String, u64>,
    file_times: BTreeMap<String, Vec<u64>>,
    /// By request: when its words were done after the line's end, and when
    /// its first answer audio came after the words.
    words_done: BTreeMap<Request, u64>,
    first_audio: BTreeMap<Request, u64>,
    /// The rows already counted for a request, each once.
    seen: std::collections::BTreeSet<(&'static str, Request)>,
    /// Live's requests that made a first sound, and those held back.
    sounded: std::collections::BTreeSet<Request>,
    held: std::collections::BTreeSet<Request>,
    /// Whether the arm's stretch holds the second route's lines, and Live's.
    second: bool,
    live: bool,
}

impl Tally {
    fn count(&mut self, row: &'static str) {
        *self.counts.entry(row).or_default() += 1;
    }

    fn time(&mut self, row: &'static str, ms: u64) {
        self.times.entry(row).or_default().push(ms);
    }

    /// `row` counted once for `request`, or each time with none.
    fn once(&mut self, row: &'static str, request: Option<Request>) {
        if request.is_none_or(|request| self.seen.insert((row, request))) {
            self.count(row);
        }
    }

    fn read(&mut self, line: &str) {
        let process = number_after(line, "lamp-voice[").unwrap_or(0);
        let Some(at) = line
            .find("lamp-voice second: ")
            .or_else(|| line.find("lamp-voice provider: "))
        else {
            return;
        };
        let line = &line[at..];
        let second = line.starts_with("lamp-voice second: ");
        if second {
            self.second = true;
        } else {
            self.live = true;
        }
        let request = number_after(line, "request ").map(|number| (process, number));
        if let Some(reason) = line.split("its answer is played: ").nth(1) {
            self.count("lines judged");
            self.count("let out");
            self.count(let_out_by(reason));
        } else if let Some(reason) = line.split("its answer is held back: ").nth(1) {
            self.count("lines judged");
            self.count("held back");
            self.count(held_for(reason));
            self.held.extend(request);
        }
        // The second route's `chat round N called a, b`, and Live's decline
        // taken (`the model said it was not for it`); not Live's one said
        // after doing what was asked, which is not taken.
        let called = line.split(" called ").nth(1).is_some_and(|names| {
            names
                .split(", ")
                .any(|name| name.trim_end() == "not_for_me")
        });
        if called
            || line
                .trim_end()
                .ends_with(": the model said it was not for it")
        {
            self.once("declined by the model (not_for_me)", request);
        }
        // The second route's chat that never began, and Live's request
        // closed unanswered; not a wait sound's line, nor Live's sound,
        // reminder or reply said over, which need no answer.
        if (line.contains("chat round") && line.contains(" had not begun in "))
            || line.contains("; closed as unanswered")
        {
            self.once("unanswered in time", request);
        }
        if line.contains("chat round") && line.contains("failed after") {
            self.count("chat failed");
        }
        if line.contains("failed to start a sentence") {
            self.count("voice tries failed to start a sentence (each asked once more)");
        }
        if line.contains("goes on from request") && line.contains(" of sound go first") {
            self.count("sound carried from the line before");
        }
        if line.contains("are answered in its place") {
            self.count("words cut short, answered in the held line's place");
        }
        if let Some(words) =
            number_after(line, " heard ").filter(|_| line.contains(" words, the last "))
        {
            if words == 0 {
                self.count("lines with no words");
            }
            if let Some(ms) = number_after(line, " words, the last ") {
                self.time("line end to words done, ms", ms);
                if let Some(request) = request {
                    self.words_done.insert(request, ms);
                }
            }
        }
        if line.contains("chat round 1 first event in ")
            && let Some(ms) = number_after(line, "first event in ")
        {
            self.time("chat's first event, ms", ms);
        }
        if let Some(ms) = number_after(line, "first piece to the voice ") {
            self.time("words to first piece to the voice, ms", ms);
        }
        if let Some(ms) = number_after(line, "first answer audio ") {
            self.once(
                "answered (first answer audio; Live: first sound, not held)",
                request,
            );
            self.time("words to first answer audio, ms", ms);
            if let Some(request) = request {
                self.first_audio.insert(request, ms);
            }
        }
        if !second && line.contains(": first sound") {
            self.sounded.extend(request);
        }
    }

    /// A scene file's events: played and heard lines, the lamp's turns and
    /// its time to speak, how turns finished, yields and rejected inputs.
    fn scene(&mut self, text: &str) {
        let mut count = |row: String| *self.file_counts.entry(row).or_default() += 1;
        let mut spoke = Vec::new();
        for line in text.lines().skip(1) {
            let line = line.trim();
            let after_time = line
                .split_once("s ")
                .filter(|(time, _)| time.parse::<f64>().is_ok())
                .map_or(line, |(_, rest)| rest.trim_start());
            if after_time.starts_with("PLAY") {
                count("scenes: lines played".into());
            } else if after_time.starts_with("HEARD") {
                count("scenes: lines heard".into());
            } else if let Some(event) = after_time.strip_prefix('[') {
                if let Some(seconds) = event
                    .strip_prefix("lamp starts speaking ")
                    .and_then(|rest| rest.split(" s").next())
                    .and_then(|seconds| seconds.parse::<f64>().ok())
                {
                    count("scenes: the lamp's turns".into());
                    spoke.push((seconds * 1000.0).round() as u64);
                } else if let Some(reason) = event.strip_prefix("turn finished: ") {
                    let reason: String = reason
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                        .collect();
                    count(format!("scenes: turn finished, {reason}"));
                } else if event.starts_with("duplex_yield") {
                    count("scenes: yields".into());
                } else if event.starts_with("input_candidate_rejected") {
                    count("scenes: inputs rejected".into());
                }
            }
        }
        self.file_times
            .entry("scenes: end of speech to first sound, ms".into())
            .or_default()
            .extend(spoke);
    }

    /// A battery summary (the first builder on #1899, 21:01): its
    /// two-people rounds first (`== round R, N: …`, then a line `Speaker:
    /// words | answer` each), then its sections, each begun by `-- <suite>`
    /// (`-- day`, `-- follow-ups (#1465)`, `-- hand-overs`). A section's
    /// lines played and answered are its own `… answered: N of M` line where
    /// it has one, or else its `|` lines. A round's line names the lamp or
    /// is between people. An answer is read only as blank or not, and a
    /// round's words only for the lamp's name.
    fn battery(&mut self, text: &str) {
        let mut suite: Option<String> = None;
        // The section's `|` lines, played and answered, and its own tally.
        let mut lines = (0, 0);
        let mut told = None;
        for line in text.lines().skip(1) {
            if let Some(header) = line.strip_prefix("-- ") {
                if let Some(suite) = suite.take() {
                    self.battery_section(&suite, told.take().unwrap_or(lines));
                }
                suite = Some(
                    header
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                        .collect(),
                );
                (lines, told) = ((0, 0), None);
            } else if suite.is_some() {
                if let Some(tally) = answered_of(line) {
                    told = Some(tally);
                } else if let Some((_, answer)) = line.split_once('|') {
                    lines.0 += 1;
                    lines.1 += u64::from(!answer.trim().is_empty());
                }
            } else if !line.starts_with("==")
                && let Some((said, answer)) = line.split_once('|')
            {
                let answered = !answer.trim().is_empty();
                let (row, done, kept) = if names_the_lamp(said) {
                    (
                        "battery rounds: lines naming the lamp",
                        "battery rounds: lines naming the lamp, answered",
                        answered,
                    )
                } else {
                    (
                        "battery rounds: lines between people",
                        "battery rounds: lines between people, left alone",
                        !answered,
                    )
                };
                *self.file_counts.entry(row.to_owned()).or_default() += 1;
                *self.file_counts.entry(done.to_owned()).or_default() += u64::from(kept);
            }
        }
        if let Some(suite) = suite {
            self.battery_section(&suite, told.unwrap_or(lines));
        }
    }

    /// A battery section's lines played and answered.
    fn battery_section(&mut self, suite: &str, (played, answered): (u64, u64)) {
        *self
            .file_counts
            .entry(format!("battery {suite}: lines played"))
            .or_default() += played;
        *self
            .file_counts
            .entry(format!("battery {suite}: lines answered"))
            .or_default() += answered;
    }

    /// Line end to first answer audio, for each request with both, and
    /// Live's answered: a first sound not held back.
    fn join(&mut self) {
        let both: Vec<u64> = self
            .first_audio
            .iter()
            .filter_map(|(request, audio)| Some(self.words_done.get(request)? + audio))
            .collect();
        for ms in both {
            self.time("line end to first answer audio, ms", ms);
        }
        let answered: Vec<Request> = self.sounded.difference(&self.held).copied().collect();
        for request in answered {
            self.once(
                "answered (first answer audio; Live: first sound, not held)",
                Some(request),
            );
        }
    }

    /// `row`'s cell: "—" when no route in the arm logs it.
    fn cell(&self, row: &str, value: String) -> String {
        if self.second || (self.live && !second_only(row)) {
            value
        } else {
            "—".to_owned()
        }
    }
}

/// A battery section's own tally, `… answered: N of M`, as (played M,
/// answered N).
fn answered_of(line: &str) -> Option<(u64, u64)> {
    let (_, rest) = line.split_once("answered: ")?;
    let (answered, played) = rest.split_once(" of ")?;
    let number = |text: &str| -> Option<u64> {
        let digits: String = text
            .trim_start()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        digits.parse().ok()
    };
    Some((number(played)?, number(answered)?))
}

/// Whether a played line calls the lamp by name: a word `lamp`, any case.
fn names_the_lamp(said: &str) -> bool {
    said.split(|c: char| !c.is_alphanumeric())
        .any(|word| word.eq_ignore_ascii_case("lamp"))
}

/// The median and the 90th percentile of `values`, and how many. Each is
/// the value at that share of the sorted list, rounded to the nearest
/// place: of an even count, the median is the upper of the middle two.
fn spread(values: &[u64]) -> String {
    if values.is_empty() {
        return "—".to_owned();
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let at = |share: f64| sorted[((sorted.len() - 1) as f64 * share).round() as usize];
    format!("{} ({}), {}", at(0.5), at(0.9), sorted.len())
}

/// The table for `arms` from `journal`: a row a measure, a column an arm, in
/// Markdown. Times are the median, the 90th percentile in brackets, and how
/// many.
pub fn score(journal: &str, arms: &[Arm]) -> String {
    score_with(journal, arms, &Files::default())
}

/// The played test's own files, by their text: scene files, with the hours
/// their UTC start is behind the lamp's clock (`offset`, seconds east), and
/// battery summaries, whose start is the lamp's clock already, each with
/// when its file was written (seconds since 1970), which gives its day.
/// Each is placed on the run's `day` ([`day`], the day its first arm
/// begins), and none without one.
#[derive(Default)]
pub struct Files {
    pub scenes: Vec<String>,
    pub batteries: Vec<(String, Option<i64>)>,
    pub offset: i32,
    pub day: Option<i64>,
}

/// A scene file's start: its `YYYY-MM-DDTHH:MM:SS…Z` on the first line, as
/// seconds since 1970 by the lamp's clock with `offset`.
fn scene_start(text: &str, offset: i32) -> Option<i64> {
    let first = text.lines().next()?;
    first.strip_prefix("scenario ")?;
    let stamp = first
        .split_whitespace()
        .find(|word| word.contains('T') && word.ends_with('Z'))?;
    let date = day(stamp.get(..10)?)?;
    let utc = time_of_day(stamp)?;
    Some(date * 86_400 + i64::from(utc) + i64::from(offset))
}

/// A battery summary's start: the `HH:MM:SS` on its first line, on the day
/// its file was `written`, or the day before when that time is later than
/// the writing's (a battery begun before midnight), as seconds since 1970
/// by the lamp's clock with `offset`.
fn battery_start(text: &str, written: Option<i64>, offset: i32) -> Option<i64> {
    let first = text.lines().next()?;
    first.strip_prefix("battery ")?;
    let at = i64::from(time_of_day(first)?);
    let written = written? + i64::from(offset);
    let mut date = written.div_euclid(86_400);
    if at > written.rem_euclid(86_400) {
        date -= 1;
    }
    Some(date * 86_400 + at)
}

/// The arm holding a file's `start` on the run's `day`.
fn arm_for(arms: &[Arm], day: Option<i64>, start: Option<i64>) -> Option<usize> {
    let (day, start, first) = (day?, start?, arms.first()?.from);
    arms.iter()
        .position(|arm| arm.holds_moment(start, day, first))
}

/// [`score`] with the played test's own files' rows after the journal's.
pub fn score_with(journal: &str, arms: &[Arm], files: &Files) -> String {
    let mut tallies: Vec<Tally> = arms.iter().map(|_| Tally::default()).collect();
    let mut scenes_placed = 0;
    for text in &files.scenes {
        if let Some(index) = arm_for(arms, files.day, scene_start(text, files.offset)) {
            tallies[index].scene(text);
            scenes_placed += 1;
        }
    }
    let mut batteries_placed = 0;
    for (text, written) in &files.batteries {
        if let Some(index) = arm_for(arms, files.day, battery_start(text, *written, files.offset)) {
            tallies[index].battery(text);
            batteries_placed += 1;
        }
    }
    for line in journal.lines() {
        let Some(at) = time_of_day(line) else {
            continue;
        };
        for (arm, tally) in arms.iter().zip(tallies.iter_mut()) {
            if arm.holds(at) {
                tally.read(line);
            }
        }
    }
    for tally in &mut tallies {
        tally.join();
    }
    let mut out = String::from("| |");
    for arm in arms {
        out.push_str(&format!(" {} |", arm.name));
    }
    out.push_str("\n|---|");
    out.push_str(&"---|".repeat(arms.len()));
    out.push('\n');
    for row in COUNTS {
        out.push_str(&format!("| {row} |"));
        for tally in &tallies {
            let count = tally.counts.get(row).copied().unwrap_or(0);
            out.push_str(&format!(" {} |", tally.cell(row, count.to_string())));
        }
        out.push('\n');
    }
    for row in TIMES {
        out.push_str(&format!("| {row} |"));
        for tally in &tallies {
            let times = spread(tally.times.get(row).map_or(&[][..], Vec::as_slice));
            out.push_str(&format!(" {} |", tally.cell(row, times)));
        }
        out.push('\n');
    }
    let rows: std::collections::BTreeSet<&String> = tallies
        .iter()
        .flat_map(|tally| tally.file_counts.keys())
        .collect();
    for row in rows {
        out.push_str(&format!("| {row} |"));
        for tally in &tallies {
            out.push_str(&format!(
                " {} |",
                tally.file_counts.get(row).copied().unwrap_or(0)
            ));
        }
        out.push('\n');
    }
    let rows: std::collections::BTreeSet<&String> = tallies
        .iter()
        .flat_map(|tally| tally.file_times.keys())
        .collect();
    for row in rows {
        out.push_str(&format!("| {row} |"));
        for tally in &tallies {
            out.push_str(&format!(
                " {} |",
                spread(tally.file_times.get(row).map_or(&[][..], Vec::as_slice))
            ));
        }
        out.push('\n');
    }
    let mut notes = Vec::new();
    for (arm, tally) in arms.iter().zip(&tallies) {
        if !tally.second && !tally.live {
            notes.push(format!(
                "{}: no voice lines in its stretch. Check its times against the lamp's clock and its zone (`timedatectl`).",
                arm.name
            ));
        }
    }
    if !files.scenes.is_empty() || !files.batteries.is_empty() {
        notes.push(format!(
            "{scenes_placed} of {} scene files and {batteries_placed} of {} battery summaries fall in an arm; the rest are of another day or time, or have no start.",
            files.scenes.len(),
            files.batteries.len()
        ));
    }
    if !notes.is_empty() {
        out.push('\n');
        for note in notes {
            out.push_str(&note);
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const JOURNAL: &str = "\
Oct 19 20:01:02 lamp-4ace lamp-voice[11]: lamp-voice second: request 7 heard 5 words, the last 610 ms after the line ended
Oct 19 20:01:02 lamp-4ace lamp-voice[11]: lamp-voice second: request 7: its answer is played: the text shows it was called
Oct 19 20:01:03 lamp-4ace lamp-voice[11]: lamp-voice second: request 7 chat round 1 first event in 840 ms
Oct 19 20:01:03 lamp-4ace lamp-voice[11]: lamp-voice second: request 7 first answer audio 1400 ms after the words were heard
Oct 19 20:02:10 lamp-4ace lamp-voice[11]: lamp-voice second: request 8 heard 0 words, the last 900 ms after the line ended
Oct 19 20:02:20 lamp-4ace lamp-voice[11]: lamp-voice second: request 9 heard 3 words, the last 700 ms after the line ended
Oct 19 20:02:20 lamp-4ace lamp-voice[11]: lamp-voice second: request 9: its answer is held back: nobody spoke to the lamp: no name, nobody turned to it, no conversation with it going on
Oct 19 20:02:30 lamp-4ace lamp-voice[11]: lamp-voice second: request 10: its answer is played: someone is turned to the lamp
Oct 19 20:02:31 lamp-4ace lamp-voice[11]: lamp-voice second: request 10 chat round 1 called not_for_me
Oct 19 20:03:00 lamp-4ace lamp-voice[11]: lamp-voice second: request 9 made a sound while its chat had not begun in 1200 ms
Oct 19 20:03:06 lamp-4ace lamp-voice[11]: lamp-voice second: request 11 chat round 1 had not begun in 6000 ms; the turn ends unanswered
Oct 19 20:03:10 lamp-4ace lamp-voice[11]: lamp-voice second: request 12 goes on from request 11: the name heard in it counts; its sound is not sent again
Oct 19 20:03:20 lamp-4ace lamp-voice[11]: lamp-voice second: request 13 goes on from request 12: its 1130 ms of sound go first
Oct 19 20:31:00 lamp-4ace lamp-voice[12]: lamp-voice provider: request 3: its answer is played: a conversation with the lamp is going on
Oct 19 20:31:01 lamp-4ace lamp-voice[12]: lamp-voice provider: request 3: first sound 300 ms after the model said it heard its name
Oct 19 20:31:20 lamp-4ace lamp-voice[12]: lamp-voice provider: request 4: its answer is held back: a busy room, and nothing shows it was called or that anyone is turned to the lamp
Oct 19 20:31:21 lamp-4ace lamp-voice[12]: lamp-voice provider: request 4: first sound, and the model has not said it heard its name
Oct 19 20:31:29 lamp-4ace lamp-voice[12]: lamp-voice provider: request 5 got no answer in 4000 ms; closed as unanswered, connection kept
Oct 19 20:31:29 lamp-4ace lamp-voice[12]: lamp-voice provider: request 5: the service sent something 1000 ms after it was closed as unanswered, and it was dropped (UnownedOutput)
Oct 19 20:31:30 lamp-4ace lamp-voice[12]: lamp-voice provider: a sound (request 6) got no answer in 900 ms, as a sound should; closed, connection kept
Oct 19 20:31:31 lamp-4ace lamp-voice[12]: lamp-voice provider: reminder request 7 got no answer in 900 ms; closed, connection kept
Oct 19 20:31:40 lamp-4ace lamp-voice[12]: lamp-voice provider: request 8: the model said it was not for it
Oct 19 20:31:45 lamp-4ace lamp-voice[12]: lamp-voice provider: request 9: the model said it was not for it after doing what was asked; not taken
Oct 19 20:31:30 lamp-4ace kernel: something else at 20:31:30
";

    fn row<'a>(table: &'a str, name: &str) -> Vec<&'a str> {
        table
            .lines()
            .find(|line| line.starts_with(&format!("| {name} |")))
            .unwrap_or_else(|| panic!("no row {name}"))
            .split('|')
            .map(str::trim)
            .filter(|cell| !cell.is_empty())
            .skip(1)
            .collect()
    }

    #[test]
    fn each_arm_is_scored_from_its_own_stretch_of_the_journal() {
        let arms = [
            Arm::parse("deepgram", "20:00", "20:30").unwrap(),
            Arm::parse("live", "20:30", "21:00").unwrap(),
        ];
        let table = score(JOURNAL, &arms);
        assert!(
            table.starts_with("| | deepgram | live |\n|---|---|---|\n"),
            "{table}"
        );
        assert_eq!(row(&table, "lines judged"), ["3", "2"]);
        assert_eq!(row(&table, "let out"), ["2", "1"]);
        assert_eq!(row(&table, "let out by its name or words"), ["1", "0"]);
        assert_eq!(row(&table, "let out by a face"), ["1", "0"]);
        assert_eq!(
            row(&table, "let out by a conversation going on"),
            ["0", "1"]
        );
        assert_eq!(row(&table, "held: nobody spoke to the lamp"), ["1", "0"]);
        assert_eq!(row(&table, "held: a busy room, no call"), ["0", "1"]);
        // Live's first sound counts once it isn't held back (Jarvis on #1899).
        assert_eq!(
            row(
                &table,
                "answered (first answer audio; Live: first sound, not held)"
            ),
            ["1", "1"]
        );
        assert_eq!(
            row(&table, "declined by the model (not_for_me)"),
            ["1", "1"]
        );
        // Not a wait sound's line, nor Live's sound, reminder or a late
        // message after the close.
        assert_eq!(row(&table, "unanswered in time"), ["1", "1"]);
        assert_eq!(row(&table, "lines with no words"), ["1", "—"]);
        assert_eq!(row(&table, "chat failed"), ["0", "—"]);
        // Only the carry that sends the sound again.
        assert_eq!(
            row(&table, "sound carried from the line before"),
            ["1", "—"]
        );
        assert_eq!(
            row(&table, "line end to words done, ms"),
            ["700 (900), 3", "—"]
        );
        assert_eq!(row(&table, "chat's first event, ms"), ["840 (840), 1", "—"]);
        assert_eq!(
            row(&table, "line end to first answer audio, ms"),
            ["2010 (2010), 1", "—"]
        );
        assert!(!table.contains("no voice lines"), "{table}");
    }

    /// Jarvis on #1899: request numbers start again with the service, so a
    /// request is known by its process too; an arm with no voice lines
    /// prints "—" and says so.
    #[test]
    fn a_restart_keeps_requests_apart_and_an_empty_arm_says_so() {
        let journal = "\
Oct 19 20:01:02 lamp-4ace lamp-voice[11]: lamp-voice second: request 7 heard 5 words, the last 610 ms after the line ended
Oct 19 20:01:03 lamp-4ace lamp-voice[11]: lamp-voice second: request 7 first answer audio 1400 ms after the words were heard
Oct 19 20:05:02 lamp-4ace lamp-voice[20]: lamp-voice second: request 7 heard 5 words, the last 900 ms after the line ended
Oct 19 20:05:03 lamp-4ace lamp-voice[20]: lamp-voice second: request 7 first answer audio 1800 ms after the words were heard
";
        let arms = [
            Arm::parse("deepgram", "20:00", "20:30").unwrap(),
            Arm::parse("late", "21:00", "21:30").unwrap(),
        ];
        let table = score(journal, &arms);
        assert_eq!(
            row(&table, "line end to first answer audio, ms"),
            ["2700 (2700), 2", "—"]
        );
        assert_eq!(
            row(
                &table,
                "answered (first answer audio; Live: first sound, not held)"
            ),
            ["2", "—"]
        );
        assert_eq!(row(&table, "lines judged"), ["0", "—"]);
        assert!(
            table.contains("\nlate: no voice lines in its stretch."),
            "{table}"
        );
        assert!(!table.contains("deepgram: no voice lines"), "{table}");
    }

    #[test]
    fn no_words_said_reach_the_table() {
        let said = "Oct 19 20:01:02 lamp-4ace lamp-voice[11]: lamp-voice second: request 7: its answer is held back: nobody spoke to the lamp\nOct 19 20:01:02 x: heard secret words";
        let table = score(said, &[Arm::parse("a", "20:00", "21:00").unwrap()]);
        assert!(!table.contains("secret"));
    }

    const SCENE: &str = "\
scenario day-bat  2026-10-19T13:03:10Z  say rate 185
   0.00s PLAY   A: Hey Lamp, what time is it?
   2.10s        [input_admitted]
   3.40s        [end of speech decided after 600 ms of silence (smart turn)]
   3.40s HEARD  Hey Lamp, what time is it?
   5.60s        [lamp starts speaking 2.2 s after that decision]
   5.60s LAMP   It's eight.
   7.00s        [turn finished: audio_written_unscored]
   8.00s PLAY   B: Yes, give me five minutes.
   9.00s        [duplex_yield]
  10.00s        [turn finished: interrupted]
";
    /// A battery summary's shape, as the first builder's
    /// `battery-night1-deepgram.txt` (#1899, 21:01): the rounds, then the
    /// sections, a section's own tally where it has one.
    const BATTERY: &str = "\
battery night1-deepgram-2003 20:03:00
== round 1, 1: busy room (expected: nothing said)
A: Sounds like a big saving.                       |
B: Hey Lamp, what's the weather?                   | Sunny and warm.
== round 1, 2: two people (expected: left alone)
A: Did you see the game?                           | Yes, a great one.
-- day
1. What time is it? -> It's eight.
2. Is it raining? -> (nothing)
day lines answered: 1 of 2
-- follow-ups (#1465)
follow-ups answered: 3 of 4
-- hand-overs
A: Take the reports, please.                       | Done.
B: And the mail.                                   |
";

    /// #1712, Fourth's shapes on #1722: the played test's own files add
    /// rows, counted into the arm their start falls in, with no words.
    #[test]
    fn scene_files_and_battery_summaries_add_rows_without_words() {
        let arms = [
            Arm::parse("deepgram", "20:00", "20:30").unwrap(),
            Arm::parse("live", "20:30", "21:00").unwrap(),
        ];
        let files = Files {
            scenes: vec![SCENE.to_owned()],
            // Written at 20:10 on 19 Oct at +07:00.
            batteries: vec![(BATTERY.to_owned(), Some(written("2026-10-19", "13:10")))],
            // 13:03 UTC is 20:03 at +07:00.
            offset: 7 * 3600,
            day: day("2026-10-19"),
        };
        let table = score_with("", &arms, &files);
        assert_eq!(row(&table, "scenes: lines played"), ["2", "0"]);
        assert_eq!(row(&table, "scenes: lines heard"), ["1", "0"]);
        assert_eq!(row(&table, "scenes: the lamp's turns"), ["1", "0"]);
        assert_eq!(
            row(&table, "scenes: turn finished, audio_written_unscored"),
            ["1", "0"]
        );
        assert_eq!(
            row(&table, "scenes: turn finished, interrupted"),
            ["1", "0"]
        );
        assert_eq!(row(&table, "scenes: yields"), ["1", "0"]);
        assert_eq!(
            row(&table, "scenes: end of speech to first sound, ms"),
            ["2200 (2200), 1", "—"]
        );
        // The rounds' lines are no section's (the first builder on #1899,
        // 21:01): one names the lamp and was answered, two are between
        // people and one of them was left alone.
        assert_eq!(
            row(&table, "battery rounds: lines naming the lamp"),
            ["1", "0"]
        );
        assert_eq!(
            row(&table, "battery rounds: lines naming the lamp, answered"),
            ["1", "0"]
        );
        assert_eq!(
            row(&table, "battery rounds: lines between people"),
            ["2", "0"]
        );
        assert_eq!(
            row(&table, "battery rounds: lines between people, left alone"),
            ["1", "0"]
        );
        // A section's own tally, where it has one; else its `|` lines.
        assert_eq!(row(&table, "battery day: lines played"), ["2", "0"]);
        assert_eq!(row(&table, "battery day: lines answered"), ["1", "0"]);
        assert_eq!(row(&table, "battery follow-ups: lines played"), ["4", "0"]);
        assert_eq!(
            row(&table, "battery follow-ups: lines answered"),
            ["3", "0"]
        );
        assert_eq!(row(&table, "battery hand-overs: lines played"), ["2", "0"]);
        assert_eq!(
            row(&table, "battery hand-overs: lines answered"),
            ["1", "0"]
        );
        for said in [
            "what time",
            "Sunny",
            "saving",
            "weather",
            "eight",
            "game",
            "raining",
            "reports",
            "mail",
            "Done",
        ] {
            assert!(!table.contains(said), "{said} in {table}");
        }
        assert!(
            table.contains("1 of 1 scene files and 1 of 1 battery summaries fall in an arm"),
            "{table}"
        );
    }

    /// Seconds since 1970 of a UTC date and time.
    fn written(date: &str, time: &str) -> i64 {
        day(date).unwrap() * 86_400 + i64::from(clock(time).unwrap())
    }

    /// Jarvis on #1899: an earlier day's scene or battery at the same time
    /// of day falls in no arm; a night's arms run past midnight, an arm
    /// that begins earlier than the first on the day after.
    #[test]
    fn files_are_placed_by_their_day_as_well_as_their_time() {
        let arms = [
            Arm::parse("evening", "20:00", "23:30").unwrap(),
            Arm::parse("night", "23:30", "00:30").unwrap(),
            Arm::parse("late", "00:30", "01:00").unwrap(),
        ];
        let scene_at = |stamp: &str| SCENE.replace("2026-10-19T13:03:10Z", stamp);
        let files = Files {
            scenes: vec![
                // 20:03 on the 19th, and on the 18th.
                scene_at("2026-10-19T13:03:10Z"),
                scene_at("2026-10-18T13:03:10Z"),
                // 00:10 and 00:40 on the 20th.
                scene_at("2026-10-19T17:10:00Z"),
                scene_at("2026-10-19T17:40:00Z"),
                // 00:10 on the 19th: the night before the run.
                scene_at("2026-10-18T17:10:00Z"),
            ],
            batteries: vec![
                (BATTERY.to_owned(), Some(written("2026-10-19", "13:10"))),
                (BATTERY.to_owned(), Some(written("2026-10-18", "13:10"))),
                // Begun at 23:58 on the 19th, written at 00:05 on the 20th.
                (
                    BATTERY.replace("20:03:00", "23:58:00"),
                    Some(written("2026-10-19", "17:05")),
                ),
                (BATTERY.to_owned(), None),
            ],
            offset: 7 * 3600,
            day: day("2026-10-19"),
        };
        let table = score_with("", &arms, &files);
        assert_eq!(row(&table, "scenes: lines played"), ["2", "2", "2"]);
        assert_eq!(row(&table, "battery day: lines played"), ["2", "2", "0"]);
        assert!(
            table.contains("3 of 5 scene files and 2 of 4 battery summaries fall in an arm"),
            "{table}"
        );
        // With no day, none is placed.
        let undated = Files { day: None, ..files };
        let table = score_with("", &arms, &undated);
        assert!(!table.contains("scenes: lines played"), "{table}");
        assert!(table.contains("0 of 5 scene files and 0 of 4"), "{table}");
    }

    #[test]
    fn a_day_is_read_as_written() {
        assert_eq!(day("1970-01-01"), Some(0));
        assert_eq!(day("2026-10-19"), Some(20_745));
        assert_eq!(day("2000-02-29"), Some(11_016));
        assert_eq!(day("2026-03-01"), Some(day("2026-02-28").unwrap() + 1));
        for wrong in [
            "2026-02-29",
            "2026-13-01",
            "2026-10-00",
            "2026-1-19",
            "26-10-19",
            "2026/10/19",
            "2026-10-1x",
        ] {
            assert_eq!(day(wrong), None, "{wrong}");
        }
    }

    #[test]
    fn times_and_arms_are_read_as_written() {
        assert_eq!(
            time_of_day("2026-10-19T20:01:02+0700 lamp-4ace x"),
            Some(72_062)
        );
        assert_eq!(
            time_of_day("Oct 19 20:01:02.123456 lamp-4ace x"),
            Some(72_062)
        );
        assert_eq!(time_of_day("no time here"), None);
        let night = Arm::parse("night", "23:30", "00:30").unwrap();
        assert!(night.holds(clock("23:45").unwrap()) && night.holds(clock("00:10").unwrap()));
        assert!(!night.holds(clock("01:00").unwrap()));
        assert!(Arm::parse("x", "25:00", "26:00").is_err());
        assert!(Arm::parse("x", "8:00", "9:00").is_err());
    }
}
