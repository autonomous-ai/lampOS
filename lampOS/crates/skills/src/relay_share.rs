//! Skills' runs never take the voice's share of the lamp's day (#7342, the
//! follow-up to #7151's 20 runs a skill and 100 in all). Off unless
//! [`SWITCH`] is set.
//!
//! Each `do.task` run is an agent session whose chats go through
//! lamp-cloud's model forwarder (`lamp_cloud::hermes_llm`, `127.0.0.1:7741`)
//! on the voice's key, so they spend the same daily allowance the voice
//! talks on. The relay says how much of that day is used on each HTTP answer
//! of a metered route: [`HEADER`], a number from 0 to 1, counted the way the
//! relay counts (cached tokens cheaper), absent when the lamp has no day
//! limit (Backend on #7347, live in production since 17 Nov).
//!
//! - **The reading** ([`Reading`]): the forwarder keeps the last value it
//!   saw in [`FILE`] in its state folder ([`DIRECTORY`], `lampos`'s, the
//!   user lamp-skills runs as too), with the UTC day it was read on (the
//!   day the allowance is counted in) and, for measuring a run, the day's
//!   count of the agent's chat answers and their tokens, as the relay's
//!   `usage` gives them. Numbers only: no word of a chat.
//! - **The bound** ([`refuses`]): a skill's run is refused before it is
//!   handed over (`task::would_refuse`, `task::begin`) once what is left of
//!   the day, `1 - used`, is at or below the voice's share: [`DEFAULT_SHARE`]
//!   with the switch `on`, or the share it gives (`0.3`). The share is of the
//!   relay's own count, never of a token number guessed here. The words are
//!   plain, for the voice to say when the owner asked for the run.
//! - **What it never does:** refuse the voice. The voice talks to the relay
//!   itself, and the forwarder only reads the header; it forwards every
//!   request as before, the owner's own agent tasks included.
//! - **Not known is let through:** no reading, one of another UTC day, a
//!   chat answer with no header (not limited), or a file that can't be read.
//!   The relay's own limit still holds, and a broken reading must not stop
//!   every skill for good.
//! - **How fresh:** the reading is the forwarder's last answer's. The voice's
//!   own use since then is in the relay's count but not yet in the reading,
//!   so a run may start a little past the mark; its first chat brings the
//!   reading up to date and the next run is refused.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    io::{self, Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

/// `on` (the voice keeps [`DEFAULT_SHARE`] of the day) or a share of its
/// own, a decimal between 0 and 1 (`0.3`); anything else, or unset, off.
/// Read by lamp-skills (the bound) and lamp-cloud (the reading): set it
/// alike in both.
pub const SWITCH: &str = "LAMP_SKILL_RUNS_VOICE_SHARE";
/// The voice's share with the switch `on`: half the day. The lead's plan
/// covers about two hours of one-to-one talk a day (#7347, 21:54), and the
/// first builder's mark for a busy desk is under half the allowance.
pub const DEFAULT_SHARE: f64 = 0.5;
/// lamp-cloud's state folder, where the forwarder keeps [`FILE`].
pub const DIRECTORY: &str = "/var/lib/lampos-cloud";
/// The reading.
pub const FILE: &str = "day-used.json";
/// The relay's header: how much of the lamp's day is used, 0 to 1.
pub const HEADER: &str = "x-usage-day-used";
/// The most bytes of [`FILE`] read.
const MOST_BYTES: u64 = 4096;
const DAY_MS: u64 = 86_400_000;

#[cfg(test)]
thread_local! {
    /// The share as a test of this thread sets it (`Some(None)`: off).
    static SHARE_HERE: std::cell::Cell<Option<Option<f64>>> = const { std::cell::Cell::new(None) };
    /// The folder [`FILE`] is read from in a test of this thread.
    static DIRECTORY_HERE: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// The voice's share from the switch's value, or `None` (off).
pub fn share_from(value: Option<&str>) -> Option<f64> {
    let value = value?.trim();
    if value == "on" {
        return Some(DEFAULT_SHARE);
    }
    value
        .parse::<f64>()
        .ok()
        .filter(|share| share.is_finite() && *share > 0.0 && *share < 1.0)
}

/// The voice's share in this process, or `None` with the switch off.
pub fn share() -> Option<f64> {
    #[cfg(test)]
    if let Some(share) = SHARE_HERE.with(std::cell::Cell::get) {
        return share;
    }
    share_from(std::env::var(SWITCH).ok().as_deref())
}

/// Whether the switch is on in this process.
pub fn on() -> bool {
    share().is_some()
}

/// The share and the reading's folder for this test's thread alone.
#[cfg(test)]
pub fn set_here(share: Option<f64>, directory: Option<&Path>) {
    SHARE_HERE.with(|here| here.set(Some(share)));
    DIRECTORY_HERE.with(|here| *here.borrow_mut() = directory.map(Path::to_path_buf));
}

fn directory() -> PathBuf {
    #[cfg(test)]
    if let Some(directory) = DIRECTORY_HERE.with(|here| here.borrow().clone()) {
        return directory;
    }
    PathBuf::from(DIRECTORY)
}

/// The header's value as a share of the day used: a number from 0 to 1,
/// and one over 1 up to [`MOST_SAID`] as 1, the whole day, so a relay that
/// overshoots its limit is never left at an older, lower value (#7538's
/// P3, blue and Twelve). Anything else isn't read.
pub fn used_from(header: &str) -> Option<f64> {
    header
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|used| used.is_finite() && (0.0..=MOST_SAID).contains(used))
        .map(|used| used.min(1.0))
}

/// The most a header may say and still be read: twice the day.
const MOST_SAID: f64 = 2.0;

/// The UTC day of `ms` since 1970.
pub fn day_of(ms: u64) -> u64 {
    ms / DAY_MS
}

/// What the forwarder last read of the lamp's day, and its count of the
/// agent's chat answers on it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Reading {
    /// The UTC day, in days since 1970, all of this is for.
    pub day: u64,
    /// The relay's [`HEADER`] at the last answer that said: `None` when a
    /// chat answer came without one (no day limit) or none has said today.
    pub used: Option<f64>,
    /// When it was last read, milliseconds since 1970.
    pub at_ms: u64,
    /// The agent's chat answers forwarded today.
    pub answers: u64,
    /// Of them, those whose answer gave no `usage`.
    pub uncounted: u64,
    /// Their tokens asked with.
    pub prompt_tokens: u64,
    /// Of those, the ones the relay's cache held.
    pub cached_tokens: u64,
    /// Their tokens answered with.
    pub completion_tokens: u64,
}

impl Reading {
    /// Turned over to the UTC day of `now_ms`: a later (or earlier) day
    /// begins from nothing.
    pub fn roll(&mut self, now_ms: u64) {
        let day = day_of(now_ms);
        if self.day != day {
            *self = Reading {
                day,
                ..Reading::default()
            };
        }
    }

    /// An answer's [`HEADER`] at `now_ms`: `Some(value)` when it carried
    /// one (one [`used_from`] doesn't read is not kept), `None` when not.
    /// Only an answer that `clears` (a successful one on the metered chat
    /// route) says, with none, that the lamp has no day limit: another
    /// route's may carry no header at all (#7538's P2).
    pub fn heard(&mut self, now_ms: u64, header: Option<&str>, clears: bool) {
        self.roll(now_ms);
        match header {
            Some(header) => {
                if let Some(used) = used_from(header) {
                    self.used = Some(used);
                    self.at_ms = now_ms;
                }
            }
            None if clears => {
                self.used = None;
                self.at_ms = now_ms;
            }
            None => {}
        }
    }

    /// One chat answer counted, with its `usage` if it gave one.
    pub fn counted(&mut self, usage: Option<&Value>) {
        self.answers = self.answers.saturating_add(1);
        let count = |pointer: &str| usage.and_then(|usage| usage.pointer(pointer)?.as_u64());
        let (Some(prompt), Some(completion)) =
            (count("/prompt_tokens"), count("/completion_tokens"))
        else {
            self.uncounted = self.uncounted.saturating_add(1);
            return;
        };
        let cached = count("/prompt_cache_hit_tokens")
            .or_else(|| count("/prompt_tokens_details/cached_tokens"))
            .unwrap_or(0);
        self.prompt_tokens = self.prompt_tokens.saturating_add(prompt);
        self.cached_tokens = self.cached_tokens.saturating_add(cached);
        self.completion_tokens = self.completion_tokens.saturating_add(completion);
    }
}

/// The reading in `dir`'s [`FILE`], or `None`: no file, one too large or
/// one that isn't a reading.
pub fn read(dir: &Path) -> Option<Reading> {
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(dir.join(FILE))
        .ok()?;
    let mut bytes = Vec::new();
    file.take(MOST_BYTES + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > MOST_BYTES {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

/// The reading in this process's folder ([`DIRECTORY`]).
pub fn read_here() -> Option<Reading> {
    read(&directory())
}

/// `reading` kept whole in `dir`'s [`FILE`], 0600: written beside it and
/// moved over it, never through a link. Not synced: one lost to a power cut
/// is read again at the next answer.
pub fn keep(dir: &Path, reading: &Reading) -> io::Result<()> {
    let path = dir.join(FILE);
    let beside = dir.join(format!(".{FILE}.new"));
    match fs::remove_file(&beside) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    let json = serde_json::to_vec(reading).map_err(io::Error::other)?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(libc::O_NOFOLLOW)
        .mode(0o600)
        .open(&beside)?;
    file.write_all(&json)?;
    drop(file);
    fs::rename(&beside, &path)
}

/// Why a skill's run is refused at `now_ms` with the voice keeping `share`
/// of the day, by `reading`, in words the lamp can say; `None` when the
/// day's rest is above the share or isn't known.
pub fn refuses(reading: Option<&Reading>, share: f64, now_ms: u64) -> Option<String> {
    let reading = reading?;
    if reading.day != day_of(now_ms) {
        return None;
    }
    let used = reading.used?;
    // At or below the share: `1 - used <= share`, kept clear of a float's
    // last bit so exactly the share is refused.
    if 1.0 - used > share + 1e-9 {
        return None;
    }
    Some(format!(
        "the lamp has used {}% of its thinking for today, and the last {}% is kept \
         for talking, so skills' runs wait until its day starts again",
        percent(used),
        percent(share)
    ))
}

fn percent(share: f64) -> u64 {
    (share * 100.0).round().clamp(0.0, 100.0) as u64
}

/// [`refuses`] in this process: the switch's share and the reading in
/// [`DIRECTORY`]; `None` with the switch off, whatever the reading.
pub fn would_refuse(now_ms: u64) -> Option<String> {
    let share = share()?;
    refuses(read_here().as_ref(), share, now_ms)
}

/// One run's line for the journal, from the reading `before` it was handed
/// over and `after` it ended: the day used at each, and the agent's chat
/// answers and tokens between them. Every agent chat in that time is in
/// it, an owner's agent task run beside it too, and the day used counts
/// the voice's use as well; the header is read as each chat is asked, so
/// the run's last chat is not in the day used after it.
pub fn run_line(before: Option<&Reading>, after: Option<&Reading>) -> String {
    let used = |reading: Option<&Reading>| match reading {
        None => "no reading".to_owned(),
        Some(Reading { used: None, .. }) => "not limited".to_owned(),
        Some(Reading {
            used: Some(used), ..
        }) => format!("{used:.3}"),
    };
    let mut line = format!(
        "the relay's day used {} before its run and {} after",
        used(before),
        used(after)
    );
    if let Some(after) = after {
        let zero = Reading::default();
        // A run across a UTC midnight is counted from the new day's start.
        let before = before
            .filter(|before| before.day == after.day)
            .unwrap_or(&zero);
        let less = |a: u64, b: u64| a.saturating_sub(b);
        line.push_str(&format!(
            "; the agent's chats meanwhile: {} answers ({} gave no count), {} prompt tokens \
             ({} from the cache), {} completion tokens",
            less(after.answers, before.answers),
            less(after.uncounted, before.uncounted),
            less(after.prompt_tokens, before.prompt_tokens),
            less(after.cached_tokens, before.cached_tokens),
            less(after.completion_tokens, before.completion_tokens),
        ));
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 20_774 * DAY_MS + 3_600_000;

    fn reading(used: Option<f64>) -> Reading {
        Reading {
            day: day_of(NOW),
            used,
            at_ms: NOW - 1000,
            ..Reading::default()
        }
    }

    #[test]
    fn the_switch_is_on_or_a_share_between_nothing_and_all() {
        assert_eq!(share_from(None), None);
        assert_eq!(share_from(Some("on")), Some(DEFAULT_SHARE));
        assert_eq!(share_from(Some("0.3")), Some(0.3));
        for off in ["", "off", "yes", "0", "1", "1.5", "-0.2", "NaN", "inf"] {
            assert_eq!(share_from(Some(off)), None, "{off}");
        }
    }

    #[test]
    fn a_run_is_refused_once_the_rest_is_the_voices_share() {
        assert_eq!(refuses(Some(&reading(Some(0.499))), 0.5, NOW), None);
        let refused = refuses(Some(&reading(Some(0.5))), 0.5, NOW).expect("at the share");
        assert!(
            refused.contains("used 50% of its thinking for today"),
            "{refused}"
        );
        assert!(
            refused.contains("the last 50% is kept for talking"),
            "{refused}"
        );
        assert!(refuses(Some(&reading(Some(0.7))), 0.5, NOW).is_some());
        assert!(refuses(Some(&reading(Some(1.0))), 0.5, NOW).is_some());
        // The share is the switch's: 0.3 keeps the last 30%.
        assert_eq!(refuses(Some(&reading(Some(0.69))), 0.3, NOW), None);
        assert!(refuses(Some(&reading(Some(0.7))), 0.3, NOW).is_some());
    }

    /// #7538's P3 (blue, Twelve): a relay that says a little over the whole
    /// day, on an overshoot, is at its limit, never left at an older, lower
    /// value; past twice the day, or not a number, it isn't read.
    #[test]
    fn a_little_over_the_whole_day_is_the_whole_day() {
        assert_eq!(used_from("1.02"), Some(1.0));
        assert_eq!(used_from("2"), Some(1.0));
        let mut kept = reading(Some(0.4));
        kept.heard(NOW, Some("1.020"), true);
        assert_eq!(kept.used, Some(1.0));
        assert!(refuses(Some(&kept), 0.5, NOW).is_some());
        for unread in ["2.01", "-0.1", "NaN", "inf", "0.9x", ""] {
            assert_eq!(used_from(unread), None, "{unread}");
        }
    }

    #[test]
    fn what_is_not_known_lets_a_run_through() {
        assert_eq!(refuses(None, 0.5, NOW), None);
        assert_eq!(refuses(Some(&reading(None)), 0.5, NOW), None);
        let mut yesterday = reading(Some(0.95));
        yesterday.day -= 1;
        assert_eq!(refuses(Some(&yesterday), 0.5, NOW), None);
    }

    #[test]
    fn the_header_is_read_and_a_new_day_begins_from_nothing() {
        let mut kept = Reading::default();
        kept.heard(NOW, Some("0.412"), true);
        assert_eq!(
            (kept.day, kept.used, kept.at_ms),
            (day_of(NOW), Some(0.412), NOW)
        );
        // Past twice the day is not kept, nor is a refusal without the header.
        kept.heard(NOW + 1, Some("2.5"), true);
        kept.heard(NOW + 2, None, false);
        assert_eq!((kept.used, kept.at_ms), (Some(0.412), NOW));
        kept.counted(Some(&serde_json::json!({
            "prompt_tokens": 9000, "completion_tokens": 120, "prompt_cache_hit_tokens": 8000
        })));
        kept.counted(Some(&serde_json::json!({
            "prompt_tokens": 100, "completion_tokens": 5,
            "prompt_tokens_details": {"cached_tokens": 64}
        })));
        kept.counted(None);
        assert_eq!(
            (
                kept.answers,
                kept.uncounted,
                kept.prompt_tokens,
                kept.cached_tokens
            ),
            (3, 1, 9100, 8064)
        );
        assert_eq!(kept.completion_tokens, 125);
        // A successful answer with no header: no day limit.
        kept.heard(NOW + 3, None, true);
        assert_eq!(kept.used, None);
        kept.heard(NOW + DAY_MS, Some("0.010"), true);
        assert_eq!(
            kept,
            Reading {
                day: day_of(NOW) + 1,
                used: Some(0.01),
                at_ms: NOW + DAY_MS,
                ..Reading::default()
            }
        );
    }

    #[test]
    fn a_reading_is_kept_whole_and_read_back() {
        let dir = std::env::temp_dir().join(format!("relay-share-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        assert_eq!(read(&dir), None);
        let mut kept = reading(Some(0.25));
        kept.counted(None);
        keep(&dir, &kept).unwrap();
        assert_eq!(read(&dir), Some(kept.clone()));
        let mode = fs::metadata(dir.join(FILE)).unwrap().permissions();
        assert_eq!(
            std::os::unix::fs::PermissionsExt::mode(&mode) & 0o777,
            0o600
        );
        fs::write(dir.join(FILE), b"not a reading").unwrap();
        assert_eq!(read(&dir), None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn with_the_switch_off_nothing_is_refused() {
        let dir = std::env::temp_dir().join(format!("relay-share-off-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        keep(&dir, &reading(Some(0.99))).unwrap();
        set_here(None, Some(&dir));
        assert_eq!(would_refuse(NOW), None);
        set_here(Some(0.5), Some(&dir));
        assert!(would_refuse(NOW).is_some());
        set_here(None, None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_runs_line_gives_the_day_used_and_its_tokens() {
        let mut before = reading(Some(0.412));
        before.counted(Some(
            &serde_json::json!({"prompt_tokens": 10, "completion_tokens": 1}),
        ));
        let mut after = before.clone();
        after.used = Some(0.431);
        after.counted(Some(&serde_json::json!({
            "prompt_tokens": 9000, "completion_tokens": 120, "prompt_cache_hit_tokens": 8000
        })));
        after.counted(None);
        assert_eq!(
            run_line(Some(&before), Some(&after)),
            "the relay's day used 0.412 before its run and 0.431 after; the agent's chats \
             meanwhile: 2 answers (1 gave no count), 9000 prompt tokens (8000 from the cache), \
             120 completion tokens"
        );
        assert_eq!(
            run_line(None, None),
            "the relay's day used no reading before its run and no reading after"
        );
    }
}
