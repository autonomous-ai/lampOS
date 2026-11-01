//! The first stand of a lamp moved from V1, confirmed at the lamp (#2396;
//! the first builder on #1711, 21 Oct 15:0x).
//!
//! A moved lamp starts seated (`poses::seated_lamp_env`): its body holds the
//! head and base and never the arm, until a person beside it says yes. This
//! is how they say it:
//!
//! - **The ask.** Once someone is seen near the lamp (the eyes' presence
//!   file, a face at least [`NEAR_SHARE`] of the picture's height for
//!   [`NEAR_FOR_US`]), the lamp says [`ASK`] through the voice's say
//!   service, which says it only once the room has been quiet for its
//!   usual 15 s.
//! - **The yes.** A deliberate touch on the head pads or a button, held
//!   [`HOLD_US`], begun after the ask began: a pad already touched when the
//!   ask begins counts only once it has been let go and touched again
//!   ([`Touches`], from `controls/state`). **Or a spoken yes** (the owner,
//!   22 Oct): a short yes ([`is_yes`]) on `voice/heard`, heard after the
//!   ask's words ended, with a face near; the words are never journaled.
//!   Only in English: when the owner's language is another
//!   ([`LANGUAGE_FILE`]), the lamp says [`ASK_TOUCH_ONLY`] and the touch is
//!   the only yes (Fourth's finding on #4710).
//! - **The stand.** The factory's: lamp-body stopped, a rest read, the arm
//!   stood at 30% and then 100% by a body run by hand, and this arm's stand
//!   written into `lamp.env` as `deploy/factory/steps.sh` writes it. A second
//!   touch while it moves stops it where it is: the body is sent SIGTERM,
//!   eases back and lets go, and nothing is written.
//! - **No yes.** Nothing moves without a yes, touched or said, within
//!   [`ANSWER_US`]. The lamp
//!   asks once more at the next quiet time someone is near, at least
//!   [`ASK_AGAIN_AFTER_US`] later, and then waits: it never stands on its own.
//!   A person asks again by removing the record and restarting the unit.
//! - **Recorded.** [`Record`] in the body's state folder: how many asks, the
//!   outcome, when, and the stand's shoulder and elbow, the factory step's
//!   fields, for the app and Migrate's rollback to read.
//!
//! Nothing is asked in the first [`SETTLE`] after the run starts.
//! Whatever happens the body is started again at the end, on the confirmed
//! poses if they were written and seated otherwise, and the run ends well:
//! a lamp that stays seated is safe, and its unit stays active.
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::Duration,
};

/// What the lamp says, the first builder's words. It ends on words that
/// aren't a yes (the first builder on #2893, 12:1x), so an echo of its tail
/// can't read as one.
pub const ASK: &str = "I'd like to stretch for the first time. When you're ready, say yes, or rest your hand on my head.";
/// The ask in a busy room, where only the touch counts (the first builder on
/// #2602, 12:0x).
pub const ASK_TOUCH_ONLY: &str =
    "I'd like to stretch for the first time. Rest your hand on my head when you're ready.";
/// Said once the yes is taken, a beat before anything moves, so the person
/// isn't surprised by the arm.
pub const GOING: &str = "Okay, here I go.";
/// The beat after [`GOING`] is said.
pub const GOING_BEAT: Duration = Duration::from_secs(1);
/// The say service's skill name for it, and its name as the bus's
/// subscriber, so `[a-z][a-z0-9_]{0,31}` (`lamp_bus`'s names: a hyphen is
/// refused, El on #2893).
pub const SKILL: &str = "first_stand";
/// The words a spoken yes opens with.
const YES_WORDS: [&str; 9] = [
    "yes", "yeah", "yep", "yup", "sure", "okay", "ok", "ready", "go",
];
/// The only other words a spoken yes may hold: a line with any word not
/// here is no yes ("go to sleep", "okay, bye", "yeah, nah", "okay hold on":
/// El on #2893, since a list of words that take it back can never be
/// whole, and the yes moves the arm).
const YES_ALSO_WORDS: [&str; 2] = ["please", "now"];
/// And these, only whole: a line cut off inside one ("okay let's… not",
/// "yeah do") is no yes (El on #3093).
const YES_PHRASES: [[&str; 2]; 5] = [
    ["i'm", "ready"],
    ["im", "ready"],
    ["do", "it"],
    ["let's", "go"],
    ["go", "ahead"],
];
/// Words that alone are only "I heard you", never a yes (El on #3093).
const HEARD_YOU_WORDS: [&str; 4] = ["okay", "ok", "please", "now"];
/// A spoken yes is a short line: a longer one is someone talking.
const YES_MOST_WORDS: usize = 5;
/// Past this a line's pieces are not joined: a yes is far shorter (the
/// onboarding's 4 KiB).
const LINE_MOST_BYTES: usize = 4_096;
/// Lines the voice began hearing this soon after the ask's words ended are
/// the lamp's own, its echo through a weak canceller (the Reviewer-lead and
/// El on #2893).
pub const ECHO_TAIL_US: u64 = 1_500_000;

/// Whether `heard`, a finished line the voice heard, is a spoken yes to the
/// ask: short, opening with a yes, every word of it a yes's own or in a
/// whole phrase of one, more than "okay", and no question ("Yes?").
pub fn is_yes(heard: &str) -> bool {
    let words = words(heard);
    !words.is_empty()
        && !heard.contains('?')
        && words.len() <= YES_MOST_WORDS
        && YES_WORDS.contains(&words[0].as_str())
        && only_yes_words(&words)
        && !words
            .iter()
            .all(|word| HEARD_YOU_WORDS.contains(&word.as_str()))
}

/// Whether every word is a yes's own: a yes word, one of
/// [`YES_ALSO_WORDS`], or within a whole [`YES_PHRASES`] phrase.
fn only_yes_words(words: &[String]) -> bool {
    let mut at = 0;
    while at < words.len() {
        if let Some(phrase) = YES_PHRASES
            .iter()
            .find(|phrase| words[at..].starts_with(&phrase.map(String::from)))
        {
            at += phrase.len();
        } else if YES_WORDS.contains(&words[at].as_str())
            || YES_ALSO_WORDS.contains(&words[at].as_str())
        {
            at += 1;
        } else {
            return false;
        }
    }
    true
}

/// `text`'s words, lower case, without their marks.
fn words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|word| {
            word.replace('\u{2019}', "'")
                .trim_matches(|c: char| !c.is_alphanumeric() && c != '\'')
                .to_lowercase()
        })
        .filter(|word| !word.is_empty())
        .collect()
}

/// Whether `heard` is the lamp's own `asked` heard back: two words or more,
/// all of them a run of the ask's own words in order. A lone word can't be
/// told from a person's, so the time of the line keeps it out.
pub fn is_echo(heard: &str, asked: &str) -> bool {
    let heard = words(heard);
    let asked = words(asked);
    heard.len() >= 2
        && asked
            .windows(heard.len())
            .any(|run| run == heard.as_slice())
}

/// The unit's switch for the spoken yes: `off` makes every ask the
/// touch-only one (El on #2893).
pub const SPOKEN_SWITCH: &str = "LAMP_FIRST_STAND_SPOKEN";

/// Whether [`SPOKEN_SWITCH`]'s value turns the spoken yes off.
pub fn spoken_is_off(value: Option<&str>) -> bool {
    value.is_some_and(|value| value.trim() == "off")
}

/// The variable naming the owner's language file, the voice's own
/// (`lamp_voice::language::FILE_ENVIRONMENT`, #4473).
pub const LANGUAGE_FILE: &str = "LAMP_LANGUAGE_FILE";

/// Whether the language file's text is a language the lamp speaks other
/// than English (vi, ja, zh, ko). Missing, empty, `en` or unknown is
/// English, as the voice takes it.
pub fn speaks_other_than_english(text: Option<&str>) -> bool {
    text.is_some_and(|text| matches!(text.trim(), "vi" | "ja" | "zh" | "ko"))
}

/// Whether an ask takes the touch alone: in a busy room, with
/// [`SPOKEN_SWITCH`] off, or when the owner's language is not English,
/// since a spoken yes is matched by English words only (Fourth's finding on
/// #4710). `language` is the language file's text.
pub fn takes_touch_alone(busy: bool, spoken_switch: Option<&str>, language: Option<&str>) -> bool {
    busy || spoken_is_off(spoken_switch) || speaks_other_than_english(language)
}

/// The language file's text, from [`LANGUAGE_FILE`]: none when it names no
/// absolute path or the file can't be read.
pub fn language_text() -> Option<String> {
    use std::io::Read;
    let path = PathBuf::from(std::env::var_os(LANGUAGE_FILE)?);
    if !path.is_absolute() {
        return None;
    }
    let mut text = String::new();
    fs::File::open(path)
        .and_then(|file| file.take(64).read_to_string(&mut text))
        .ok()?;
    Some(text)
}

/// What became of a line the voice heard, for a spoken yes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Heard {
    /// Nothing to say of it: before the ask was spoken whole, or not
    /// finished, or a line that isn't a yes.
    Nothing,
    /// A yes, taken.
    Yes,
    /// A yes, not taken, and why: the room was busy, a second voice was
    /// heard in the window, or nobody was near.
    NotTaken(&'static str),
}

/// The spoken yes in one ask's window (the first builder on #2602, 12:0x):
/// only after the ask was spoken whole, never in a busy room, never once
/// another voice has spoken in the window, and only with a face near.
///
/// `voice/heard` carries a line in pieces, and a closing piece may be empty
/// (the Reviewer-lead on #2893): the pieces are joined and the whole line is
/// judged when it finishes, as the onboarding does. A line any piece of
/// which was heard before the window opened is the lamp's own, or begun
/// while it spoke, and counts for nothing.
#[derive(Debug, Clone, Default)]
pub struct SpokenYes {
    opens_at_us: Option<u64>,
    closed: bool,
    touch_only: bool,
    other_voice: bool,
    line: String,
    line_early: bool,
}

impl SpokenYes {
    /// A window whose ask was made in a busy room takes the touch alone.
    pub fn new(touch_only: bool) -> Self {
        Self {
            touch_only,
            ..Self::default()
        }
    }

    /// The words asked in this window.
    pub fn asked(&self) -> &'static str {
        if self.touch_only { ASK_TOUCH_ONLY } else { ASK }
    }

    /// The ask's words ended at `spoken_us` (the bus's monotonic clock):
    /// lines begun [`ECHO_TAIL_US`] after it are the person's.
    pub fn open(&mut self, spoken_us: u64) {
        self.opens_at_us = Some(spoken_us.saturating_add(ECHO_TAIL_US));
    }

    /// The lamp itself said something (`voice/said`), stamped `at_us`: lines
    /// begun before [`ECHO_TAIL_US`] after it may be its echo, and count
    /// for nothing (El on #3093).
    pub fn lamp_said(&mut self, at_us: u64) {
        if let Some(opens) = self.opens_at_us.as_mut() {
            *opens = (*opens).max(at_us.saturating_add(ECHO_TAIL_US));
        }
    }

    /// The window is over ([`ANSWER_US`] after the ask): no yes counts.
    pub fn close(&mut self) {
        self.closed = true;
    }

    /// A piece of a line the voice heard, stamped `at_us` by the voice, with
    /// whether the room is busy and a face is near now.
    pub fn line(
        &mut self,
        piece: &str,
        at_us: u64,
        finished: bool,
        busy: bool,
        near: bool,
    ) -> Heard {
        if self.opens_at_us.is_none_or(|opens| at_us < opens) {
            self.line_early = true;
        }
        if self.line.len() + piece.len() <= LINE_MOST_BYTES {
            self.line.push_str(piece);
        }
        if !finished {
            return Heard::Nothing;
        }
        let text = std::mem::take(&mut self.line);
        let early = std::mem::take(&mut self.line_early);
        // An empty line is no voice; the lamp's own words are none either.
        if early || self.closed || text.trim().is_empty() || is_echo(&text, self.asked()) {
            return Heard::Nothing;
        }
        if !is_yes(&text) {
            // Someone said something else: a second voice, or the person
            // talking of something else. Only the touch counts from now.
            self.other_voice = true;
            return Heard::Nothing;
        }
        if self.touch_only {
            Heard::NotTaken("the ask took the touch alone")
        } else if busy {
            Heard::NotTaken("the room is busy")
        } else if self.other_voice {
            Heard::NotTaken("another voice was heard after the ask")
        } else if !near {
            Heard::NotTaken("nobody is seen near")
        } else {
            Heard::Yes
        }
    }
}

/// The near face's height as a share of the picture's: someone at arm's
/// length or closer (lamp-a63e, a person seated at the desk: 0.245).
pub const NEAR_SHARE: f32 = 0.12;
/// Someone is near for this long before the lamp asks: not a person passing.
pub const NEAR_FOR_US: u64 = 5_000_000;
/// A presence file older than this is no one there (the eyes keep it fresh
/// several times a second while there is a face).
pub const PRESENCE_FRESH_US: u64 = 2_000_000;
/// The yes: a touch held this long.
pub const HOLD_US: u64 = 1_000_000;
/// How long the lamp waits for the yes after the ask.
pub const ANSWER_US: u64 = 120_000_000;
/// The asks, and the least time between them.
pub const MOST_ASKS: u32 = 2;
pub const ASK_AGAIN_AFTER_US: u64 = 600_000_000;
/// How long the say service may keep the ask waiting for its quiet.
pub const SAY_EXPIRES_MS: u64 = 120_000;
/// The body run by hand: started for this long before it is told to stand,
/// and given this long to stand and settle (steps.sh's pause 4 and
/// STAND_WAIT).
pub const BODY_STARTS: Duration = Duration::from_secs(4);
pub const STAND_WAIT: Duration = Duration::from_secs(34);
/// Nothing is asked this soon after the run starts: an update's check that
/// every part is running (45 s, V1's `software-update`, from activation)
/// never meets the body stopped for a stand.
pub const SETTLE: Duration = Duration::from_secs(120);
/// After its SIGTERM, a body run by hand is given this long to ease back,
/// fold and let go before it is killed: it folds in about 6 s (lamp-body's
/// "folded in 6.0 s"), and nothing must cut that short (Jarvis on #2541).
pub const FOLD_WAIT: Duration = Duration::from_secs(20);
/// A hand still on the head after the yes is waited off this long before
/// the stand begins, so that any touch while it moves is a stop.
pub const HANDS_OFF_WAIT: Duration = Duration::from_secs(10);
/// How often the touch is looked at.
pub const LOOK_EVERY: Duration = Duration::from_millis(100);

/// The yes, or the stop, from what `controls/state` says of the head pads
/// and buttons. Nothing counts until the hands have been seen off them, so a
/// pad touched before the ask began is not a yes.
#[derive(Debug, Clone)]
pub struct Touches {
    hold_us: u64,
    armed: bool,
    since_us: Option<u64>,
}

impl Touches {
    pub fn new(hold_us: u64) -> Self {
        Self {
            hold_us,
            armed: false,
            since_us: None,
        }
    }

    /// One that counts a touch from the first look: for the stop, once the
    /// hands were seen off the controls.
    pub fn armed(hold_us: u64) -> Self {
        Self {
            armed: true,
            ..Self::new(hold_us)
        }
    }

    /// Whether a touch has now been held long enough. `touched` is None
    /// while the controls' state is not known: no touch, and not a release.
    pub fn observe(&mut self, touched: Option<bool>, now_us: u64) -> bool {
        match touched {
            None => {
                self.since_us = None;
                false
            }
            Some(false) => {
                self.armed = true;
                self.since_us = None;
                false
            }
            Some(true) if self.armed => {
                let since = *self.since_us.get_or_insert(now_us);
                now_us.saturating_sub(since) >= self.hold_us
            }
            Some(true) => false,
        }
    }
}

/// While the arm moves, the controls not known for this long stop it: a
/// person's touch could no longer be seen (the reviewer-lead on #2541).
pub const CONTROLS_LOST_US: u64 = 1_000_000;

/// Whether the controls have been unknown for [`CONTROLS_LOST_US`].
#[derive(Debug, Clone, Default)]
pub struct ControlsLost {
    since_us: Option<u64>,
}

impl ControlsLost {
    pub fn observe(&mut self, touched: Option<bool>, now_us: u64) -> bool {
        if touched.is_some() {
            self.since_us = None;
            return false;
        }
        let since = *self.since_us.get_or_insert(now_us);
        now_us.saturating_sub(since) >= CONTROLS_LOST_US
    }
}

/// A body run by hand as its own process (`setpriv` execs it, so this pid
/// is the body's: nothing between passes the signal on and kills it 2 s
/// later, as `runuser` does; Jarvis on #2541). However its run ends, early
/// return or not, a body still running is sent SIGTERM, which eases the arm
/// back, folds it and lets it go, and is waited for up to [`FOLD_WAIT`]; only
/// past that is it killed. So no body is left holding the servo lock, and
/// none is cut short mid-fold.
#[cfg(unix)]
pub struct HandRun(pub std::process::Child);

#[cfg(unix)]
impl HandRun {
    /// Ask it to stop and wait for it, at most `within`; whether it ended
    /// by itself.
    pub fn stop_within(&mut self, within: Duration) -> bool {
        if !matches!(self.0.try_wait(), Ok(None)) {
            let _ = self.0.wait();
            return true;
        }
        if let Some(pid) = rustix::process::Pid::from_raw(self.0.id() as i32) {
            let _ = rustix::process::kill_process(pid, rustix::process::Signal::TERM);
        }
        let asked = std::time::Instant::now();
        while asked.elapsed() < within {
            if !matches!(self.0.try_wait(), Ok(None)) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        eprintln!(
            "lamp-first-stand: the body did not end within {} s of its stop: killed",
            within.as_secs()
        );
        let _ = self.0.kill();
        let _ = self.0.wait();
        false
    }
}

#[cfg(unix)]
impl Drop for HandRun {
    fn drop(&mut self) {
        self.stop_within(FOLD_WAIT);
    }
}

/// Whether the head pads or a button are touched, from `controls/state`'s
/// payload; None when it cannot be read or the controls are not ready.
pub fn touched(state: &lamp_msgs::ControlsState) -> Option<bool> {
    state
        .ready
        .then(|| state.head_active || state.buttons.iter().any(|button| button.pressed))
}

/// Whether the presence file says someone is near: its one number, the near
/// face's height as a share of the picture's, written within
/// [`PRESENCE_FRESH_US`].
pub fn near(text: &str, age_us: u64) -> bool {
    age_us <= PRESENCE_FRESH_US
        && text
            .trim()
            .parse::<f32>()
            .is_ok_and(|share| share >= NEAR_SHARE)
}

/// Where the arm stopped standing, from the body's lines: the last one at
/// its full stand (`stand 0.95` to `1.00`), `arm goals [..] positions
/// [SHOULDER, ELBOW]`, as `steps.sh`'s `stood_at` reads it.
pub fn stood_at(log: &str) -> Option<(u16, u16)> {
    log.lines()
        .rev()
        .filter(|line| {
            line.split("stand ").skip(1).any(|after| {
                let share = after.get(..4).unwrap_or("");
                share == "1.00"
                    || (share.starts_with("0.9")
                        && share
                            .as_bytes()
                            .get(3)
                            .is_some_and(|d| (b'5'..=b'9').contains(d)))
            })
        })
        .find_map(|line| {
            let after = line.split("arm goals [").nth(1)?;
            let after = after.split("] positions [").nth(1)?;
            let inside = after.split(']').next()?;
            let mut parts = inside.split(',').map(|part| part.trim().parse::<u16>());
            match (parts.next(), parts.next(), parts.next()) {
                (Some(Ok(shoulder)), Some(Ok(elbow)), None) => Some((shoulder, elbow)),
                _ => None,
            }
        })
}

/// The rest read's `lamp.env` with this arm's stand: `--stand P,R,Y,S,E`
/// given this arm's shoulder and elbow, and the comment saying so, as
/// `steps.sh` writes it.
pub fn confirmed_lamp_env(read: &str, shoulder: u16, elbow: u16) -> Option<String> {
    const FOUND: &str =
        "# home, folded-arm: found at rest. stand: lamp-4ace's with this lamp's roll and yaw.";
    const STOOD: &str = "# home, folded-arm: found at rest. stand: this arm's, stood at 100% at the lamp's first stand, confirmed by a touch, with this lamp's roll and yaw.";
    let mut stood = false;
    let lines: Vec<String> = read
        .lines()
        .map(|line| {
            if let Some(rest) = line.strip_prefix(FOUND) {
                return format!("{STOOD}{rest}");
            }
            let Some((before, after)) = line.split_once("--stand ") else {
                return line.to_owned();
            };
            let (value, rest) = after.split_once(' ').unwrap_or((after, ""));
            let fields: Vec<&str> = value.split(',').collect();
            if fields.len() != 5 || fields.iter().any(|f| f.parse::<u16>().is_err()) {
                return line.to_owned();
            }
            stood = true;
            let space = if rest.is_empty() { "" } else { " " };
            format!(
                "{before}--stand {},{},{},{shoulder},{elbow}{space}{rest}",
                fields[0], fields[1], fields[2]
            )
        })
        .collect();
    stood.then(|| lines.join("\n") + "\n")
}

/// What became of the first stand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Asked, and no yes came in time.
    NoAnswer,
    /// It stood and `lamp.env` was written.
    Stood,
    /// A second touch stopped it; nothing was written.
    Stopped,
    /// The body or the read failed; nothing was written.
    Failed,
}

/// `value` as JSON at `path`, whole or not at all: written beside it, synced,
/// renamed over it and the folder synced.
pub fn write_json(path: &Path, value: &impl Serialize) -> io::Result<()> {
    use std::io::Write;
    let new = path.with_extension("json.new");
    let mut file = fs::File::create(&new)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.sync_all()?;
    fs::rename(&new, path)?;
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

/// The record in the body's state folder.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub asks: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<Outcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_unix_ms: Option<u64>,
    /// When the last ask was said, for the wait before the next.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asked_at_unix_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shoulder: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elbow: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

impl Record {
    pub fn read(path: &Path) -> Record {
        fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn write(&self, path: &Path) -> io::Result<()> {
        write_json(path, self)
    }

    /// Whether the lamp may ask now, `now_unix_ms`: never once it has
    /// stood or been stopped, never after [`MOST_ASKS`], and not within
    /// [`ASK_AGAIN_AFTER_US`] of the last ask.
    pub fn may_ask(&self, now_unix_ms: u64) -> bool {
        !matches!(self.outcome, Some(Outcome::Stood | Outcome::Stopped))
            && self.asks < MOST_ASKS
            && self
                .asked_at_unix_ms
                .is_none_or(|at| now_unix_ms.saturating_sub(at) >= ASK_AGAIN_AFTER_US / 1_000)
    }
}

/// Where the run finds and puts things.
#[derive(Debug, Clone)]
pub struct Options {
    pub unit_id: String,
    pub device: PathBuf,
    pub lock_directory: PathBuf,
    pub body_user: String,
    pub bus_root: PathBuf,
    pub presence_file: PathBuf,
    pub say_socket: PathBuf,
    pub lamp_env: PathBuf,
    pub record: PathBuf,
    /// lamp-body and lamp-body-measure, in the body's folder.
    pub body: PathBuf,
    pub measure: PathBuf,
}

#[cfg(target_os = "linux")]
pub use run::run;

#[cfg(target_os = "linux")]
mod run {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Write},
        os::unix::{
            fs::{DirBuilderExt, OpenOptionsExt},
            net::UnixDatagram,
            net::UnixStream,
        },
        process::{Command, Stdio},
        sync::mpsc,
        thread,
        time::{Instant, SystemTime, UNIX_EPOCH},
    };

    fn unix_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_millis() as u64)
    }

    fn say(line: &str) {
        eprintln!("lamp-first-stand: {line}");
    }

    /// The touch now, from the bus's kept `controls/state`.
    fn touch_now(bus: &lamp_bus::Bus) -> Option<bool> {
        let message = bus.latest(lamp_msgs::ControlsState::TOPIC).ok().flatten()?;
        if message.expired {
            return None;
        }
        let state = message.parse::<lamp_msgs::ControlsState>().ok()?;
        // Each change of what the controls say, for the journal: which
        // source a yes or a stop came from.
        static SEEN: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
        let pressed: Vec<&str> = state
            .buttons
            .iter()
            .filter(|button| button.pressed)
            .map(|button| button.name.as_str())
            .collect();
        let now = format!(
            "ready {} head {} buttons {pressed:?}",
            state.ready, state.head_active
        );
        if let Ok(mut seen) = SEEN.lock()
            && *seen != now
        {
            say(&format!("controls: {now}"));
            *seen = now;
        }
        touched(&state)
    }

    fn near_now(options: &Options) -> bool {
        let Ok(meta) = fs::metadata(&options.presence_file) else {
            return false;
        };
        let age_us = meta
            .modified()
            .ok()
            .and_then(|at| at.elapsed().ok())
            .map_or(u64::MAX, |age| age.as_micros() as u64);
        fs::read_to_string(&options.presence_file).is_ok_and(|text| near(&text, age_us))
    }

    /// Wait until someone has been near for [`NEAR_FOR_US`].
    fn wait_for_someone(options: &Options) {
        let mut since: Option<Instant> = None;
        loop {
            if near_now(options) {
                let at = *since.get_or_insert_with(Instant::now);
                if at.elapsed() >= Duration::from_micros(NEAR_FOR_US) {
                    return;
                }
            } else {
                since = None;
            }
            thread::sleep(Duration::from_millis(500));
        }
    }

    /// What the say service said of the ask: `begun`, then its outcome.
    enum Said {
        Begun,
        /// The outcome, and why when the say service says.
        Outcome(String, Option<String>),
    }

    fn ask(options: &Options, text: &str) -> io::Result<mpsc::Receiver<Said>> {
        let mut stream = UnixStream::connect(&options.say_socket)?;
        let line = serde_json::json!({"skill": SKILL, "text": text, "priority": 5,
            "expires_in_ms": SAY_EXPIRES_MS});
        stream.write_all(format!("{line}\n").as_bytes())?;
        let (tell, told) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stream).lines() {
                let Ok(line) = line else { break };
                let value: serde_json::Value =
                    serde_json::from_str(&line).unwrap_or(serde_json::Value::Null);
                let said = match (value["event"].as_str(), value["outcome"].as_str()) {
                    (Some("begun"), _) => Said::Begun,
                    (_, Some(outcome)) => {
                        Said::Outcome(outcome.to_owned(), value["why"].as_str().map(str::to_owned))
                    }
                    _ => continue,
                };
                let last = matches!(said, Said::Outcome(..));
                if tell.send(said).is_err() || last {
                    break;
                }
            }
        });
        Ok(told)
    }

    /// One ask and its answer: Some(true) for a yes, Some(false) when it
    /// was said and no yes came, None when it was not said (asked again
    /// later, not counted).
    fn ask_and_wait(options: &Options, bus: &lamp_bus::Bus) -> Option<bool> {
        // What the voice hears, from before the ask: only a line begun
        // after the ask was spoken whole, and its echo's tail, can be its
        // yes, so the lamp's own words never count. Without the bus's \`voice/heard\` (the voice
        // with \`LAMP_BUS=on\`), the touch alone is the yes.
        let mut heard = bus
            .subscribe_typed(lamp_msgs::Speech::TOPICS[0], SKILL, lamp_msgs::Speech::TYPE)
            .map_err(|error| {
                say(&format!(
                    "what the voice hears isn't read ({error}): the touch alone is the yes"
                ))
            })
            .ok();
        // What the lamp itself says, so a late echo of it is no yes (El on
        // #3093). Without it, the spoken yes isn't listened for.
        let mut said = heard.as_ref().and_then(|_| {
            bus.subscribe_typed(lamp_msgs::Speech::TOPICS[1], SKILL, lamp_msgs::Speech::TYPE)
                .map_err(|error| {
                    say(&format!(
                        "what the lamp says isn't read ({error}): the touch alone is the yes"
                    ))
                })
                .ok()
        });
        if said.is_none() {
            heard = None;
        }
        // In a busy room only the touch counts, and the ask says so; and
        // with `LAMP_FIRST_STAND_SPOKEN=off`, or an owner's language other
        // than English, always.
        let touch_alone = takes_touch_alone(
            busy_now(bus),
            std::env::var(SPOKEN_SWITCH).ok().as_deref(),
            language_text().as_deref(),
        );
        let mut spoken_yes = SpokenYes::new(touch_alone || heard.is_none());
        let told = match ask(options, spoken_yes.asked()) {
            Ok(told) => told,
            Err(error) => {
                say(&format!(
                    "the voice's say service is not there ({error}); asked again in a minute"
                ));
                return None;
            }
        };
        let mut touches = Touches::new(HOLD_US);
        let mut begun: Option<Instant> = None;
        // The yes counts only once the ask's words have ended (the first
        // builder on #2541): a hand on the head while the lamp speaks, a
        // pet, is not taken for an answer to what it hasn't finished saying.
        let mut spoken: Option<Instant> = None;
        let started = Instant::now();
        loop {
            match told.try_recv() {
                Ok(Said::Begun) => {
                    say("the ask has begun");
                    begun.get_or_insert_with(Instant::now);
                }
                Ok(Said::Outcome(outcome, _)) if outcome == "spoken" => {
                    say("the ask was spoken");
                    spoken = Some(Instant::now());
                    spoken_yes.open(lamp_ipc::monotonic_us());
                }
                // Not said to its end: no yes is taken for words not heard.
                Ok(Said::Outcome(outcome, why)) => {
                    say(&format!(
                        "the ask was not said to its end: {outcome}{}; asked again later",
                        why.map(|why| format!(" ({why})")).unwrap_or_default()
                    ));
                    return None;
                }
                Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => {}
            }
            // Every line waiting is read on every pass, and judged by when
            // the voice heard it: one begun before the window opened counts
            // for nothing, whenever it is read (the Reviewer-lead on #2893).
            // While the lamp is heard speaking, its words may come back:
            // the tail runs from the end of what it plays, not from when
            // its text arrived (El on #3093).
            if speaking_now(bus) {
                spoken_yes.lamp_said(lamp_ipc::monotonic_us());
            }
            let verdict = heard_lines(heard.as_mut(), said.as_mut(), &mut spoken_yes, bus, options);
            let Some(at) = spoken else {
                if started.elapsed() > Duration::from_millis(SAY_EXPIRES_MS + 10_000) {
                    say(if begun.is_some() {
                        "the ask was begun and never finished; asked again later"
                    } else {
                        "the ask was never begun; asked again later"
                    });
                    return None;
                }
                thread::sleep(LOOK_EVERY);
                continue;
            };
            let now_us = at.elapsed().as_micros() as u64;
            if touches.observe(touch_now(bus), now_us) {
                say("yes: a touch held a second");
                return Some(true);
            }
            match verdict {
                // Never the words: only that a yes was heard.
                Heard::Yes => {
                    say("yes: said, with someone near");
                    return Some(true);
                }
                Heard::NotTaken(why) => say(&format!("a yes was heard and not taken: {why}")),
                Heard::Nothing => {}
            }
            if now_us >= ANSWER_US {
                spoken_yes.close();
                say(&format!(
                    "no yes, touched or said, within {} s of the ask; nothing moves",
                    ANSWER_US / 1_000_000
                ));
                return Some(false);
            }
            thread::sleep(LOOK_EVERY);
        }
    }

    /// Every line waiting on `heard`, through [`SpokenYes`], once what the
    /// lamp said meanwhile is taken in: the first yes taken, else the last
    /// yes not taken, else nothing.
    fn heard_lines(
        heard: Option<&mut lamp_bus::Subscriber>,
        said: Option<&mut lamp_bus::Subscriber>,
        spoken_yes: &mut SpokenYes,
        bus: &lamp_bus::Bus,
        options: &Options,
    ) -> Heard {
        let (Some(heard), Some(said)) = (heard, said) else {
            return Heard::Nothing;
        };
        while let Ok(Some(message)) = said.recv(Duration::ZERO) {
            spoken_yes.lamp_said(message.header.at_us);
        }
        let mut verdict = Heard::Nothing;
        while let Ok(Some(message)) = heard.recv(Duration::ZERO) {
            if let Ok(line) = message.parse::<lamp_msgs::Speech>() {
                let at_us = message.header.at_us;
                match spoken_yes.line(
                    &line.text,
                    at_us,
                    line.finished,
                    busy_now(bus),
                    near_now(options),
                ) {
                    Heard::Yes => return Heard::Yes,
                    Heard::NotTaken(why) => verdict = Heard::NotTaken(why),
                    Heard::Nothing => {}
                }
            }
        }
        verdict
    }

    /// Whether the voice says it is speaking now (`voice/state`).
    fn speaking_now(bus: &lamp_bus::Bus) -> bool {
        bus.latest(lamp_msgs::VoiceState::TOPIC)
            .ok()
            .flatten()
            .filter(|message| !message.expired)
            .and_then(|message| message.parse::<lamp_msgs::VoiceState>().ok())
            .is_some_and(|voice| voice.state == lamp_msgs::Voice::Speaking)
    }

    /// Whether the voice says the room is busy now (`room/state`).
    fn busy_now(bus: &lamp_bus::Bus) -> bool {
        bus.latest(lamp_msgs::RoomState::TOPIC)
            .ok()
            .flatten()
            .filter(|message| !message.expired)
            .and_then(|message| message.parse::<lamp_msgs::RoomState>().ok())
            .is_some_and(|room| room.state == lamp_msgs::Room::Busy)
    }

    /// [`GOING`], waited for a few seconds at most, then the beat.
    fn say_going(options: &Options) {
        if let Ok(told) = ask(options, GOING) {
            let until = Instant::now() + Duration::from_secs(6);
            while Instant::now() < until {
                if let Ok(Said::Outcome(..)) = told.recv_timeout(LOOK_EVERY) {
                    break;
                }
            }
        }
        thread::sleep(GOING_BEAT);
    }

    fn systemctl(verb: &str, unit: &str) -> io::Result<()> {
        let status = Command::new("systemctl").args([verb, unit]).status()?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "systemctl {verb} {unit}: {status}"
            )))
        }
    }

    /// The program as the body's user, with its groups (the servo port's
    /// `dialout`): `setpriv` execs it, so the child's pid is the program's
    /// own and a signal reaches it alone (Jarvis on #2541).
    fn as_body(options: &Options, program: &Path) -> io::Result<Command> {
        let user = nix::unistd::User::from_name(&options.body_user)
            .map_err(io::Error::other)?
            .ok_or_else(|| io::Error::other("the body's user is not there"))?;
        let mut command = Command::new("setpriv");
        command
            .arg(format!("--reuid={}", user.uid))
            .arg(format!("--regid={}", user.gid))
            .args(["--init-groups", "--"])
            .arg(program);
        Ok(command)
    }

    fn rest_read(options: &Options, file: &Path) -> io::Result<(bool, String)> {
        let output = as_body(options, &options.measure)?
            .arg("--device")
            .arg(&options.device)
            .args(["--unit-id", &options.unit_id])
            .arg("--lock-directory")
            .arg(&options.lock_directory)
            .arg("--write")
            .arg(file)
            .output()?;
        let said = String::from_utf8_lossy(&output.stdout).into_owned()
            + &String::from_utf8_lossy(&output.stderr);
        eprint!("{said}");
        Ok((output.status.success(), said))
    }

    /// How a run of the body by hand ended.
    enum Ran {
        Done,
        Stopped,
    }

    /// `steps.sh`'s `body_once`: the body on a private mood socket; with a
    /// share, told "listening" once and given [`STAND_WAIT`], then SIGTERM
    /// eases it back and lets it go. A touch while it runs stops it.
    fn body_once(
        options: &Options,
        bus: &lamp_bus::Bus,
        dir: &Path,
        share: Option<(u8, &str)>,
        stop: &mut Touches,
    ) -> io::Result<Ran> {
        let log = fs::File::create(dir.join("body.log"))?;
        let mood = dir.join("mood.sock");
        let mut command = as_body(options, &options.body)?;
        command
            .arg("--device")
            .arg(&options.device)
            .args(["--unit-id", &options.unit_id])
            .arg("--lock-directory")
            .arg(&options.lock_directory)
            .arg("--mood-socket")
            .arg(&mood)
            .args([
                "--reach",
                "150,15,1,250",
                "--speed",
                "120",
                "--servo-speed",
                "1000",
                "--servo-acceleration",
                "60",
                "--force-limit",
                "600",
                "--rest-glance",
                "off",
                "--look-at-name",
                "off",
                "--let-go-at-stop",
                "on",
            ]);
        match share {
            Some((share, poses)) => {
                command.args([
                    "--wake",
                    "on",
                    "--stand-share",
                    &share.to_string(),
                    "--stand-seconds",
                    "6",
                    "--awake-seconds",
                    "20",
                    "--idle",
                    "off",
                ]);
                command.args(poses.split_whitespace());
            }
            None => {
                command.args(["--wake", "off"]);
            }
        }
        let mut body = HandRun(
            command
                .stdin(Stdio::null())
                .stdout(log.try_clone()?)
                .stderr(log)
                .spawn()?,
        );
        let mut lost = ControlsLost::default();
        let began = Instant::now();
        let mut told = share.is_none();
        let until = BODY_STARTS
            + if share.is_some() {
                STAND_WAIT
            } else {
                Duration::ZERO
            };
        while began.elapsed() < until {
            if body.0.try_wait()?.is_some() {
                return Err(io::Error::other(
                    "the body ended by itself (its lines are in body.log)",
                ));
            }
            let now_us = began.elapsed().as_micros() as u64;
            let touched = touch_now(bus);
            if share.is_some() && stop.observe(touched, now_us) {
                say("a touch while it moved: stopped, easing back");
                return Ok(Ran::Stopped);
            }
            if share.is_some() && lost.observe(touched, now_us) {
                return Err(io::Error::other(
                    "the controls were not known for a second while it moved, so a touch could not stop it: eased back",
                ));
            }
            if !told && began.elapsed() >= BODY_STARTS {
                UnixDatagram::unbound()?.send_to(br#"{"mood":"listening","level":128}"#, &mood)?;
                told = true;
            }
            thread::sleep(LOOK_EVERY);
        }
        drop(body);
        Ok(Ran::Done)
    }

    /// The stand, from a stopped body to a written `lamp.env`.
    fn stand(options: &Options, bus: &lamp_bus::Bus, record: &mut Record) -> io::Result<Outcome> {
        // A new folder, never one already there (create_dir, not _all), in
        // the unit's private /tmp (PrivateTmp=yes), handed to the body's user
        // by id without following a link (the reviewer-lead on #2541).
        let dir =
            std::env::temp_dir().join(format!("first-stand.{}.{}", std::process::id(), unix_ms()));
        fs::DirBuilder::new().mode(0o700).create(&dir)?;
        let user = nix::unistd::User::from_name(&options.body_user)
            .map_err(io::Error::other)?
            .ok_or_else(|| io::Error::other("the body's user is not there"))?;
        std::os::unix::fs::lchown(&dir, Some(user.uid.as_raw()), Some(user.gid.as_raw()))?;
        let read = dir.join("lamp.env");
        let (read_ok, said) = rest_read(options, &read)?;
        // The yes hand off the controls before anything moves, so that any
        // touch while it moves is a stop (Jarvis on #2541).
        let off = Instant::now();
        while touch_now(bus) != Some(false) {
            if off.elapsed() >= HANDS_OFF_WAIT {
                return Err(io::Error::other(
                    "a hand stayed on the head or a button, or the controls were not known: nothing moved",
                ));
            }
            thread::sleep(LOOK_EVERY);
        }
        let mut stop = Touches::armed(0);
        if !read_ok {
            if !said.contains("has torque on") {
                return Err(io::Error::other("lamp-body-measure refused the rest read"));
            }
            say("the lamp is held, torque on: letting it go where it lies");
            body_once(options, bus, &dir, None, &mut stop)?;
            let _ = fs::remove_file(&read);
            if !rest_read(options, &read)?.0 {
                return Err(io::Error::other(
                    "lamp-body-measure still refused after the lamp was let go",
                ));
            }
        }
        let text = fs::read_to_string(&read)?;
        let poses = text
            .lines()
            .find_map(|line| line.strip_prefix("BODY_POSES=\"")?.strip_suffix('"'))
            .ok_or_else(|| io::Error::other("the read gave no BODY_POSES"))?
            .to_owned();
        for share in [30, 100] {
            say(&format!("standing at {share}%"));
            if let Ran::Stopped = body_once(options, bus, &dir, Some((share, &poses)), &mut stop)? {
                return Ok(Outcome::Stopped);
            }
        }
        let log = fs::read_to_string(dir.join("body.log"))?;
        let (shoulder, elbow) = stood_at(&log)
            .ok_or_else(|| io::Error::other("the arm did not reach its full stand"))?;
        say(&format!(
            "this arm stands at shoulder {shoulder}, elbow {elbow}"
        ));
        let confirmed = confirmed_lamp_env(&text, shoulder, elbow)
            .ok_or_else(|| io::Error::other("the read gave no --stand"))?;
        if let Some(parent) = options.lamp_env.parent() {
            fs::create_dir_all(parent)?;
        }
        let new = options.lamp_env.with_extension("env.new");
        let _ = fs::remove_file(&new);
        {
            // Whole on disk before it is linked into place: after a power
            // cut, an empty lamp.env would never be asked for again.
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o644)
                .open(&new)?;
            file.write_all(confirmed.as_bytes())?;
            file.sync_all()?;
        }
        // Never over a lamp.env written meanwhile.
        fs::hard_link(&new, &options.lamp_env)?;
        fs::remove_file(&new)?;
        if let Some(parent) = options.lamp_env.parent() {
            fs::File::open(parent)?.sync_all()?;
        }
        let _ = fs::remove_dir_all(&dir);
        (record.shoulder, record.elbow) = (Some(shoulder), Some(elbow));
        say(&format!("wrote {}", options.lamp_env.display()));
        Ok(Outcome::Stood)
    }

    fn finish(options: &Options, record: &mut Record, outcome: Outcome, why: Option<String>) {
        record.outcome = Some(outcome);
        record.at_unix_ms = Some(unix_ms());
        record.why = why;
        if let Err(error) = record.write(&options.record) {
            say(&format!(
                "could not write {}: {error}",
                options.record.display()
            ));
        }
    }

    /// The whole first stand. Ends well whatever became of it; an error is
    /// only a fault of its own (the bus or the record cannot be opened).
    pub fn run(options: &Options) -> io::Result<()> {
        if options.lamp_env.exists() {
            say("the first stand is confirmed already; nothing to ask");
            return Ok(());
        }
        let bus = lamp_bus::Bus::new(&options.bus_root)?;
        let mut record = Record::read(&options.record);
        if record.may_ask(unix_ms()) {
            thread::sleep(SETTLE);
        }
        loop {
            if !record.may_ask(unix_ms()) {
                if matches!(record.outcome, Some(Outcome::Stood | Outcome::Stopped))
                    || record.asks >= MOST_ASKS
                {
                    say(&format!(
                        "not asking again ({:?} after {} asks): a person asks again by removing {} and restarting this unit",
                        record.outcome,
                        record.asks,
                        options.record.display()
                    ));
                    return Ok(());
                }
                thread::sleep(Duration::from_secs(30));
                continue;
            }
            wait_for_someone(options);
            match ask_and_wait(options, &bus) {
                None => thread::sleep(Duration::from_secs(60)),
                Some(false) => {
                    record.asks += 1;
                    record.asked_at_unix_ms = Some(unix_ms());
                    finish(options, &mut record, Outcome::NoAnswer, None);
                }
                Some(true) => {
                    record.asks += 1;
                    record.asked_at_unix_ms = Some(unix_ms());
                    say_going(options);
                    break;
                }
            }
        }
        if let Err(error) = systemctl("stop", "lamp-body.service") {
            finish(
                options,
                &mut record,
                Outcome::Failed,
                Some(error.to_string()),
            );
            return Ok(());
        }
        let (outcome, why) = match stand(options, &bus, &mut record) {
            Ok(outcome) => (outcome, None),
            Err(error) => {
                say(&format!("the stand failed: {error}; the lamp stays seated"));
                (Outcome::Failed, Some(error.to_string()))
            }
        };
        finish(options, &mut record, outcome, why);
        if let Err(error) = systemctl("start", "lamp-body.service") {
            say(&format!("could not start lamp-body again: {error}"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_touch_held_a_second_after_a_release_is_the_yes() {
        let mut touches = Touches::new(HOLD_US);
        // Touched when the ask began: not a yes, however long.
        assert!(!touches.observe(Some(true), 0));
        assert!(!touches.observe(Some(true), 5_000_000));
        // Let go, touched again: a yes once held a second.
        assert!(!touches.observe(Some(false), 5_100_000));
        assert!(!touches.observe(Some(true), 6_000_000));
        assert!(!touches.observe(Some(true), 6_900_000));
        assert!(touches.observe(Some(true), 7_000_000));
    }

    #[test]
    fn a_brush_is_not_the_yes_and_an_unknown_state_breaks_a_hold() {
        let mut touches = Touches::new(HOLD_US);
        assert!(!touches.observe(Some(false), 0));
        assert!(!touches.observe(Some(true), 100_000));
        assert!(!touches.observe(Some(false), 400_000));
        assert!(!touches.observe(Some(true), 500_000));
        assert!(!touches.observe(None, 1_000_000));
        assert!(!touches.observe(Some(true), 1_600_000));
        assert!(!touches.observe(Some(true), 2_500_000));
        assert!(touches.observe(Some(true), 2_600_000));
    }

    #[test]
    fn the_stop_is_any_new_touch_after_the_yes_is_let_go() {
        let mut stop = Touches::new(0);
        // The yes still held as the stand begins.
        assert!(!stop.observe(Some(true), 0));
        assert!(!stop.observe(Some(false), 200_000));
        assert!(stop.observe(Some(true), 300_000));
    }

    #[test]
    fn a_short_yes_is_a_yes_and_anything_that_takes_it_back_is_not() {
        for yes in [
            "Yes",
            "yes.",
            "Yeah, go ahead",
            "Sure!",
            "OK, go",
            "okay, I'm ready",
            "Ready",
            "Yep",
            "Yes, do it",
            "Okay, let's go",
            "Yes please",
            "Go",
        ] {
            assert!(is_yes(yes), "{yes}");
        }
        for no in [
            "",
            "No",
            "not yet",
            "yes but wait",
            "Yes, stop",
            "don't",
            "I said yes to my friend about the movie",
            "maybe",
            "the lamp says yes",
            "yes later",
        ] {
            assert!(!is_yes(no), "{no}");
        }
    }

    #[test]
    fn a_put_off_is_no_yes() {
        // El's on #2893, and their kin.
        for no in [
            "go away",
            "okay hold on",
            "yeah one sec",
            "okay never mind",
            "sure, in a minute",
            "yeah hang on",
            "ok just a moment",
            "yes or no",
            "okay after lunch",
            // El's on #3093: none holds a word that takes it back, and
            // each is no yes.
            "Yes?",
            "Yeah, what?",
            "Okay, what was that?",
            "Go back to sleep",
            "Go to sleep",
            "Okay, bye",
            "Ok, cancel",
            "Yeah, nah",
            "Okay, nope",
            "Okay, stay there",
            "Okay, sit down",
            "Yeah, I'm on the phone",
            "Okay, shut up",
            // Cut off inside a phrase, or only "I heard you" (El on #3093).
            "okay I'm",
            "yeah do",
            "ok it",
            "okay let's",
            "yes ahead",
            "okay",
            "OK.",
            "ok, please",
        ] {
            assert!(!is_yes(no), "{no}");
        }
    }

    /// A window opened at 10 s on the voice's clock: lines begun from
    /// 11.5 s on are the person's.
    fn opened(touch_only: bool) -> SpokenYes {
        let mut window = SpokenYes::new(touch_only);
        window.open(10_000_000);
        window
    }
    const AFTER: u64 = 10_000_000 + ECHO_TAIL_US;

    /// The first builder's refusals (#2602, 12:0x): the lamp's own words, a
    /// yes with nobody near, "yes… wait", a yes after another voice, a yes
    /// in a busy room; and a plain yes with someone near is taken.
    #[test]
    fn a_spoken_yes_is_taken_only_after_the_ask_alone_and_with_someone_near() {
        // The lamp's own "say yes", heard before the ask ended.
        let mut window = SpokenYes::new(false);
        assert_eq!(window.line("yes", 1, true, false, true), Heard::Nothing);
        window.open(10_000_000);
        assert_eq!(
            window.line("yes", AFTER, false, false, true),
            Heard::Nothing,
            "unfinished"
        );
        assert_eq!(
            window.line("", AFTER, true, false, false),
            Heard::NotTaken("nobody is seen near")
        );
        assert_eq!(
            window.line("yes, wait", AFTER, true, false, true),
            Heard::Nothing
        );
        // After "yes, wait", another line was heard: only the touch now.
        assert_eq!(
            window.line("yes", AFTER, true, false, true),
            Heard::NotTaken("another voice was heard after the ask")
        );

        let mut busy = opened(false);
        assert_eq!(
            busy.line("yes", AFTER, true, true, true),
            Heard::NotTaken("the room is busy")
        );
        let mut asked_busy = opened(true);
        assert_eq!(asked_busy.asked(), ASK_TOUCH_ONLY);
        assert_eq!(
            asked_busy.line("yes", AFTER, true, false, true),
            Heard::NotTaken("the ask took the touch alone")
        );

        let mut plain = opened(false);
        assert_eq!(plain.asked(), ASK);
        assert_eq!(
            plain.line("Yes, go ahead", AFTER, true, false, true),
            Heard::Yes
        );

        // After the window: nothing counts.
        let mut late = opened(false);
        late.close();
        assert_eq!(late.line("yes", AFTER, true, false, true), Heard::Nothing);
    }

    /// The Reviewer-lead's P1 on #2893: the line's pieces are joined, and
    /// the whole line is judged.
    #[test]
    fn the_joined_line_is_judged_not_each_piece() {
        // "No, not now… okay": the last piece alone would be a yes.
        let mut window = opened(false);
        assert_eq!(
            window.line("No, not now…", AFTER, false, false, true),
            Heard::Nothing
        );
        assert_eq!(
            window.line(" okay", AFTER + 1, true, false, true),
            Heard::Nothing
        );
        assert_eq!(
            window.line("yes", AFTER + 2, true, false, true),
            Heard::NotTaken("another voice was heard after the ask")
        );
        // A yes whose closing piece is empty is still the yes.
        let mut window = opened(false);
        assert_eq!(
            window.line("Yes", AFTER, false, false, true),
            Heard::Nothing
        );
        assert_eq!(window.line("", AFTER + 1, true, false, true), Heard::Yes);
        // A line of nothing is no other voice: a yes after it is taken.
        let mut window = opened(false);
        assert_eq!(window.line("", AFTER, true, false, true), Heard::Nothing);
        assert_eq!(window.line(" ", AFTER, true, false, true), Heard::Nothing);
        assert_eq!(window.line("yeah", AFTER, true, false, true), Heard::Yes);
    }

    /// The Reviewer-lead's and El's P2s on #2893: the lamp's own words never
    /// stand it, however late they are read.
    #[test]
    fn the_lamps_own_words_are_never_its_yes() {
        // Heard in the echo's tail, read after the window opened.
        let mut window = opened(false);
        assert_eq!(
            window.line("yes", AFTER - 1, true, false, true),
            Heard::Nothing
        );
        // Begun while the lamp spoke, finished after it.
        assert_eq!(
            window.line("yes", 9_000_000, false, false, true),
            Heard::Nothing
        );
        assert_eq!(window.line("", AFTER, true, false, true), Heard::Nothing);
        // Neither counts as another voice: the person's yes is taken.
        assert_eq!(window.line("yes", AFTER, true, false, true), Heard::Yes);
        // The ask heard back late is no other voice either.
        let mut window = opened(false);
        assert_eq!(
            window.line("When you're ready, say yes", AFTER, true, false, true),
            Heard::Nothing
        );
        assert_eq!(window.line("yes", AFTER, true, false, true), Heard::Yes);
        // Whatever else the lamp says in the window moves the tail on.
        let mut window = opened(false);
        window.lamp_said(AFTER + 5_000_000);
        assert_eq!(
            window.line(
                "yes",
                AFTER + 5_000_000 + ECHO_TAIL_US - 1,
                true,
                false,
                true
            ),
            Heard::Nothing
        );
        assert_eq!(
            window.line("yes", AFTER + 5_000_000 + ECHO_TAIL_US, true, false, true),
            Heard::Yes
        );
        // Said before the ask ended, it changes nothing.
        let mut window = opened(false);
        window.lamp_said(1);
        assert_eq!(window.line("yes", AFTER, true, false, true), Heard::Yes);
        assert!(is_echo("ready, say", ASK));
        assert!(!is_echo("yes", ASK));
        assert!(!is_echo("yes, go ahead", ASK));
    }

    /// The first builder on #2893 (12:1x): the ask ends on words that aren't
    /// a yes, so no tail of it is one. (A busy room's ask takes no spoken
    /// yes at all.)
    #[test]
    fn no_tail_of_the_ask_is_a_yes() {
        let words: Vec<&str> = ASK.split_whitespace().collect();
        for from in 0..words.len() {
            let tail = words[from..].join(" ");
            assert!(!is_yes(&tail), "{tail}");
        }
    }

    /// El's P1 on #2893: the subscriber's name is one the bus takes, and a
    /// line the voice publishes reaches it.
    #[test]
    fn what_the_voice_hears_is_subscribed_by_this_name() {
        let root = PathBuf::from(format!("/tmp/lb-fs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bus = lamp_bus::Bus::new(&root).unwrap();
        let mut heard = bus
            .subscribe_typed(lamp_msgs::Speech::TOPICS[0], SKILL, lamp_msgs::Speech::TYPE)
            .unwrap();
        let mut voice = bus
            .publisher_declared(
                lamp_msgs::Speech::TOPICS[0],
                &lamp_msgs::Speech::declaration(),
                "voice",
            )
            .unwrap();
        let line = lamp_msgs::Speech {
            text: "yes".into(),
            finished: true,
            locale: None,
            request: None,
        };
        voice.publish_json(42, &line).unwrap();
        let message = heard
            .recv(Duration::from_secs(5))
            .unwrap()
            .expect("the line");
        assert_eq!(message.header.at_us, 42);
        assert_eq!(message.parse::<lamp_msgs::Speech>().unwrap().text, "yes");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_spoken_yes_is_turned_off_only_by_off() {
        assert!(spoken_is_off(Some("off")));
        assert!(spoken_is_off(Some(" off\n")));
        assert!(!spoken_is_off(None));
        assert!(!spoken_is_off(Some("on")));
        assert!(!spoken_is_off(Some("")));
    }

    /// Fourth's finding on #4710: an owner's language other than English
    /// asks for the touch alone and takes no spoken yes; English is as it
    /// was.
    #[test]
    fn another_language_takes_the_touch_alone() {
        let touch_alone = takes_touch_alone(false, None, Some("ja\n"));
        assert!(touch_alone);
        let mut window = opened(touch_alone);
        assert_eq!(window.asked(), ASK_TOUCH_ONLY);
        assert_eq!(
            window.line("yes", AFTER, true, false, true),
            Heard::NotTaken("the ask took the touch alone")
        );
        for language in ["vi", "zh", " ko "] {
            assert!(takes_touch_alone(false, None, Some(language)), "{language}");
        }

        for language in [None, Some(""), Some("en\n"), Some("fr")] {
            let touch_alone = takes_touch_alone(false, None, language);
            assert!(!touch_alone, "{language:?}");
            let mut window = opened(touch_alone);
            assert_eq!(window.asked(), ASK);
            assert_eq!(window.line("yes", AFTER, true, false, true), Heard::Yes);
        }
        assert!(takes_touch_alone(true, None, Some("en")));
        assert!(takes_touch_alone(false, Some("off"), None));
    }

    #[test]
    fn a_busy_rooms_ask_names_the_touch_alone() {
        assert!(ASK.contains("say yes"));
        assert!(!ASK_TOUCH_ONLY.contains("yes"));
        assert!(ASK_TOUCH_ONLY.contains("Rest your hand on my head"));
    }

    #[test]
    fn an_armed_stop_takes_the_first_touch() {
        let mut stop = Touches::armed(0);
        assert!(!stop.observe(Some(false), 0));
        assert!(stop.observe(Some(true), 100_000));
        assert!(Touches::armed(0).observe(Some(true), 0));
    }

    /// Jarvis on #2541: a body asked to stop is waited for while it folds,
    /// never killed 2 s in. A stand-in that takes 3 s to end on SIGTERM.
    #[cfg(unix)]
    #[test]
    fn a_stopped_body_is_waited_for_while_it_folds() {
        let child = std::process::Command::new("/bin/sh")
            .args([
                "-c",
                "trap 'sleep 3; exit 7' TERM; while :; do sleep 0.1; done",
            ])
            .spawn()
            .unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let mut run = HandRun(child);
        let asked = std::time::Instant::now();
        assert!(
            run.stop_within(Duration::from_secs(10)),
            "it ended by itself"
        );
        assert!(asked.elapsed() >= Duration::from_millis(2_900));
        assert_eq!(run.0.wait().unwrap().code(), Some(7));
    }

    #[cfg(unix)]
    #[test]
    fn a_body_that_never_ends_is_killed_past_its_bound() {
        let child = std::process::Command::new("/bin/sh")
            .args(["-c", "trap '' TERM; while :; do sleep 0.1; done"])
            .spawn()
            .unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let mut run = HandRun(child);
        assert!(!run.stop_within(Duration::from_millis(500)));
    }

    #[test]
    fn controls_unknown_for_a_second_while_it_moves_stop_it() {
        let mut lost = ControlsLost::default();
        assert!(!lost.observe(None, 0));
        assert!(!lost.observe(Some(false), 500_000));
        assert!(!lost.observe(None, 600_000));
        assert!(!lost.observe(None, 1_599_999));
        assert!(lost.observe(None, 1_600_000));
    }

    #[test]
    fn controls_not_ready_are_not_known() {
        let state: lamp_msgs::ControlsState = serde_json::from_value(serde_json::json!({
            "stamp": {"clock_id": "c", "at_us": 1, "sequence": 1},
            "session_id": "s", "enabled": false, "ready": false, "buttons": [],
            "head_active": true, "base_ready": false, "base_tier": "none",
            "event_sequence": 0, "rejected_inputs": 0
        }))
        .unwrap();
        assert_eq!(touched(&state), None);
        let ready = lamp_msgs::ControlsState {
            ready: true,
            ..state
        };
        assert_eq!(touched(&ready), Some(true));
    }

    #[test]
    fn near_is_a_fresh_face_big_enough() {
        assert!(near("0.245\n", 100_000));
        assert!(!near("0.05\n", 100_000));
        assert!(!near("0.245\n", PRESENCE_FRESH_US + 1));
        assert!(!near("", 0));
    }

    #[test]
    fn the_stand_is_read_from_the_last_full_stand_line() {
        let log = "\
lamp-body: stand 0.50 arm goals [2400, 2400] positions [2300, 2310]
lamp-body: stand 1.00 arm goals [2633, 2603] positions [2630, 2601]
lamp-body: stand 0.97 arm goals [2633, 2603] positions [2629, 2600]
lamp-body: stand 0.80 arm goals [2500, 2500] positions [2490, 2495]
";
        assert_eq!(stood_at(log), Some((2629, 2600)));
        assert_eq!(
            stood_at("lamp-body: stand 0.90 arm goals [1, 2] positions [3, 4]"),
            None
        );
    }

    #[test]
    fn the_confirmed_file_has_this_arms_stand_and_says_so() {
        let read = "# home, folded-arm: found at rest. stand: lamp-4ace's with this lamp's roll and yaw. pitch-window: the default, checked.\nBODY_POSES=\"--home 1916,2049,2041 --folded-arm 2046,2046 --stand 1772,2049,2041,2633,2603 --pitch-window 1740,2030 --torque on\"\n";
        let confirmed = confirmed_lamp_env(read, 2629, 2600).unwrap();
        assert!(confirmed.contains("--stand 1772,2049,2041,2629,2600 --pitch-window"));
        assert!(
            confirmed.contains("confirmed by a touch, with this lamp's roll and yaw. pitch-window")
        );
        assert_eq!(
            confirmed_lamp_env("BODY_POSES=\"--home 1,2,3\"\n", 1, 2),
            None
        );
    }

    #[test]
    fn the_lamp_asks_twice_at_most_ten_minutes_apart_and_never_after_a_stand() {
        let mut record = Record::default();
        assert!(record.may_ask(0));
        record.asks = 1;
        record.asked_at_unix_ms = Some(1_000);
        record.outcome = Some(Outcome::NoAnswer);
        assert!(!record.may_ask(1_000 + 599_999));
        assert!(record.may_ask(1_000 + 600_000));
        record.asks = 2;
        assert!(!record.may_ask(u64::MAX));
        let stood = Record {
            outcome: Some(Outcome::Stood),
            ..Record::default()
        };
        assert!(!stood.may_ask(0));
        let failed = Record {
            asks: 1,
            outcome: Some(Outcome::Failed),
            ..Record::default()
        };
        assert!(failed.may_ask(0));
    }

    #[test]
    fn the_record_says_its_outcome_in_words() {
        let record = Record {
            asks: 1,
            outcome: Some(Outcome::Stood),
            at_unix_ms: Some(5),
            shoulder: Some(2629),
            elbow: Some(2600),
            ..Record::default()
        };
        let text = serde_json::to_string(&record).unwrap();
        assert!(text.contains(r#""outcome":"stood""#), "{text}");
        assert_eq!(serde_json::from_str::<Record>(&text).unwrap(), record);
    }
}
