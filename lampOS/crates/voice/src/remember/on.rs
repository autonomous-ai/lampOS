//! A kept fact's day (#1693, `LAMP_MEMORY_EVENTS`): `remember {text, on}`.
//!
//! The model works out the day ("my sister's wedding is on Saturday" said
//! on Monday 19 October is `on: "2026-10-24"`), and the voice keeps it only
//! when the words heard in that line name a day it can come from
//! ([`names_a_day`]): a weekday; today, tonight, tomorrow, the day after
//! tomorrow; next or this week, a weekend; a month with a day number; a
//! numeric date. Anything else, a line not heard at the lamp included, has
//! its `on` dropped before the memory is asked: a day is never guessed
//! from a sentence that names none. What is left must be a real day from
//! the lamp's local today to [`WITHIN_DAYS`] after it ([`day_within`]), as
//! the memory's own check of an event day.
use lamp_gemini::ToolCall;
use lamp_local_time::LocalTime;
use serde_json::Value;
use std::borrow::Cow;

/// The furthest day ahead a kept fact's day may be: the memory's
/// `EVENT_WITHIN_DAYS`.
pub const WITHIN_DAYS: i64 = 60;

/// What `remember`'s description gains with the switch.
pub const DESCRIBED: &str = " When what they ask you to keep is about a day they name in their own words \
     (\"my sister's wedding is on Saturday\", \"the dentist tomorrow\", \"on 24 October\"), also give that day \
     as on, YYYY-MM-DD, worked out from today's date. Leave on out when they name no day, and never guess one.";

/// `on`'s parameter description.
pub const PARAMETER: &str = "Only when they named the day: that day, YYYY-MM-DD.";

const WEEKDAYS: &[&str] = &[
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
    "mondays",
    "tuesdays",
    "wednesdays",
    "thursdays",
    "fridays",
    "saturdays",
    "sundays",
];
/// Words that name a day by themselves.
const DAY_WORDS: &[&str] = &["today", "tonight", "tomorrow", "weekend"];
/// Months, with their short forms; never "may", the verb as often as the
/// month, nor "mar", "jun", "jul" alone, which are names.
const MONTHS: &[&str] = &[
    "january",
    "february",
    "march",
    "april",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
    "jan",
    "feb",
    "apr",
    "aug",
    "sep",
    "sept",
    "oct",
    "nov",
    "dec",
];
/// Vietnamese: words that name a day by themselves, two-word phrases.
const VI_PHRASES: &[[&str; 2]] = &[
    ["hôm", "nay"],
    ["tối", "nay"],
    ["ngày", "mai"],
    ["ngày", "kia"],
    ["tuần", "sau"],
    ["tuần", "tới"],
    ["tuần", "này"],
    ["cuối", "tuần"],
    ["chủ", "nhật"],
];
/// Vietnamese weekdays after "thứ": hai (Monday) to bảy (Saturday).
const VI_WEEKDAYS: &[&str] = &[
    "hai", "ba", "tư", "năm", "sáu", "bảy", "2", "3", "4", "5", "6", "7",
];

/// A day number: 1 to 31, bare or with its ordinal ending ("24th").
fn day_number(word: &str) -> bool {
    let digits = word.trim_end_matches(|c: char| c.is_ascii_alphabetic());
    let ending = &word[digits.len()..];
    matches!(ending, "" | "st" | "nd" | "rd" | "th")
        && !digits.is_empty()
        && digits.len() <= 2
        && digits.parse::<u32>().is_ok_and(|n| (1..=31).contains(&n))
}

/// A numeric date: two or three groups of digits joined by `/` or `-`
/// ("24/10", "10-24-2026", "2026-10-24").
fn numeric_date(token: &str) -> bool {
    let token = token.trim_matches(|c: char| !c.is_ascii_digit());
    for separator in ['/', '-'] {
        let parts: Vec<&str> = token.split(separator).collect();
        if (2..=3).contains(&parts.len())
            && parts.iter().all(|part| {
                (1..=4).contains(&part.len()) && part.bytes().all(|b| b.is_ascii_digit())
            })
        {
            return true;
        }
    }
    false
}

/// Whether `heard` names a day a date can come from. Deterministic:
/// lower-cased words, compared whole.
pub fn names_a_day(heard: &str) -> bool {
    let lower = heard.to_lowercase();
    if lower.split_whitespace().any(numeric_date) {
        return true;
    }
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    let has = |set: &[&str]| words.iter().any(|word| set.contains(word));
    if has(WEEKDAYS) || has(DAY_WORDS) {
        return true;
    }
    let pairs = || words.windows(2);
    if pairs().any(|pair| matches!(pair, ["next" | "this", "week"])) {
        return true;
    }
    if has(MONTHS) && words.iter().any(|word| day_number(word)) {
        return true;
    }
    if pairs().any(|pair| VI_PHRASES.iter().any(|phrase| pair == phrase))
        || pairs().any(|pair| pair[0] == "thứ" && VI_WEEKDAYS.contains(&pair[1]))
    {
        return true;
    }
    // "ngày 24 tháng 10": a month with a day number.
    words.contains(&"tháng") && words.iter().any(|word| day_number(word))
}

/// `remember`'s call as the memory is asked it: `on` dropped unless the
/// line's `heard` words name a day ([`names_a_day`]). Nothing heard, a
/// text from the app or a line the lamp wrote, drops it too. Any other
/// call is as it is.
pub fn call_as_heard<'a>(call: &'a ToolCall, heard: Option<&str>) -> Cow<'a, ToolCall> {
    if call.name != super::REMEMBER
        || call.args.get("on").is_none()
        || heard.is_some_and(names_a_day)
    {
        return Cow::Borrowed(call);
    }
    let mut call = call.clone();
    if let Some(args) = call.args.as_object_mut() {
        args.remove("on");
    }
    Cow::Owned(call)
}

/// `on` as the memory is sent it: a real `YYYY-MM-DD` from the local day
/// of `now` to [`WITHIN_DAYS`] after it, else nothing.
pub fn day_within(
    on: Option<&Value>,
    now_unix_ms: u64,
    zone: Option<&LocalTime>,
) -> Option<String> {
    let on = on?.as_str()?;
    let day = crate::local_time::day_of(on)?;
    let today = super::local_day(now_unix_ms, zone);
    (today..=today + WITHIN_DAYS)
        .contains(&day)
        .then(|| on.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The words check: kept for a line that names a day, dropped for one
    /// that names none.
    #[test]
    fn a_day_is_kept_only_from_words_that_name_one() {
        for (heard, named) in [
            ("my sister's wedding is on Saturday", true),
            ("the dentist tomorrow", true),
            ("on the 24th of October", true),
            ("remember the dentist the day after tomorrow", true),
            ("I fly out tonight", true),
            ("the meeting is today at three", true),
            ("we go camping next week", true),
            ("we go camping this weekend", true),
            ("my exam is on October 24", true),
            ("the party is on 24/10", true),
            ("the party is on 2026-10-24", true),
            ("đám cưới chị tôi vào thứ bảy", true),
            ("nhớ giúp tôi ngày mai đi nha sĩ", true),
            ("sinh nhật ngày 24 tháng 10", true),
            ("remember my sister's name is Mai", false),
            ("my sister's wedding", false),
            ("remember that I like October", false),
            ("my locker is number 24", false),
            ("I may be late", false),
            ("I'm 3.5 feet from the door", false),
            ("", false),
        ] {
            assert_eq!(names_a_day(heard), named, "{heard:?}");
        }
    }

    fn remember(args: Value) -> ToolCall {
        ToolCall {
            id: "c1".into(),
            name: super::super::REMEMBER.into(),
            args,
        }
    }

    /// `on` goes to the memory only with heard words that name a day.
    #[test]
    fn on_is_dropped_unless_the_heard_line_names_a_day() {
        let call = remember(json!({"text":"Sister's wedding is on Saturday.","on":"2026-10-24"}));
        assert!(matches!(
            call_as_heard(&call, Some("my sister's wedding is on Saturday")),
            Cow::Borrowed(_)
        ));
        for heard in [None, Some("remember my sister's wedding")] {
            assert_eq!(
                call_as_heard(&call, heard).args,
                json!({"text":"Sister's wedding is on Saturday."}),
                "{heard:?}"
            );
        }
        let plain = remember(json!({"text":"Her name is Mai."}));
        assert!(matches!(call_as_heard(&plain, None), Cow::Borrowed(_)));
    }

    /// From today, local, to 60 days on; nothing else.
    #[test]
    fn a_day_is_sent_only_from_today_to_sixty_days_on() {
        // Monday 2026-10-19, noon UTC.
        let now = 1_792_411_200_000;
        for (on, sent) in [
            (json!("2026-10-24"), true),
            (json!("2026-10-19"), true),
            (json!("2026-12-18"), true),
            (json!("2026-12-19"), false),
            (json!("2026-10-18"), false),
            (json!("2026-02-30"), false),
            (json!("Saturday"), false),
            (json!(20261024), false),
        ] {
            assert_eq!(day_within(Some(&on), now, None).is_some(), sent, "{on}");
        }
        assert_eq!(day_within(None, now, None), None);
    }
}
