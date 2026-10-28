//! A kept fact's day (#1693, `LAMP_MEMORY_EVENTS`): `remember {text, on}`.
//!
//! The model works out the day ("my sister's wedding is on Saturday" said
//! on Monday 19 October is `on: "2026-10-24"`), and the voice keeps it only
//! when the words heard in that line name that same day ([`names_the_day`],
//! against the lamp's local today): a weekday `on` falls on, within the
//! next 7 days, or 8 to 14 days on after "next"; today or tonight, `on` is
//! today; tomorrow, today and one; the day after tomorrow, today and two;
//! a weekend, a Saturday or Sunday within two weeks; this week or next
//! week, `on` within that Monday-to-Sunday week; a month with a day
//! number, `on`'s month and day; a numeric date, two or three numbers
//! joined by `/` or `-` that read as `on`, day first or month first ("3/4
//! cup" agrees only with 3 April or 4 March). Anything else, a day the
//! words name that `on` is not and a line not heard at the lamp included,
//! has its `on` dropped before the memory is asked: a day is never guessed
//! from a sentence that names none, nor taken from one that names
//! another. What is left must be a real day from the lamp's local today to
//! [`WITHIN_DAYS`] after it ([`day_within`]), as the memory's own check of
//! an event day.
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

/// Weekdays, Monday first, and their plurals ("on Fridays").
const WEEKDAYS: [&str; 7] = [
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
];
/// Months, January first, with their short forms; never "may", the verb
/// as often as the month, nor "mar", "jun", "jul" alone, which are names.
const MONTHS: &[(&str, usize)] = &[
    ("january", 1),
    ("february", 2),
    ("march", 3),
    ("april", 4),
    ("june", 6),
    ("july", 7),
    ("august", 8),
    ("september", 9),
    ("october", 10),
    ("november", 11),
    ("december", 12),
    ("jan", 1),
    ("feb", 2),
    ("apr", 4),
    ("aug", 8),
    ("sep", 9),
    ("sept", 9),
    ("oct", 10),
    ("nov", 11),
    ("dec", 12),
];
/// Vietnamese weekdays after "thứ": hai (Monday) to bảy (Saturday), as
/// days after Monday.
const VI_WEEKDAYS: &[(&str, i64)] = &[
    ("hai", 0),
    ("ba", 1),
    ("tư", 2),
    ("năm", 3),
    ("sáu", 4),
    ("bảy", 5),
    ("2", 0),
    ("3", 1),
    ("4", 2),
    ("5", 3),
    ("6", 4),
    ("7", 5),
];

/// A day number: 1 to 31, bare or with its ordinal ending ("24th").
fn day_number(word: &str) -> Option<i64> {
    let digits = word.trim_end_matches(|c: char| c.is_ascii_alphabetic());
    let ending = &word[digits.len()..];
    let number = digits.parse::<i64>().ok()?;
    (matches!(ending, "" | "st" | "nd" | "rd" | "th")
        && !digits.is_empty()
        && digits.len() <= 2
        && (1..=31).contains(&number))
    .then_some(number)
}

/// Whether `token` is a numeric date that reads as `(year, month, day)`:
/// two or three groups of digits joined by `/` or `-`; `YYYY-MM-DD`, or a
/// day and a month in either order with an optional year after them
/// ("24/10", "10-24-2026", "3/4").
fn numeric_date(token: &str, (year, month, day): (i64, usize, i64)) -> bool {
    let token = token.trim_matches(|c: char| !c.is_ascii_digit());
    let month = month as i64;
    for separator in ['/', '-'] {
        let parts: Vec<&str> = token.split(separator).collect();
        if !(2..=3).contains(&parts.len())
            || !parts.iter().all(|part| {
                (1..=4).contains(&part.len()) && part.bytes().all(|b| b.is_ascii_digit())
            })
        {
            continue;
        }
        let Ok(numbers) = parts
            .iter()
            .map(|part| part.parse::<i64>())
            .collect::<Result<Vec<_>, _>>()
        else {
            continue;
        };
        if parts[0].len() == 4 {
            return numbers.len() == 3 && numbers[..] == [year, month, day];
        }
        let year_agrees = match parts.get(2) {
            None => true,
            Some(part) if part.len() == 4 => numbers[2] == year,
            Some(part) if part.len() == 2 => numbers[2] == year % 100,
            Some(_) => false,
        };
        return year_agrees
            && ((numbers[0], numbers[1]) == (day, month)
                || (numbers[0], numbers[1]) == (month, day));
    }
    false
}

/// Whether the words in `heard` name the day `on`, both days since
/// 1970-01-01, said on the lamp's local day `today`. Deterministic:
/// lower-cased words, compared whole; one naming that agrees is enough.
pub fn names_the_day(heard: &str, on: i64, today: i64) -> bool {
    let ahead = on - today;
    if ahead < 0 {
        return false;
    }
    let (year, month, day) = crate::local_time::civil(on);
    // Days after Monday, of `on` and of today.
    let weekday = (on + 3).rem_euclid(7);
    let this_sunday = today + 6 - (today + 3).rem_euclid(7);
    let in_week = |sunday: i64| (sunday - 6..=sunday).contains(&on);
    let lower = heard.to_lowercase();
    if lower
        .split_whitespace()
        .any(|token| numeric_date(token, (year, month, day)))
    {
        return true;
    }
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    let before = |at: usize| at.checked_sub(1).map(|at| words[at]);
    let after = |at: usize| words.get(at + 1).copied();
    let day_numbers = || words.iter().filter_map(|word| day_number(word));
    let vi_next_week = words
        .windows(2)
        .any(|pair| matches!(pair, ["tuần", "sau" | "tới"]));
    for (at, word) in words.iter().enumerate() {
        let named_weekday = WEEKDAYS
            .iter()
            .position(|name| *word == *name || word.strip_suffix('s') == Some(*name));
        let agrees = match *word {
            _ if named_weekday.is_some() => {
                named_weekday == Some(weekday as usize)
                    && if before(at) == Some("next") {
                        (8..=14).contains(&ahead)
                    } else {
                        ahead <= 7
                    }
            }
            "today" | "tonight" => ahead == 0,
            "tomorrow" => ahead == if before(at) == Some("after") { 2 } else { 1 },
            "weekend" => weekday >= 5 && ahead <= 13,
            "week" => match before(at) {
                Some("this") => in_week(this_sunday),
                Some("next") => in_week(this_sunday + 7),
                _ => false,
            },
            "nay" => matches!(before(at), Some("hôm" | "tối")) && ahead == 0,
            "mai" => before(at) == Some("ngày") && ahead == 1,
            "kia" => before(at) == Some("ngày") && ahead == 2,
            "tuần" => match after(at) {
                Some("này") if before(at) != Some("cuối") => in_week(this_sunday),
                Some("sau" | "tới") => in_week(this_sunday + 7),
                _ => before(at) == Some("cuối") && weekday >= 5 && ahead <= 13,
            },
            "thứ" | "chủ" => {
                let named = if *word == "chủ" {
                    (after(at) == Some("nhật")).then_some(6)
                } else {
                    after(at).and_then(|next| {
                        VI_WEEKDAYS
                            .iter()
                            .find(|(name, _)| *name == next)
                            .map(|(_, day)| *day)
                    })
                };
                named == Some(weekday)
                    && if vi_next_week {
                        (1..=14).contains(&ahead)
                    } else {
                        ahead <= 7
                    }
            }
            "tháng" => {
                after(at).and_then(|next| next.parse::<usize>().ok()) == Some(month)
                    && day_numbers().any(|number| number == day)
            }
            _ => {
                MONTHS
                    .iter()
                    .any(|(name, number)| name == word && *number == month)
                    && day_numbers().any(|number| number == day)
            }
        };
        if agrees {
            return true;
        }
    }
    false
}

/// The lamp's local today, as days since 1970-01-01.
fn today() -> i64 {
    super::local_day(super::unix_ms(), crate::local_time::system())
}

/// `remember`'s call as the memory is asked it: `on` dropped unless the
/// line's `heard` words name that day ([`names_the_day`]) on the lamp's
/// local today. Nothing heard, a text from the app or a line the lamp
/// wrote, drops it too. A call with no `on` is given one only as
/// [`day_to_fill`] says (#3987). Any other call is as it is.
pub fn call_as_heard<'a>(call: &'a ToolCall, heard: Option<&str>) -> Cow<'a, ToolCall> {
    call_as_heard_on(call, heard, today)
}

/// [`call_as_heard`] with today's day from `today`.
fn call_as_heard_on<'a>(
    call: &'a ToolCall,
    heard: Option<&str>,
    today: impl FnOnce() -> i64,
) -> Cow<'a, ToolCall> {
    if call.name != super::REMEMBER {
        return Cow::Borrowed(call);
    }
    let Some(on) = call.args.get("on") else {
        // No day from the model: one only from the call's own text and the
        // heard line together (#3987).
        let text = call.args.get("text").and_then(Value::as_str);
        let Some(day) = heard
            .zip(text)
            .and_then(|(heard, text)| day_to_fill(text, heard, today()))
        else {
            return Cow::Borrowed(call);
        };
        let mut call = call.clone();
        if let Some(args) = call.args.as_object_mut() {
            args.insert("on".into(), super::day_name(day).into());
        }
        return Cow::Owned(call);
    };
    let day = on.as_str().and_then(crate::local_time::day_of);
    if let (Some(heard), Some(day)) = (heard, day)
        && names_the_day(heard, day, today())
    {
        return Cow::Borrowed(call);
    }
    let mut call = call.clone();
    if let Some(args) = call.args.as_object_mut() {
        args.remove("on");
    }
    Cow::Owned(call)
}

/// Words that make a line about a day past, never a coming one: "last
/// Friday", "two days ago", "I saw my sister on Sunday".
const PAST: &[&str] = &[
    "last",
    "ago",
    "yesterday",
    "was",
    "were",
    "went",
    "had",
    "did",
    "saw",
    "met",
    "used",
];
/// Words that make a line about a day a coming event: "my dentist is on
/// Friday", "I have an appointment", "remind me", "tomorrow".
const COMING: &[&str] = &[
    "is",
    "are",
    "will",
    "ll",
    "going",
    "gonna",
    "have",
    "has",
    "appointment",
    "booked",
    "starts",
    "begins",
    "due",
    "remind",
    "tomorrow",
    "next",
    "coming",
    "upcoming",
];

/// The day a `remember` the model gave no `on` is kept with (#3987: a
/// Friday dental appointment kept undated was later spoken of as today).
/// Strict, so a day is never put on what isn't about one: the call's own
/// `text` must name exactly one day from tomorrow to [`WITHIN_DAYS`] after
/// `today` ([`names_the_day`]), and the `heard` line that same day (a day
/// named elsewhere in the line never lands on another fact: "my son starts
/// school on Monday, remember his name is Tom"). Neither may say a repeat
/// (a weekday's plural, "every", "each", "weekly", "daily": a habit is no
/// event day) or a past ([`PAST`]: "I went to the dentist last Friday"),
/// and one of them must say a coming event ([`COMING`]). Today itself, or
/// two days with today among them ("on Friday" said on a Friday, which may
/// be today or next week), give none: "I fed the cat today" is no coming
/// event to ask after.
fn day_to_fill(text: &str, heard: &str, today: i64) -> Option<i64> {
    let words = |said: &str| -> Vec<String> {
        said.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|word| !word.is_empty())
            .map(str::to_owned)
            .collect()
    };
    let (text_words, heard_words) = (words(text), words(heard));
    let undated = |word: &String| {
        PAST.contains(&word.as_str())
            || matches!(word.as_str(), "every" | "each" | "weekly" | "daily")
            || WEEKDAYS
                .iter()
                .any(|name| word.strip_suffix('s') == Some(*name))
    };
    if text_words.iter().chain(&heard_words).any(undated)
        || !text_words
            .iter()
            .chain(&heard_words)
            .any(|word| COMING.contains(&word.as_str()))
    {
        return None;
    }
    let mut named = (today..=today + WITHIN_DAYS).filter(|day| names_the_day(text, *day, today));
    let day = named.next().filter(|day| *day > today)?;
    (named.next().is_none() && names_the_day(heard, day, today)).then_some(day)
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

    /// Days since 1970-01-01 of a `YYYY-MM-DD`.
    fn day(name: &str) -> i64 {
        crate::local_time::day_of(name).unwrap()
    }

    /// The words check, said on Monday 19 October 2026: kept only when the
    /// words name the day `on` is (reviewer-lead P3 on #2680).
    #[test]
    fn a_day_is_kept_only_from_words_that_name_that_day() {
        let monday = day("2026-10-19");
        for (heard, on, agrees) in [
            ("my sister's wedding is on Saturday", "2026-10-24", true),
            ("my sister's wedding is on Saturday", "2026-10-31", false),
            ("dentist on Friday", "2026-10-23", true),
            ("dentist on Friday", "2026-10-22", false),
            ("dentist on Fridays", "2026-10-23", true),
            ("dentist next Friday", "2026-10-30", true),
            ("dentist next Friday", "2026-10-23", false),
            ("the meeting is on Monday", "2026-10-26", true),
            ("the meeting is today at three", "2026-10-19", true),
            ("the meeting is today at three", "2026-10-20", false),
            ("I fly out tonight", "2026-10-19", true),
            ("the dentist tomorrow", "2026-10-20", true),
            ("the dentist tomorrow", "2026-10-21", false),
            ("the dentist the day after tomorrow", "2026-10-21", true),
            ("the dentist the day after tomorrow", "2026-10-20", false),
            ("we go camping this weekend", "2026-10-24", true),
            ("we go camping this weekend", "2026-10-25", true),
            ("we go camping this weekend", "2026-10-23", false),
            ("we go camping next week", "2026-10-28", true),
            ("we go camping next week", "2026-10-22", false),
            ("the review is this week", "2026-10-25", true),
            ("the review is this week", "2026-10-26", false),
            ("my exam is on October 24", "2026-10-24", true),
            ("on the 24th of October", "2026-10-24", true),
            ("my exam is on October 24", "2026-10-25", false),
            ("my exam is on November 24", "2026-10-24", false),
            ("the party is on 24/10", "2026-10-24", true),
            ("the party is on 10/24/2026", "2026-10-24", true),
            ("the party is on 2026-10-24", "2026-10-24", true),
            ("the party is on 24/10", "2026-10-25", false),
            ("the party is on 24/10/2027", "2026-10-24", false),
            ("add 3/4 cup of flour", "2026-10-24", false),
            ("add 3/4 cup of flour", "2026-11-03", false),
            ("đám cưới chị tôi vào thứ bảy", "2026-10-24", true),
            ("đám cưới chị tôi vào thứ bảy", "2026-10-25", false),
            ("đám cưới vào chủ nhật", "2026-10-25", true),
            ("nhớ giúp tôi ngày mai đi nha sĩ", "2026-10-20", true),
            ("nhớ giúp tôi ngày mai đi nha sĩ", "2026-10-21", false),
            ("hôm nay họp", "2026-10-19", true),
            ("sinh nhật ngày 24 tháng 10", "2026-10-24", true),
            ("sinh nhật ngày 24 tháng 11", "2026-10-24", false),
            ("đi cắm trại cuối tuần", "2026-10-24", true),
            ("đi cắm trại tuần sau", "2026-10-27", true),
            ("remember my sister's name is Mai", "2026-10-24", false),
            ("my sister's wedding", "2026-10-24", false),
            ("remember that I like October", "2026-10-24", false),
            ("my locker is number 24", "2026-10-24", false),
            ("I may be late", "2026-10-19", false),
            ("I'm 3.5 feet from the door", "2026-10-19", false),
            ("", "2026-10-19", false),
        ] {
            assert_eq!(
                names_the_day(heard, day(on), monday),
                agrees,
                "{heard:?} on {on}"
            );
        }
        // "3/4 cup" reads as 3 April or 4 March: the only days it agrees with.
        let march = day("2027-03-01");
        for (on, agrees) in [
            ("2027-03-04", true),
            ("2027-04-03", true),
            ("2027-04-04", false),
        ] {
            assert_eq!(names_the_day("add 3/4 cup", day(on), march), agrees, "{on}");
        }
        // A day before today is never one the words name.
        assert!(!names_the_day("on Friday", day("2026-10-16"), monday));
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
    fn on_is_dropped_unless_the_heard_line_names_that_day() {
        let call = remember(json!({"text":"Sister's wedding is on Saturday.","on":"2026-10-24"}));
        let monday = || day("2026-10-19");
        assert!(matches!(
            call_as_heard_on(&call, Some("my sister's wedding is on Saturday"), monday),
            Cow::Borrowed(_)
        ));
        for heard in [
            None,
            Some("remember my sister's wedding"),
            Some("my sister's wedding is on Sunday"),
        ] {
            assert_eq!(
                call_as_heard_on(&call, heard, monday).args,
                json!({"text":"Sister's wedding is on Saturday."}),
                "{heard:?}"
            );
        }
        let plain = remember(json!({"text":"Her name is Mai."}));
        assert!(matches!(call_as_heard(&plain, None), Cow::Borrowed(_)));
    }

    /// #3987, the tester's day: "my dental appointment on Friday" said on
    /// Wednesday 28 October, kept with no `on` from the model, is kept as
    /// Friday 30 October; "tomorrow" and "next Tuesday" the same way, each
    /// named by the call's own text and the line. Today, a habit, a span,
    /// a past, no coming word, no day, or a day the line names for another
    /// fact keeps none (the reviewers' P2s on #4010).
    #[test]
    fn a_day_the_model_left_out_is_kept_only_from_the_facts_own_coming_day() {
        let wednesday = day("2026-10-28");
        for (text, heard, on) in [
            (
                "Dental appointment on Friday.",
                "Lamp, remind me about my dental appointment on Friday.",
                "2026-10-30",
            ),
            (
                "My dentist appointment is on Friday.",
                "My dentist appointment is on Friday",
                "2026-10-30",
            ),
            (
                "Dentist is tomorrow.",
                "my dentist is tomorrow",
                "2026-10-29",
            ),
            (
                "Flies to Hanoi the day after tomorrow.",
                "the day after tomorrow I fly to Hanoi",
                "2026-10-30",
            ),
            (
                "Exam is next Tuesday.",
                "my exam is next Tuesday",
                "2026-11-10",
            ),
            (
                "The party is on 7 November.",
                "the party is on 7 November",
                "2026-11-07",
            ),
        ] {
            assert_eq!(
                day_to_fill(text, heard, wednesday),
                Some(day(on)),
                "{heard}"
            );
            assert_eq!(
                call_as_heard_on(&remember(json!({"text":text})), Some(heard), || wednesday).args["on"],
                on,
                "{heard}"
            );
        }
        for (text, heard) in [
            ("Fed the cat today.", "I fed the cat today"),
            (
                "The plant needs water on Fridays.",
                "the plant needs water on Fridays",
            ),
            ("Dentist is every Friday.", "my dentist is every Friday"),
            ("Busy this week.", "I'm busy this week"),
            ("Favourite colour is green.", "my favourite colour is green"),
            (
                "Went to the dentist last Friday.",
                "I went to the dentist last Friday",
            ),
            ("Saw my sister on Sunday.", "I saw my sister on Sunday"),
            ("Dentist on Friday.", "dentist on Friday"),
            (
                "Son's name is Tom.",
                "My son starts school on Monday, remember his name is Tom.",
            ),
        ] {
            assert_eq!(day_to_fill(text, heard, wednesday), None, "{heard}");
            assert!(matches!(
                call_as_heard_on(&remember(json!({"text":text})), Some(heard), || wednesday),
                Cow::Borrowed(_)
            ));
        }
        // The text's day must be the line's too.
        assert_eq!(
            day_to_fill(
                "Dentist is on Friday.",
                "my dentist is on Thursday",
                wednesday
            ),
            None
        );
        // "On Friday" said on a Friday may be today or next week: none.
        assert_eq!(
            day_to_fill(
                "Dentist is on Friday.",
                "my dentist is on Friday",
                day("2026-10-30")
            ),
            None
        );
        // A model's day the words don't name is dropped, never replaced
        // with another the line names ("last Friday" said with the 23rd).
        let past = remember(json!({"text":"Went to the dentist last Friday.","on":"2026-10-23"}));
        assert_eq!(
            call_as_heard_on(&past, Some("I went to the dentist last Friday"), || {
                wednesday
            })
            .args,
            json!({"text":"Went to the dentist last Friday."})
        );
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
