//! How loud this home's person is when they call the lamp from their own
//! chair (#753): the measurement setup takes, and the file it keeps, for the
//! voice's busy-near rule (lamp-voice `attention::NEAR_RISE_DB`).
//!
//! That rule lets a call out in a busy room with no face seen when the name
//! is heard at [`SCORE`] or more while the room microphone (mic1) rose
//! [`DEFAULT_RISE_DB`] or more over its quiet. 18 dB came from one evening
//! at lamp-4ace: the owner at the desk rose 19.5 to 26.5 dB, people farther
//! off 9.8 to 15.1 dB (`docs/ears.md`). Setup asks the person to say the
//! lamp's name a few times; each answer is one [`near_try`], and
//! [`near_rise`] turns three into this home's bar.
//!
//! The name may also be heard by speech to text (#3923): a line whose words
//! were the lamp's name alone, which the on-device detector missed on some
//! voices. Such a call counts as a detector's would, its rise read the same
//! way; the detector's own bar is not lowered for it.
//!
//! Only numbers are measured and kept, never sound: the rise in decibels
//! and how surely the name was heard. Nothing here is voice identity, and
//! the voice microphone (mic2) stays out: its processing evens out loudness
//! and it runs on its own clock.
//!
//! Every time is milliseconds since the Unix epoch, as the bus's headers
//! give them.
use serde::{Deserialize, Serialize};
use std::fmt;
use std::io::{self, Write};
use std::path::Path;

/// Where setup keeps the result and the voice reads it.
pub const FILE: &str = "/var/lib/lampOS/voice/near-call.json";
/// A name heard at least this surely: the busy-near rule's own bar
/// (lamp-voice `attention::NEAR_SCORE`).
pub const SCORE: f32 = 0.95;
/// The rule's bar when nothing was measured, or the file is unfit.
pub const DEFAULT_RISE_DB: f32 = 18.0;
/// A try's window: from the end of the lamp's prompt to this after.
pub const WINDOW_MS: u64 = 6_000;
/// The rise is read from the hearing's start to this after.
pub const RISE_MS: u64 = 1_000;
/// The lamp speaking this soon before the hearing's start spoils the try:
/// the speaker's tail and the room's echo of its last word.
pub const ECHO_BEFORE_MS: u64 = 500;
/// A line heard by speech to text as the name, and a detector's hearing,
/// that began at most this far apart are one call: the detector's start
/// and the line's first speech block mark the same "Lamp". So is a hearing
/// begun inside the line, however far in (a slow "Hey, … Lamp", or a
/// cough before it that speech to text left out).
pub const ONE_CALL_MS: u64 = 1_000;
/// Tries accepted for a result.
pub const TRIES: usize = 3;
/// Prompts at most, accepted or not.
pub const MOST_PROMPTS: usize = 5;
/// The bar sits this far under the quietest accepted call, so that call
/// and ones a little softer get through.
pub const UNDER_QUIETEST_DB: f32 = 3.0;
/// The bar is never under this: people farther off reached 15.1 dB.
pub const LOWEST_DB: f32 = 16.0;
/// Nor over this: a loud talker's bar would leave a raised voice the only
/// way in.
pub const HIGHEST_DB: f32 = 24.0;
/// Accepted tries further apart than this are not one way of calling.
pub const MOST_SPREAD_DB: f32 = 10.0;

/// One hearing of the name (`name/heard`, lamp-name).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hearing {
    /// When the name began to be heard: the message's time less its
    /// `ago_ms`.
    pub began_ms: u64,
    /// The message's `score`.
    pub score: f32,
}

/// One line whose words, as speech to text heard them, were the lamp's name
/// alone (#3923, the second route's `name_alone`): no score, only when it
/// began. A sound with no words, or more words than the name, is never one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Said {
    /// When the line's speech began.
    pub began_ms: u64,
    /// When its speech ended.
    pub ended_ms: u64,
}

/// One reading of the room's level (`RoomSound`, the ears, every 100 ms).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Level {
    /// The message's time.
    pub at_ms: u64,
    /// The message's `level_db`.
    pub level_db: f32,
    /// Its `floor_db`, the room's quiet; none until the ears know it.
    pub floor_db: Option<f32>,
    /// Its `own_voice`: the lamp's own sound, never counted.
    pub own_voice: bool,
}

/// When the lamp spoke (`voice/state`): from `began_ms` to `ended_ms`, or
/// to now while it still speaks (`None`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spoke {
    pub began_ms: u64,
    pub ended_ms: Option<u64>,
}

/// How a counted call's name was heard.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeardBy {
    /// The on-device detector (`name/heard`) at [`SCORE`] or more. Files
    /// from before #3923 hold only these.
    #[default]
    Name,
    /// Speech to text, the line being the name alone ([`Said`]); the
    /// detector missed it, or heard it under [`SCORE`].
    Text,
}

impl HeardBy {
    pub fn name(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Text => "text",
        }
    }
}

/// A try that counts.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Try {
    /// mic1's largest rise over its quiet in the rise window, in dB, to
    /// one decimal.
    pub rise_db: f32,
    /// How surely the detector heard the name; none when speech to text
    /// heard it ([`HeardBy::Text`]), which gives no score.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
    #[serde(default)]
    pub heard_by: HeardBy,
}

impl Try {
    /// Whether the way it was heard is one [`near_try`] accepts: the
    /// detector at [`SCORE`] to 1, or speech to text with no score.
    pub fn heard_surely(&self) -> bool {
        match (self.heard_by, self.score) {
            (HeardBy::Name, Some(score)) => (SCORE..=1.0).contains(&score),
            (HeardBy::Text, None) => true,
            _ => false,
        }
    }
}

/// Why a try does not count. The person is asked again, up to
/// [`MOST_PROMPTS`] in all.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Rejected {
    /// The name wasn't heard in the window.
    NoHearing,
    /// It was heard more than once: which one was the call is unknown.
    TwoHearings,
    /// It was heard, under [`SCORE`].
    Unsure { score: f32 },
    /// The lamp spoke from [`ECHO_BEFORE_MS`] before the hearing to the end
    /// of the rise window: what mic1 heard may be its own voice.
    LampSpoke,
    /// No level with the room's quiet known in the rise window.
    NoLevel,
}

impl fmt::Display for Rejected {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoHearing => f.write_str("the name wasn't heard"),
            Self::TwoHearings => f.write_str("the name was heard more than once"),
            Self::Unsure { score } => write!(f, "the name was heard at {score:.2}, under {SCORE}"),
            Self::LampSpoke => f.write_str("the lamp was speaking"),
            Self::NoLevel => f.write_str("the room's level wasn't known"),
        }
    }
}

/// One try: the prompt ended at `prompt_ended_ms`; `hearings`, `levels`
/// and `spoke` are what the bus said from then to the window's end, or
/// more (anything outside is left aside).
pub fn near_try(
    prompt_ended_ms: u64,
    hearings: &[Hearing],
    levels: &[Level],
    spoke: &[Spoke],
) -> Result<Try, Rejected> {
    near_try_with_said(prompt_ended_ms, hearings, &[], levels, spoke)
}

/// [`near_try`] with the lines speech to text heard as the name alone
/// (`said`, #3923). Each is a call as a hearing is, in the same window, one
/// a try; one begun within [`ONE_CALL_MS`] of a hearing is that hearing's
/// own words, so the two are one call, and a hearing under [`SCORE`] that
/// they confirm counts as heard by text. A hearing under [`SCORE`] with no
/// such line is still unsure: the detector's bar is not lowered.
pub fn near_try_with_said(
    prompt_ended_ms: u64,
    hearings: &[Hearing],
    said: &[Said],
    levels: &[Level],
    spoke: &[Spoke],
) -> Result<Try, Rejected> {
    let window = prompt_ended_ms..=prompt_ended_ms.saturating_add(WINDOW_MS);
    let heard: Vec<Hearing> = hearings
        .iter()
        .filter(|hearing| window.contains(&hearing.began_ms))
        .copied()
        .collect();
    let said: Vec<Said> = said
        .iter()
        .filter(|line| window.contains(&line.began_ms))
        .copied()
        .collect();
    let one_call = |hearing: &Hearing, line: &Said| {
        hearing.began_ms.abs_diff(line.began_ms) <= ONE_CALL_MS
            || (line.began_ms..=line.ended_ms).contains(&hearing.began_ms)
    };
    // Lines no hearing explains are calls of their own.
    let alone: Vec<u64> = said
        .iter()
        .filter(|line| !heard.iter().any(|hearing| one_call(hearing, line)))
        .map(|line| line.began_ms)
        .collect();
    let (began_ms, score, heard_by) = match (heard.as_slice(), alone.as_slice()) {
        ([], []) => return Err(Rejected::NoHearing),
        ([hearing], []) if hearing.score >= SCORE => {
            (hearing.began_ms, Some(hearing.score), HeardBy::Name)
        }
        // Under the bar, or not a number: only its words make it a call.
        ([hearing], []) if said.iter().any(|line| one_call(hearing, line)) => {
            (hearing.began_ms, None, HeardBy::Text)
        }
        ([hearing], []) => {
            return Err(Rejected::Unsure {
                score: hearing.score,
            });
        }
        ([], [line]) => (*line, None, HeardBy::Text),
        _ => return Err(Rejected::TwoHearings),
    };
    let rise_end = began_ms.saturating_add(RISE_MS);
    let guarded = began_ms.saturating_sub(ECHO_BEFORE_MS);
    if spoke.iter().any(|spoke| {
        spoke.began_ms <= rise_end && spoke.ended_ms.is_none_or(|ended| ended >= guarded)
    }) {
        return Err(Rejected::LampSpoke);
    }
    levels
        .iter()
        .filter(|level| (began_ms..=rise_end).contains(&level.at_ms) && !level.own_voice)
        .filter_map(|level| level.floor_db.map(|floor| level.level_db - floor))
        .filter(|rise| rise.is_finite())
        .reduce(f32::max)
        .map(|rise| Try {
            rise_db: tenths(rise),
            score,
            heard_by,
        })
        .ok_or(Rejected::NoLevel)
}

/// Whether setup asks once more, after `prompts` prompts of which
/// `accepted` counted: until [`TRIES`] count, while [`MOST_PROMPTS`] can
/// still get there.
pub fn asks_again(prompts: usize, accepted: usize) -> bool {
    accepted < TRIES && prompts < MOST_PROMPTS && accepted + (MOST_PROMPTS - prompts) >= TRIES
}

/// This home's bar from the accepted `tries`: the quietest less
/// [`UNDER_QUIETEST_DB`], held to [`LOWEST_DB`]..=[`HIGHEST_DB`], to one
/// decimal. None, and the rule keeps [`DEFAULT_RISE_DB`], with fewer than
/// [`TRIES`], or when they spread over more than [`MOST_SPREAD_DB`].
pub fn near_rise(tries: &[Try]) -> Option<f32> {
    if tries.len() < TRIES || tries.iter().any(|done| !done.rise_db.is_finite()) {
        return None;
    }
    let quietest = tries.iter().map(|done| done.rise_db).reduce(f32::min)?;
    let loudest = tries.iter().map(|done| done.rise_db).reduce(f32::max)?;
    (loudest - quietest <= MOST_SPREAD_DB)
        .then(|| tenths((quietest - UNDER_QUIETEST_DB).clamp(LOWEST_DB, HIGHEST_DB)))
}

/// What [`FILE`] holds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Calibrated {
    /// The bar: [`near_rise`] of `tries`.
    pub near_rise_db: f32,
    /// The accepted tries it came from.
    pub tries: Vec<Try>,
    /// When it was measured.
    pub at_unix_s: u64,
}

impl Calibrated {
    /// The result of `tries` measured at `at_unix_s`; none when they make
    /// no bar ([`near_rise`]).
    pub fn of(tries: Vec<Try>, at_unix_s: u64) -> Option<Self> {
        near_rise(&tries).map(|near_rise_db| Self {
            near_rise_db,
            tries,
            at_unix_s,
        })
    }

    /// Whether its bar is the one its tries make, from tries that could have
    /// been accepted: each name heard at [`SCORE`] to 1, or by speech to
    /// text with no score ([`Try::heard_surely`]). A file written by hand,
    /// or by an older rule, is not used (Seventh on #1803: a score of 7 or
    /// not a number was).
    pub fn fits(&self) -> bool {
        self.tries.iter().all(Try::heard_surely)
            && near_rise(&self.tries).is_some_and(|bar| (bar - self.near_rise_db).abs() < 0.05)
    }
}

/// Keeps `calibrated` at `path`, readable by its owner only, whole or not
/// at all: written beside it, then moved over it.
pub fn write(path: &Path, calibrated: &Calibrated) -> io::Result<()> {
    let text = serde_json::to_vec(calibrated).map_err(io::Error::other)?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no file name"))?;
    let mut beside = name.to_os_string();
    beside.push(".new");
    let beside = path.with_file_name(beside);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&beside)?;
    file.write_all(&text)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&beside, path)
}

/// The bar kept at `path`, when there is one and it fits
/// ([`Calibrated::fits`]); none otherwise, and the rule keeps
/// [`DEFAULT_RISE_DB`].
pub fn read(path: &Path) -> Option<f32> {
    let calibrated: Calibrated = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    calibrated.fits().then_some(calibrated.near_rise_db)
}

fn tenths(db: f32) -> f32 {
    (db * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENDED: u64 = 1_000_000;

    fn heard(after_ms: u64, score: f32) -> Hearing {
        Hearing {
            began_ms: ENDED + after_ms,
            score,
        }
    }

    /// A level every 100 ms over `from_ms..to_ms` after the prompt, the
    /// room's quiet at 30 dB, rising `rise_db` over it at `peak_ms`.
    fn levels(from_ms: u64, to_ms: u64, peak_ms: u64, rise_db: f32) -> Vec<Level> {
        (from_ms..to_ms)
            .step_by(100)
            .map(|at| Level {
                at_ms: ENDED + at,
                level_db: 30.0 + if at == peak_ms { rise_db } else { 2.0 },
                floor_db: Some(30.0),
                own_voice: false,
            })
            .collect()
    }

    fn spoke(from_ms: u64, to_ms: Option<u64>) -> Spoke {
        Spoke {
            began_ms: ENDED + from_ms,
            ended_ms: to_ms.map(|to| ENDED + to),
        }
    }

    #[test]
    fn a_call_counts_with_mic1s_largest_rise_in_the_second_after_the_name_began() {
        let room = levels(0, 6_000, 2_400, 21.37);
        assert_eq!(
            near_try(ENDED, &[heard(2_000, 0.98)], &room, &[]),
            Ok(Try {
                rise_db: 21.4,
                score: Some(0.98),
                heard_by: HeardBy::Name,
            })
        );
        // A louder moment before the name, or after its second, isn't it.
        let mut room = levels(0, 6_000, 2_400, 21.0);
        room[5].level_db = 70.0;
        room[40].level_db = 70.0;
        assert_eq!(
            near_try(ENDED, &[heard(2_000, 0.98)], &room, &[]).map(|done| done.rise_db),
            Ok(21.0)
        );
    }

    #[test]
    fn a_try_without_one_sure_hearing_in_the_window_does_not_count() {
        let room = levels(0, 8_000, 2_400, 21.0);
        assert_eq!(near_try(ENDED, &[], &room, &[]), Err(Rejected::NoHearing));
        // Before the prompt ended, or past its six seconds.
        let outside = [
            Hearing {
                began_ms: ENDED - 1,
                score: 0.99,
            },
            heard(WINDOW_MS + 1, 0.99),
        ];
        assert_eq!(
            near_try(ENDED, &outside, &room, &[]),
            Err(Rejected::NoHearing)
        );
        assert_eq!(
            near_try(ENDED, &[heard(1_000, 0.99), heard(3_000, 0.99)], &room, &[]),
            Err(Rejected::TwoHearings)
        );
        assert_eq!(
            near_try(ENDED, &[heard(2_000, 0.94)], &room, &[]),
            Err(Rejected::Unsure { score: 0.94 })
        );
        assert!(near_try(ENDED, &[heard(2_000, SCORE)], &room, &[]).is_ok());
        assert!(near_try(ENDED, &[heard(2_000, f32::NAN)], &room, &[]).is_err());
    }

    /// A line of the name begun `after_ms` after the prompt, 0.6 s long.
    fn said(after_ms: u64) -> Said {
        said_for(after_ms, 600)
    }

    fn said_for(after_ms: u64, long_ms: u64) -> Said {
        Said {
            began_ms: ENDED + after_ms,
            ended_ms: ENDED + after_ms + long_ms,
        }
    }

    /// #3923, lamp-29e7: the detector missed "Lamp" on three prompts of
    /// four while speech to text heard it whole. A line that was the name
    /// alone counts as a call, its rise read from mic1 the same way, and is
    /// kept as heard by text, with no score.
    #[test]
    fn a_line_heard_as_the_name_alone_counts_with_its_rise() {
        let room = levels(0, 6_000, 2_400, 21.37);
        assert_eq!(
            near_try_with_said(ENDED, &[], &[said(2_000)], &room, &[]),
            Ok(Try {
                rise_db: 21.4,
                score: None,
                heard_by: HeardBy::Text,
            })
        );
        // In the same window only, and one a try.
        for outside in [
            Said {
                began_ms: ENDED - 1,
                ended_ms: ENDED + 599,
            },
            said(WINDOW_MS + 1),
        ] {
            assert_eq!(
                near_try_with_said(ENDED, &[], &[outside], &room, &[]),
                Err(Rejected::NoHearing)
            );
        }
        assert_eq!(
            near_try_with_said(ENDED, &[], &[said(1_000), said(3_000)], &room, &[]),
            Err(Rejected::TwoHearings)
        );
        // The same guards: the lamp speaking, and its own sound uncounted.
        assert_eq!(
            near_try_with_said(ENDED, &[], &[said(2_000)], &room, &[spoke(0, Some(1_600))]),
            Err(Rejected::LampSpoke)
        );
        let mut room = levels(0, 6_000, 2_400, 21.0);
        room[24].own_voice = true;
        assert_eq!(
            near_try_with_said(ENDED, &[], &[said(2_000)], &room, &[]).map(|done| done.rise_db),
            Ok(2.0)
        );
    }

    /// A cough, the TV, or "Lamp is broken" is never a [`Said`]: with no
    /// line and no sure hearing there is no call, and a detector's hearing
    /// under its bar stays unsure without the name's own words beside it.
    #[test]
    fn without_a_line_of_the_name_alone_the_detectors_bar_is_unchanged() {
        let room = levels(0, 6_000, 2_400, 21.0);
        assert_eq!(
            near_try_with_said(ENDED, &[], &[], &room, &[]),
            Err(Rejected::NoHearing)
        );
        assert_eq!(
            near_try_with_said(ENDED, &[heard(2_000, 0.9)], &[], &room, &[]),
            Err(Rejected::Unsure { score: 0.9 })
        );
        // A line of the name 1.5 s off is a second call, not its words.
        assert_eq!(
            near_try_with_said(ENDED, &[heard(2_000, 0.9)], &[said(3_500)], &room, &[]),
            Err(Rejected::TwoHearings)
        );
    }

    /// The detector and speech to text hearing the same "Lamp" are one
    /// call, never two hearings: a sure hearing stays the detector's, and
    /// one under the bar counts by its words, from the detector's start.
    #[test]
    fn a_line_and_a_hearing_begun_within_a_second_are_one_call() {
        let room = levels(0, 6_000, 2_400, 21.0);
        assert_eq!(
            near_try_with_said(ENDED, &[heard(2_000, 0.98)], &[said(2_300)], &room, &[]),
            Ok(Try {
                rise_db: 21.0,
                score: Some(0.98),
                heard_by: HeardBy::Name,
            })
        );
        assert_eq!(
            near_try_with_said(ENDED, &[heard(2_000, 0.9)], &[said(1_200)], &room, &[]),
            Ok(Try {
                rise_db: 21.0,
                score: None,
                heard_by: HeardBy::Text,
            })
        );
        // A sure hearing 1.2 s into its line (a slow "Hey, … Lamp", or a
        // cough speech to text left out) is that line's call, the
        // detector's with its score; one under the bar there counts by text.
        assert_eq!(
            near_try_with_said(
                ENDED,
                &[heard(3_200, 0.98)],
                &[said_for(2_000, 1_600)],
                &levels(0, 6_000, 3_400, 21.0),
                &[]
            ),
            Ok(Try {
                rise_db: 21.0,
                score: Some(0.98),
                heard_by: HeardBy::Name,
            })
        );
        assert_eq!(
            near_try_with_said(
                ENDED,
                &[heard(3_200, 0.9)],
                &[said_for(2_000, 1_600)],
                &levels(0, 6_000, 3_400, 21.0),
                &[]
            ),
            Ok(Try {
                rise_db: 21.0,
                score: None,
                heard_by: HeardBy::Text,
            })
        );
        // Begun past the line's end and more than a second after it: two.
        assert_eq!(
            near_try_with_said(
                ENDED,
                &[heard(3_200, 0.98)],
                &[said_for(2_000, 1_000)],
                &room,
                &[]
            ),
            Err(Rejected::TwoHearings)
        );
        // Two separate calls, each heard both ways, are still two.
        assert_eq!(
            near_try_with_said(
                ENDED,
                &[heard(1_000, 0.98), heard(4_000, 0.98)],
                &[said(1_100), said(4_100)],
                &room,
                &[]
            ),
            Err(Rejected::TwoHearings)
        );
    }

    #[test]
    fn three_calls_heard_by_text_alone_make_the_bar() {
        let room = |rise| levels(0, 6_000, 2_400, rise);
        let tries: Vec<Try> = [22.4, 25.0, 23.1]
            .into_iter()
            .map(|rise| near_try_with_said(ENDED, &[], &[said(2_000)], &room(rise), &[]).unwrap())
            .collect();
        let calibrated = Calibrated::of(tries, 1_760_000_000).expect("a bar");
        assert!(calibrated.fits());
        let folder = tempfile::tempdir().expect("folder");
        let path = folder.path().join("near-call.json");
        write(&path, &calibrated).expect("written");
        assert_eq!(read(&path), Some(19.4));
    }

    /// A file from before #3923 (a score, no `heard_by`) reads as heard by
    /// the detector; one heard by text has no score. A score with text, or
    /// none with the detector, is no try a step made.
    #[test]
    fn old_and_text_files_read_and_odd_mixes_do_not() {
        let old: Try = serde_json::from_str(r#"{"rise_db":22.4,"score":0.98}"#).unwrap();
        assert_eq!(old.heard_by, HeardBy::Name);
        assert_eq!(old.score, Some(0.98));
        assert!(old.heard_surely());
        let text: Try = serde_json::from_str(r#"{"rise_db":22.4,"heard_by":"text"}"#).unwrap();
        assert_eq!((text.score, text.heard_by), (None, HeardBy::Text));
        assert!(text.heard_surely());
        assert_eq!(
            serde_json::to_value(text).unwrap(),
            serde_json::json!({"rise_db":22.4f32,"heard_by":"text"})
        );
        for odd in [
            r#"{"rise_db":22.4}"#,
            r#"{"rise_db":22.4,"score":0.98,"heard_by":"text"}"#,
        ] {
            let odd: Try = serde_json::from_str(odd).unwrap();
            assert!(!odd.heard_surely(), "{odd:?}");
        }
        let folder = tempfile::tempdir().expect("folder");
        let path = folder.path().join("near-call.json");
        std::fs::write(
            &path,
            br#"{"near_rise_db":19.4,"tries":[{"rise_db":22.4,"score":0.98},{"rise_db":25.0,"heard_by":"text"},{"rise_db":23.1,"score":1.0,"heard_by":"name"}],"at_unix_s":1}"#,
        )
        .expect("written");
        assert_eq!(read(&path), Some(19.4));
    }

    #[test]
    fn the_lamp_speaking_spoils_only_the_measured_span() {
        let room = levels(0, 8_000, 2_400, 21.0);
        let hearing = [heard(2_000, 0.98)];
        let spoken = |spans: &[Spoke]| near_try(ENDED, &hearing, &room, spans);
        // Ended 0.6 s before the name: its echo is gone.
        assert!(spoken(&[spoke(0, Some(1_400))]).is_ok());
        // Ended 0.4 s before: its echo may be what mic1 heard.
        assert_eq!(spoken(&[spoke(0, Some(1_600))]), Err(Rejected::LampSpoke));
        // Answering inside the rise window, or still speaking.
        assert_eq!(
            spoken(&[spoke(2_900, Some(4_000))]),
            Err(Rejected::LampSpoke)
        );
        assert_eq!(spoken(&[spoke(2_500, None)]), Err(Rejected::LampSpoke));
        // Answering 1.1 s after the name began: past the span.
        assert!(spoken(&[spoke(3_100, Some(5_000)), spoke(3_200, None)]).is_ok());
    }

    /// lamp-52e6, 27 October (#3504): a name heard at 0.99 0.3 s after the
    /// prompt's end is refused, however sure: the lamp's own tail or echo
    /// can't be told from the person there. The same name 0.6 s on counts.
    #[test]
    fn a_sure_name_in_the_prompts_tail_is_refused_and_one_after_it_counts() {
        let room = levels(0, 8_000, 7_000, 2.0);
        let prompt = [spoke(0, Some(0))];
        for score in [0.98, 0.99, 1.0] {
            assert_eq!(
                near_try(ENDED, &[heard(300, score)], &room, &prompt),
                Err(Rejected::LampSpoke)
            );
        }
        assert!(near_try(ENDED, &[heard(600, 0.99)], &room, &prompt).is_ok());
    }

    #[test]
    fn a_try_needs_a_level_with_the_quiet_known_and_never_counts_the_lamps_own_sound() {
        let hearing = [heard(2_000, 0.98)];
        assert_eq!(near_try(ENDED, &hearing, &[], &[]), Err(Rejected::NoLevel));
        let mut room = levels(0, 6_000, 2_400, 21.0);
        for level in &mut room {
            level.floor_db = None;
        }
        assert_eq!(
            near_try(ENDED, &hearing, &room, &[]),
            Err(Rejected::NoLevel)
        );
        let mut room = levels(0, 6_000, 2_400, 21.0);
        room[24].own_voice = true;
        assert_eq!(
            near_try(ENDED, &hearing, &room, &[]).map(|done| done.rise_db),
            Ok(2.0)
        );
    }

    #[test]
    fn setup_asks_until_three_count_while_five_prompts_can_still_get_there() {
        assert!(asks_again(0, 0));
        assert!(asks_again(2, 2));
        assert!(!asks_again(3, 3));
        // Two failed: three left for three.
        assert!(asks_again(2, 0));
        // Three failed: two left can't make three.
        assert!(!asks_again(3, 0));
        assert!(asks_again(4, 2));
        assert!(!asks_again(5, 2));
    }

    fn tries(rises: &[f32]) -> Vec<Try> {
        rises
            .iter()
            .map(|&rise_db| Try {
                rise_db,
                score: Some(0.98),
                heard_by: HeardBy::Name,
            })
            .collect()
    }

    #[test]
    fn the_bar_sits_three_under_the_quietest_call_between_sixteen_and_twenty_four() {
        assert_eq!(near_rise(&tries(&[22.4, 25.0, 23.1])), Some(19.4));
        assert_eq!(near_rise(&tries(&[17.0, 18.5, 19.0])), Some(LOWEST_DB));
        assert_eq!(near_rise(&tries(&[30.0, 33.0, 31.2])), Some(HIGHEST_DB));
        // Too few, or too far apart: the rule keeps 18.
        assert_eq!(near_rise(&tries(&[22.0, 23.0])), None);
        assert_eq!(near_rise(&tries(&[18.0, 28.1, 20.0])), None);
        assert_eq!(near_rise(&tries(&[18.0, 28.0, 20.0])), Some(LOWEST_DB));
        assert_eq!(near_rise(&tries(&[22.0, f32::NAN, 23.0])), None);
    }

    #[test]
    fn the_file_is_the_owners_whole_and_used_only_when_its_bar_fits_its_tries() {
        let folder = tempfile::tempdir().expect("folder");
        let path = folder.path().join("near-call.json");
        assert_eq!(read(&path), None);
        assert_eq!(Calibrated::of(tries(&[22.0]), 7), None);
        let calibrated = Calibrated::of(tries(&[22.4, 25.0, 23.1]), 1_760_000_000).expect("a bar");
        write(&path, &calibrated).expect("written");
        assert_eq!(read(&path), Some(19.4));
        assert!(!folder.path().join("near-call.json.new").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).expect("kept").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let kept: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).expect("kept")).expect("json");
        assert!(
            kept["near_rise_db"]
                .as_f64()
                .is_some_and(|bar| (bar - 19.4).abs() < 1e-4),
            "{kept}"
        );
        assert_eq!(kept["tries"].as_array().map(Vec::len), Some(3));
        // A bar its tries don't make, or no file worth reading: not used.
        let edited = Calibrated {
            near_rise_db: 30.0,
            ..calibrated
        };
        write(&path, &edited).expect("written");
        assert_eq!(read(&path), None);
        std::fs::write(&path, b"{}").expect("written");
        assert_eq!(read(&path), None);
        // Tries no step could have accepted: a score over 1, not a number,
        // or under the rule's bar.
        for score in [7.0, f32::NAN, 0.9] {
            let mut odd = tries(&[22.4, 25.0, 23.1]);
            odd[1].score = Some(score);
            let odd = Calibrated {
                near_rise_db: 19.4,
                tries: odd,
                at_unix_s: 1,
            };
            write(&path, &odd).expect("written");
            assert_eq!(read(&path), None, "{score}");
        }
    }

    /// A later build may keep more beside the bar (#1505's rule): its file
    /// still gives this build the bar, from the fields it knows.
    #[test]
    fn a_file_from_a_later_build_still_gives_its_bar() {
        let folder = tempfile::tempdir().expect("folder");
        let path = folder.path().join("near-call.json");
        let calibrated = Calibrated::of(tries(&[22.4, 25.0, 23.1]), 1_760_000_000).expect("a bar");
        let mut later = serde_json::to_value(&calibrated).expect("json");
        later["seat"] = serde_json::json!("the desk chair");
        later["tries"][0]["face_seen"] = serde_json::json!(true);
        std::fs::write(&path, serde_json::to_vec(&later).expect("json")).expect("written");
        assert_eq!(read(&path), Some(19.4));
    }
}
