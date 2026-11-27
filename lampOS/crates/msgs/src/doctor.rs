//! What an app that scores the lamp's turns (Lamp Doctor, #8726) may be
//! given and may send, with no words in either. A finished turn's record is
//! numbers, yes/no and names from a fixed list; the report an app sends out
//! (`log.send`) is a fixed schema read whole or refused whole. Neither has a
//! text field, so a sentence a person said, a name or a speaker has nowhere
//! to be put: every string in them is a member of a closed list here.
use crate::Invalid;
use lamp_bus::{Declaration, Kind};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// How a turn ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Answered,
    Stuck,
    Refused,
    Held,
    AskedAgain,
    /// A call failed with an error, not by waiting out its time.
    Error,
    NoAnswer,
    /// The answer was made and never said.
    LostAnswer,
    /// The turn ended before any answer: cut in on, stopped, or let go.
    Cancelled,
}

/// Which of the voice's two routes took a turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Route {
    First,
    Second,
    /// The voice did not say which took the turn.
    Unknown,
}

/// What a call was to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallKind {
    Stt,
    Llm,
    Tts,
    /// One of the lamp's own tools (a search, the weather, a light).
    Tool,
    /// A call an app's tool made.
    App,
    Memory,
    Task,
}

/// How a call failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaseOutcome {
    Error,
    Timeout,
    Refused,
}

/// The fault category the voice already gives a failed service call
/// (`net::Error::fault()`): a fixed list, never a service's own text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fault {
    Local,
    Transport,
    ConnectTimeout,
    ResponseTimeout,
    Authentication,
    QuotaExceeded,
    RateLimited,
    ServerUnavailable,
    ServerRejected,
    Protocol,
}

/// One call of a turn that failed, as a case: what it was to, how it failed,
/// the fixed code, how long it waited and which request it was. No words.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FailedCall {
    pub kind: CallKind,
    pub outcome: CaseOutcome,
    /// The HTTP status the service answered, if it answered one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http: Option<u16>,
    /// The voice's fault category, if the call went through its net layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fault: Option<Fault>,
    pub waited_ms: u32,
    /// The voice's number for the request the call was made for.
    pub request: u64,
}

/// A call of a turn that did what it was for or did not: the skill ran, the
/// memory was saved. Yes or no, never what was done.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proof {
    pub kind: CallKind,
    pub proved: bool,
}

/// One finished turn, for an app holding `turns`: its times, its outcome and
/// its counts, never its words. The voice works what a scorer needs from the
/// words (`heard_wrongly`, `name_missed`, `held_then_repeated`) into yes/no
/// here, so the scorer needs no text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnRecord {
    /// The voice's number for the turn, counted afresh each time it starts.
    pub turn: u64,
    /// The turn's end, in whole minutes since the Unix epoch.
    pub at_min: u64,
    /// From the end of what was heard to the first sound, in milliseconds;
    /// none when nothing was said.
    pub heard_to_first_sound_ms: Option<u32>,
    /// None when the voice does not count them yet.
    pub cut_ins: Option<u32>,
    pub tool_calls: u32,
    /// Calls that hit the 4.5 s timeout.
    pub tool_timeouts: u32,
    /// Calls that failed with an error, apart from the timeouts.
    pub tool_errors: u32,
    pub outcome: Outcome,
    /// Words heard and said, as counts; none when the voice has no
    /// transcription of it.
    pub words_heard: Option<u32>,
    pub words_said: Option<u32>,
    /// Asked again within 10 s, read as heard wrongly.
    pub heard_wrongly: bool,
    pub name_missed: bool,
    pub held_then_repeated: bool,
    /// The turn's calls that failed, each as a case.
    pub failed: Vec<FailedCall>,
    /// One for each call that has a proof (a skill, a memory write).
    pub proofs: Vec<Proof>,
    pub route: Route,
    /// The lamp's build number.
    pub build: u32,
}

impl TurnRecord {
    pub const TYPE: &str = "lamp_msgs/TurnRecord";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "voice/turn_record";

    /// Something that happened and is over: no value is kept, and none is
    /// promised at any period.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Event)
    }

    /// At most this many failed calls and proofs a turn carries.
    pub const MOST_CASES: usize = 16;

    pub fn check(&self) -> Result<(), Invalid> {
        if self.failed.len() > Self::MOST_CASES || self.proofs.len() > Self::MOST_CASES {
            return Err(Invalid("a turn record's cases are at most 16".into()));
        }
        Ok(())
    }
}

/// The voice's turn-record topic, for the apps' `turns` grant (#8726).
pub fn turn_record_topics() -> Vec<(&'static str, Declaration)> {
    vec![(TurnRecord::TOPIC, TurnRecord::declaration())]
}

/// Something the lamp's other parts journal, as a fixed code the scorer
/// counts per day (#7452's v6 signals).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Signal {
    SkillMakeFailed,
    SkillRefused,
    TaskNeverRun,
    MemoryNotSaved,
    LostAnswer,
    TtsRefusal,
}

/// Failure categories, the kinds of #7452's v6 score: the only labels a report
/// may carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Stuck,
    LostAnswer,
    TaskNeverRun,
    Error,
    SkillMakeFailed,
    SkillRefused,
    MemoryNotSaved,
    NoAnswer,
    TtsRefusal,
    Issue,
    Misheard,
    VerySlow,
}

/// A failure category and how many turns it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Failure {
    pub category: Category,
    pub count: u32,
}

/// How the score reads: green from 8, amber from 6 (the v6 score's bands).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Band {
    Green,
    Amber,
    Red,
}

impl Band {
    /// The band of a score 0.0 to 10.0.
    pub fn of(score: f64) -> Self {
        if score >= 8.0 {
            Self::Green
        } else if score >= 6.0 {
            Self::Amber
        } else {
            Self::Red
        }
    }
}

/// One failed or timed-out call, as a case of the report: when (the lamp's
/// own HH:MM), what it was to, how it failed, the HTTP status or the fault
/// category, how long it waited and the request's number. No words.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    /// `HH:MM`, the lamp's own time.
    pub at: String,
    pub kind: CallKind,
    pub outcome: CaseOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fault: Option<Fault>,
    pub waited_ms: u32,
    pub request: u64,
}

/// At most this many cases a report carries; the rest are counted in
/// `cases_dropped`.
pub const MOST_CASES: usize = 50;

/// The most any count of a day's report is (a lamp has far fewer turns).
pub const MOST_COUNT: u32 = 100_000;
/// The most a request's number, a time waited, a time to first sound and a
/// diagnosis's version are. A number the report carries is bounded to what it
/// can really be, so it holds little beyond itself; the grants an app that
/// sends may hold are refused beside the ones that read what a person says
/// (`lamp_skills::app_package::report_refused`).
pub const MOST_REQUEST: u64 = 100_000;
pub const MOST_WAIT_MS: u32 = 120_000;
pub const MOST_SOUND_MS: u32 = 600_000;
pub const MOST_VERSION: u32 = 100;
pub const MOST_BUILD: u32 = 10_000_000;

/// The report `log.send` sends to the one fixed endpoint, a flat body: one
/// day's numbers, the tier, the score 0.0 to 10.0 and its band, categories
/// from [`Category`] with counts, the failed calls as cases, the route, the
/// build and the diagnosis's version. There is no free-text field, and
/// anything else is refused whole ([`Report::from_value`]). `day` and `build`
/// are the lamp's: lamp-control sets both before it reads an app's report, so
/// what an app sent for them is overwritten.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    /// The lamp's own date, `YYYY-MM-DD`.
    pub day: String,
    pub turns: u32,
    /// Turns answered within 5 s, in percent.
    pub answered_within_5s_pct: u8,
    pub heard_to_first_sound_ms_median: u32,
    pub heard_to_first_sound_ms_p90: u32,
    pub asked_again: u32,
    pub held: u32,
    pub cut_ins: u32,
    pub tool_timeouts: u32,
    pub stuck: u32,
    /// The tier, 1 to 4 (the v6 score's T1 to T4).
    pub tier: u8,
    /// 0.0 to 10.0 in steps of 0.1 (the v6 score).
    pub score: f64,
    pub band: Band,
    pub failures: Vec<Failure>,
    pub cases: Vec<Case>,
    /// Cases left out past [`MOST_CASES`].
    pub cases_dropped: u32,
    pub route: Route,
    pub build: u32,
    pub diagnosis_version: u32,
}

/// Whether `text` is `HH:MM`.
fn hh_mm_ok(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 5
        && bytes[2] == b':'
        && [0, 1, 3, 4].iter().all(|at| bytes[*at].is_ascii_digit())
        && text[..2].parse::<u8>().is_ok_and(|hour| hour < 24)
        && text[3..].parse::<u8>().is_ok_and(|minute| minute < 60)
}

/// Whether `text` is a real date `YYYY-MM-DD`, 2026 to 2100.
pub fn day_ok(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    if ![0, 1, 2, 3, 5, 6, 8, 9]
        .iter()
        .all(|at| bytes[*at].is_ascii_digit())
    {
        return false;
    }
    let (Ok(year), Ok(month), Ok(day)) = (
        text[..4].parse::<u32>(),
        text[5..7].parse::<u32>(),
        text[8..].parse::<u32>(),
    ) else {
        return false;
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let most = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (2026..=2100).contains(&year) && (1..=most).contains(&day)
}

/// The date `YYYY-MM-DD` of a day counted from 1970-01-01.
pub fn day_string(days_since_epoch: i64) -> String {
    // Howard Hinnant's civil-from-days.
    let z = days_since_epoch + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

impl Report {
    /// The report in `value`, or why it is refused. All of it or none: a
    /// field the schema does not name, a string that is not on a list, a
    /// number out of its range or a category twice refuses the whole.
    pub fn from_value(value: &Value) -> Result<Self, Invalid> {
        // The serde error is not repeated: for a string that is not on a
        // list it names the string, and that may be a sentence.
        let report: Self = serde_json::from_value(value.clone()).map_err(|_| {
            Invalid("not the report's schema: a field, a name or a number it does not list".into())
        })?;
        report.check()?;
        Ok(report)
    }

    pub fn check(&self) -> Result<(), Invalid> {
        let bad = |why: &str| Err(Invalid(format!("a report's {why}")));
        if !day_ok(&self.day) {
            return bad("day is a date, YYYY-MM-DD");
        }
        if self.answered_within_5s_pct > 100 {
            return bad("percent is 0 to 100");
        }
        let counts = [
            self.turns,
            self.asked_again,
            self.held,
            self.cut_ins,
            self.tool_timeouts,
            self.stuck,
            self.cases_dropped,
        ];
        if counts.iter().any(|count| *count > MOST_COUNT)
            || self.heard_to_first_sound_ms_p90 > MOST_SOUND_MS
            || self.diagnosis_version > MOST_VERSION
            || self.build > MOST_BUILD
        {
            return bad("counts, times, build and version are within what they can be");
        }
        // The cases left out cannot outnumber the turns of the day.
        if self.cases_dropped > self.turns {
            return bad("cases_dropped is at most the turns");
        }
        if !self.score.is_finite()
            || !(0.0..=10.0).contains(&self.score)
            || (self.score * 10.0 - (self.score * 10.0).round()).abs() > 1e-6
        {
            return bad("score is 0.0 to 10.0 in steps of 0.1");
        }
        if Band::of(self.score) != self.band {
            return bad("band is the score's (green from 8, amber from 6)");
        }
        if !(1..=4).contains(&self.tier) {
            return bad("tier is 1 to 4");
        }
        if self.heard_to_first_sound_ms_p90 < self.heard_to_first_sound_ms_median {
            return bad("p90 is not below its median");
        }
        if self.failures.len() > 12
            || self
                .failures
                .iter()
                .any(|one| !(1..=MOST_COUNT).contains(&one.count))
            || self.failures.iter().enumerate().any(|(at, one)| {
                self.failures[..at]
                    .iter()
                    .any(|before| before.category == one.category)
            })
        {
            return bad("failures list each of the 12 kinds once, with a count of 1 or more");
        }
        if self.cases.len() > MOST_CASES {
            return bad("cases are at most 50");
        }
        for case in &self.cases {
            if !hh_mm_ok(&case.at)
                || case
                    .http
                    .is_some_and(|status| !(100..=599).contains(&status))
                || case.waited_ms > MOST_WAIT_MS
                || case.request > MOST_REQUEST
            {
                return bad("case is a time HH:MM, an HTTP status and a wait");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn record() -> TurnRecord {
        TurnRecord {
            turn: 7,
            at_min: 29_000_000,
            heard_to_first_sound_ms: Some(1_800),
            cut_ins: Some(1),
            tool_calls: 2,
            tool_timeouts: 1,
            tool_errors: 1,
            outcome: Outcome::AskedAgain,
            words_heard: Some(9),
            words_said: None,
            heard_wrongly: true,
            name_missed: false,
            held_then_repeated: false,
            failed: vec![FailedCall {
                kind: CallKind::Tool,
                outcome: CaseOutcome::Timeout,
                http: None,
                fault: Some(Fault::ResponseTimeout),
                waited_ms: 4_500,
                request: 7,
            }],
            proofs: vec![Proof {
                kind: CallKind::Memory,
                proved: false,
            }],
            route: Route::Second,
            build: 15_800,
        }
    }

    /// Every string anywhere in `value`.
    fn strings(value: &Value, found: &mut Vec<String>) {
        match value {
            Value::String(text) => found.push(text.clone()),
            Value::Array(items) => items.iter().for_each(|item| strings(item, found)),
            Value::Object(fields) => fields.values().for_each(|item| strings(item, found)),
            _ => {}
        }
    }

    #[test]
    fn a_turn_record_holds_numbers_and_listed_names_and_no_text() {
        let value = serde_json::to_value(record()).unwrap();
        let mut found = Vec::new();
        strings(&value, &mut found);
        found.sort();
        // The only strings are the outcome and the route, from their lists.
        assert_eq!(
            found,
            [
                "asked_again",
                "memory",
                "response_timeout",
                "second",
                "timeout",
                "tool"
            ]
        );
        // Nothing but its own fields: none for heard or said text, a person
        // or a speaker.
        let keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        for key in keys {
            for text in ["heard", "said", "text", "words", "name", "speaker", "who"] {
                assert!(
                    key == "words_heard"
                        || key == "words_said"
                        || key == "name_missed"
                        || key == "heard_wrongly"
                        || key == "heard_to_first_sound_ms"
                        || !key.contains(text),
                    "{key}"
                );
            }
        }
        // And a record that comes with a text field is not one.
        let mut with_text = value;
        with_text["heard"] = json!("set a timer for ten minutes");
        assert!(serde_json::from_value::<TurnRecord>(with_text).is_err());
        // Nor can a failed call carry a service's text, or a reason of its own.
        let mut case = serde_json::to_value(record()).unwrap();
        case["failed"][0]["why"] = json!("the service said: sorry, Mai");
        assert!(serde_json::from_value::<TurnRecord>(case).is_err());
        let mut word = serde_json::to_value(record()).unwrap();
        word["failed"][0]["fault"] = json!("the service said: sorry");
        assert!(serde_json::from_value::<TurnRecord>(word).is_err());
    }

    fn report() -> Value {
        json!({
            "day": "2026-11-26",
            "turns": 120, "answered_within_5s_pct": 83,
            "heard_to_first_sound_ms_median": 1900, "heard_to_first_sound_ms_p90": 4100,
            "asked_again": 6, "held": 3, "cut_ins": 4, "tool_timeouts": 2, "stuck": 1,
            "tier": 3, "score": 7.8, "band": "amber",
            "failures": [{"category": "misheard", "count": 6}, {"category": "error", "count": 2}],
            "cases": [
                {"at": "14:05", "kind": "tool", "outcome": "timeout", "fault": "response_timeout", "waited_ms": 4500, "request": 41},
                {"at": "14:09", "kind": "llm", "outcome": "refused", "http": 429, "fault": "rate_limited", "waited_ms": 800, "request": 44}
            ],
            "cases_dropped": 0,
            "route": "second", "build": 15800, "diagnosis_version": 1
        })
    }

    fn one_case() -> Value {
        json!({"at": "10:00", "kind": "tool", "outcome": "error", "waited_ms": 1, "request": 1})
    }

    #[test]
    fn a_turn_record_is_an_event_on_voice_turn_record_and_its_cases_are_few() {
        assert_eq!(TurnRecord::TOPIC, "voice/turn_record");
        assert_eq!(turn_record_topics().len(), 1);
        let mut many = record();
        assert!(many.check().is_ok());
        many.failed = vec![many.failed[0]; TurnRecord::MOST_CASES + 1];
        assert!(many.check().is_err());
        assert_eq!(
            crate::declaration_of("voice/turn_record"),
            Some(TurnRecord::declaration())
        );
    }

    #[test]
    fn the_fixed_schema_is_read() {
        let read = Report::from_value(&report()).unwrap();
        assert_eq!(read.score, 7.8);
        assert_eq!(read.band, Band::Amber);
        assert_eq!(read.failures.len(), 2);
        assert_eq!(read.cases.len(), 2);
        assert_eq!(read.cases[1].http, Some(429));
        // `unknown` is a route too, and a tier of 4 the deepest.
        let mut other = report();
        other["route"] = json!("unknown");
        other["tier"] = json!(4);
        assert!(Report::from_value(&other).is_ok());
    }

    #[test]
    fn a_report_holding_a_sentence_or_a_name_is_refused_whole() {
        let sentence = "turn off the light in Mai's room";
        type Change = fn(&mut Value, &str);
        let changes: [(&str, Change); 9] = [
            ("a sentence as a category", |r, s| {
                r["failures"][0]["category"] = json!(s)
            }),
            ("a sentence as the route", |r, s| r["route"] = json!(s)),
            ("a sentence in a field of its own", |r, s| {
                r["note"] = json!(s)
            }),
            ("a sentence in a failure", |r, s| {
                r["failures"][1]["detail"] = json!(s)
            }),
            ("a sentence as the score", |r, s| r["score"] = json!(s)),
            ("a sentence as the build", |r, s| r["build"] = json!(s)),
            ("a sentence as a case's time", |r, s| {
                r["cases"][0]["at"] = json!(s)
            }),
            ("a sentence as a case's fault", |r, s| {
                r["cases"][0]["fault"] = json!(s)
            }),
            ("a sentence in a case", |r, s| {
                r["cases"][1]["why"] = json!(s)
            }),
        ];
        for (what, change) in changes {
            let mut sent = report();
            change(&mut sent, sentence);
            let refused = Report::from_value(&sent).expect_err(what);
            assert!(
                !refused.0.contains("Mai"),
                "{what} repeated the text: {}",
                refused.0
            );
            let mut named = report();
            change(&mut named, "Mai");
            assert!(
                Report::from_value(&named).is_err(),
                "{what} (a name) was taken"
            );
        }
        // The day is a date and nothing else.
        for day in [
            "26 November",
            "2026-13-01",
            "2026-02-30",
            "2025-11-26",
            "Mai",
        ] {
            let mut sent = report();
            sent["day"] = json!(day);
            assert!(Report::from_value(&sent).is_err(), "{day}");
        }
    }

    #[test]
    fn a_number_out_of_its_range_or_a_category_twice_is_refused() {
        type Change = fn(&mut Value);
        let changes: [Change; 13] = [
            |r| r["score"] = json!(10.1),
            |r| r["score"] = json!(7.85),
            |r| r["score"] = json!(-0.1),
            |r| r["band"] = json!("green"),
            |r| r["answered_within_5s_pct"] = json!(101),
            |r| r["tier"] = json!(0),
            |r| r["tier"] = json!(5),
            |r| r["failures"][1]["category"] = json!("misheard"),
            |r| r["failures"][0]["count"] = json!(0),
            |r| r["heard_to_first_sound_ms_p90"] = json!(1000),
            |r| r["cases"][0]["at"] = json!("25:00"),
            |r| r["cases"][1]["http"] = json!(99),
            |r| r["cases"][0]["kind"] = json!("search"),
        ];
        for change in changes {
            let mut sent = report();
            change(&mut sent);
            assert!(Report::from_value(&sent).is_err(), "{sent}");
        }
        // Every number is held to what it can be: a request's number past a
        // request id, a wait past two minutes, a count past a day's.
        let bounded: [Change; 6] = [
            |r| r["cases"][0]["request"] = json!(MOST_REQUEST + 1),
            |r| r["cases"][0]["waited_ms"] = json!(MOST_WAIT_MS + 1),
            |r| r["cases_dropped"] = json!(121),
            |r| r["diagnosis_version"] = json!(MOST_VERSION + 1),
            |r| r["turns"] = json!(MOST_COUNT + 1),
            |r| r["heard_to_first_sound_ms_p90"] = json!(MOST_SOUND_MS + 1),
        ];
        for change in bounded {
            let mut sent = report();
            change(&mut sent);
            assert!(Report::from_value(&sent).is_err(), "{sent}");
        }
        // Words encoded as numbers do not fit: 50 cases of a request's number
        // past a request id, or a version past the last, are refused whole.
        let mut encoded = report();
        encoded["cases"] = json!((0..MOST_CASES as u64)
            .map(|at| json!({"at": "10:00", "kind": "tool", "outcome": "error", "waited_ms": 1, "request": 0x6869_6e74_u64 * 1_000_000 + at}))
            .collect::<Vec<_>>());
        assert!(Report::from_value(&encoded).is_err());
        encoded["cases"] = json!([]);
        encoded["diagnosis_version"] = json!(0x7468_6973_u64);
        assert!(Report::from_value(&encoded).is_err());
        // 51 cases are too many; 50, with the rest counted, are right.
        let mut many = report();
        many["cases"] = json!(vec![one_case(); MOST_CASES + 1]);
        assert!(Report::from_value(&many).is_err());
        many["cases"] = json!(vec![one_case(); MOST_CASES]);
        many["cases_dropped"] = json!(7);
        assert!(Report::from_value(&many).is_ok());
    }

    #[test]
    fn the_band_follows_the_score_and_a_day_counts_from_the_epoch() {
        assert_eq!(Band::of(8.0), Band::Green);
        assert_eq!(Band::of(7.9), Band::Amber);
        assert_eq!(Band::of(6.0), Band::Amber);
        assert_eq!(Band::of(5.9), Band::Red);
        assert_eq!(day_string(0), "1970-01-01");
        assert_eq!(day_string(20_783), "2026-11-26");
        assert_eq!(day_string(19_782), "2024-02-29");
        assert!(day_ok("2028-02-29") && !day_ok("2027-02-29"));
    }
}
