//! One UTC day's tally recomputed off the lamp, from the voice's journal
//! (#7664, done-when 1: a busy-desk day against the allowance).
//!
//! Every chat answer that ends has one journal line, switch or no switch:
//! `lamp-voice second: chat answer: … after it was asked; P prompt tokens,
//! H from the cache, M not; C completion tokens` (`net::chat`'s
//! `answer_told`). Its tokens are read back into the service's `usage` and
//! counted by [`super::one_answer`], the live tally's own rule, into the
//! UTC day of the line's time. Read on another machine from journal pulls
//! (`journalctl -u lamp-voice -o short-iso`, or `short-iso-precise`): the
//! lamp keeps minutes to hours of journal, so a day is many pulls, and a
//! line in two overlapping pulls is counted once.
//!
//! - Numbers only: a line is read for its time and token counts; no other
//!   part of any line is kept, and what is printed is [`super::told`].
//! - The journal line has no model name: every answer is counted under
//!   [`MODEL`].
//! - Unlike the live tally, a restart loses nothing (its lines are in the
//!   journal), and an answer near midnight is in the day its line is.
//! - A gap between pulls is a gap in the count: nothing here can see it.

use super::{DAY_S, Day, one_answer};
use serde_json::{Map, Value};
use std::collections::HashSet;

/// The name every replayed answer is counted under.
pub const MODEL: &str = "journal";
/// What begins a chat answer's journal line, after the journal's own part.
const ANSWER: &str = "lamp-voice second: chat answer: ";
/// What comes just before its tokens.
const TOKENS: &str = " after it was asked; ";

/// `pulls`' chat answers in UTC day `day` (days since 1970), each line
/// once however many pulls hold it.
// ponytail: a line is told apart by its whole text; two answers in one
// second with the same times and tokens would be counted once.
pub fn replayed<'a>(pulls: impl IntoIterator<Item = &'a str>, day: u64) -> Day {
    let mut seen = HashSet::new();
    let mut counted = Day {
        day,
        ..Day::default()
    };
    for line in pulls.into_iter().flat_map(str::lines) {
        let line = line.trim_end();
        let in_day = line
            .split_whitespace()
            .next()
            .and_then(unix_s_of)
            .is_some_and(|at| at.div_euclid(DAY_S as i64) == day as i64);
        if !in_day {
            continue;
        }
        if let Some(usage) = usage_of(line)
            && seen.insert(line)
        {
            counted.add(&one_answer(MODEL, usage.as_ref()));
        }
    }
    counted
}

/// The service's `usage` as a `chat answer` line tells it: `None` for a
/// line that is not one, `Some(None)` for `no usage given`. A count told
/// as `?` was not given.
fn usage_of(line: &str) -> Option<Option<Value>> {
    let (_, answer) = line.split_once(ANSWER)?;
    let (_, tokens) = answer.split_once(TOKENS)?;
    if tokens == "no usage given" {
        return Some(None);
    }
    let (prompt, rest) = tokens.split_once(" prompt tokens, ")?;
    let (cache, completion) = rest.split_once("; ")?;
    let completion = completion.strip_suffix(" completion tokens")?;
    let cached = cache
        .split_once(" from the cache, ")
        .map(|(cached, _)| cached);
    let mut usage = Map::new();
    for (name, count) in [
        ("prompt_tokens", Some(prompt)),
        ("prompt_cache_hit_tokens", cached),
        ("completion_tokens", Some(completion)),
    ] {
        if let Some(count) = count.and_then(|count| count.parse::<u64>().ok()) {
            usage.insert(name.to_owned(), count.into());
        }
    }
    Some(Some(Value::Object(usage)))
}

/// `YYYY-MM-DD` as days since 1970.
pub fn day_named(date: &str) -> Option<u64> {
    u64::try_from(days_of(date)?).ok()
}

/// `YYYY-MM-DD` as days since 1970, before it too.
fn days_of(date: &str) -> Option<i64> {
    let mut parts = date.split('-');
    let (year, month, day) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || year.len() != 4 || month.len() != 2 || day.len() != 2 {
        return None;
    }
    let (year, month, day): (i64, i64, i64) =
        (year.parse().ok()?, month.parse().ok()?, day.parse().ok()?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // Days from the civil date, as H. Hinnant's `days_from_civil`.
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let of_era = year - era * 400;
    let of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let of_cycle = of_era * 365 + of_era / 4 - of_era / 100 + of_year;
    Some(era * 146_097 + of_cycle - 719_468)
}

/// A journal time as `short-iso` or `short-iso-precise` writes it
/// (`2026-11-18T07:00:00+0700`, `…+07:00`, `…Z`), as seconds since 1970.
fn unix_s_of(stamp: &str) -> Option<i64> {
    let (date, time) = stamp.split_once('T')?;
    let (clock, offset_s) = match time.strip_suffix('Z') {
        Some(clock) => (clock, 0),
        None => {
            let (clock, offset) = time.split_at(time.rfind(['+', '-'])?);
            let digits = offset[1..].replace(':', "");
            if digits.len() != 4 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            let hours: i64 = digits[..2].parse().ok()?;
            let minutes: i64 = digits[2..].parse().ok()?;
            let sign = if offset.starts_with('-') { -1 } else { 1 };
            (clock, sign * (hours * 3_600 + minutes * 60))
        }
    };
    let mut parts = clock.split('.').next()?.split(':');
    // Two digits each, so nothing a stamp says can overflow.
    let mut next = || {
        let part = parts.next().filter(|part| part.len() == 2)?;
        part.parse::<i64>().ok()
    };
    let (hours, minutes, seconds) = (next()?, next()?, next()?);
    Some(days_of(date)? * DAY_S as i64 + hours * 3_600 + minutes * 60 + seconds - offset_s)
}

#[cfg(test)]
mod tests {
    use super::super::{one_answer, told};
    use super::*;
    use serde_json::json;

    /// A journal line as `short-iso` gives it, with a chat answer's tokens.
    fn answer(at: &str, pid: u32, prompt: u64, cached: u64, completion: u64) -> String {
        format!(
            "{at} lamp-52e6 lamp-voice[{pid}]: lamp-voice second: chat answer: its first words \
             or call 900 ms and its end 1200 ms after it was asked; {prompt} prompt tokens, \
             {cached} from the cache, {} not; {completion} completion tokens",
            prompt - cached
        )
    }

    /// What the live tally counts for the same answers.
    fn live(usages: &[(u64, u64, u64)]) -> Day {
        let mut day = Day::default();
        for (prompt, cached, completion) in usages {
            let usage = json!({"prompt_tokens": prompt, "prompt_cache_hit_tokens": cached, "completion_tokens": completion});
            day.add(&one_answer(MODEL, Some(&usage)));
        }
        day
    }

    #[test]
    fn a_date_is_named_as_days_since_1970() {
        assert_eq!(day_named("1970-01-01"), Some(0));
        assert_eq!(day_named("2026-11-01"), Some(20_758));
        assert_eq!(day_named("2026-11-18"), Some(20_775));
        assert_eq!(day_named("2024-03-01"), Some(19_783));
        for bad in ["2026-13-01", "2026-11-32", "2026-11", "18/11/2026", ""] {
            assert_eq!(day_named(bad), None, "{bad}");
        }
    }

    /// 07:00 at GMT+7 is 00:00 UTC: the line a second before is the day
    /// before's, whatever the offset is written as.
    #[test]
    fn utc_midnight_divides_the_days() {
        let day = day_named("2026-11-18").unwrap();
        let pull = [
            answer("2026-11-18T06:59:59+0700", 7, 1000, 800, 10),
            answer("2026-11-18T07:00:00+0700", 7, 2000, 1500, 20),
            answer("2026-11-18T23:59:59.999999Z", 7, 3000, 0, 30),
            answer("2026-11-19T00:00:00+00:00", 7, 4000, 0, 40),
        ]
        .join("\n");
        assert_eq!(
            replayed([pull.as_str()], day),
            Day {
                day,
                ..live(&[(2000, 1500, 20), (3000, 0, 30)])
            }
        );
        assert_eq!(
            replayed([pull.as_str()], day - 1).all(),
            live(&[(1000, 800, 10)]).all()
        );
    }

    /// Two days' lines in pulls that overlap: each line once, in its day.
    #[test]
    fn overlapping_pulls_of_two_days_count_each_line_once() {
        let day = day_named("2026-11-18").unwrap();
        let a = answer("2026-11-18T08:00:00+0700", 7, 100, 0, 1);
        let b = answer("2026-11-18T09:00:00+0700", 7, 200, 100, 2);
        let c = answer("2026-11-19T08:00:00+0700", 7, 300, 0, 3);
        let first = [a.clone(), b.clone()].join("\n");
        let second = [b, c].join("\n");
        let pulls = [first.as_str(), second.as_str()];
        assert_eq!(
            replayed(pulls, day).all(),
            live(&[(100, 0, 1), (200, 100, 2)]).all()
        );
        assert_eq!(replayed(pulls, day + 1).all(), live(&[(300, 0, 3)]).all());
    }

    /// A restart mid-day: the voice's lines before and after it, under two
    /// processes, are all one day's.
    #[test]
    fn a_restart_mid_day_loses_nothing() {
        let day = day_named("2026-11-18").unwrap();
        let pull = [
            answer("2026-11-18T10:00:00+0700", 7, 500, 400, 5),
            "2026-11-18T10:00:30+0700 lamp-52e6 systemd[1]: Stopped lamp-voice.service.".to_owned(),
            "2026-11-18T10:00:31+0700 lamp-52e6 systemd[1]: Started lamp-voice.service.".to_owned(),
            answer("2026-11-18T10:01:00+0700", 9, 600, 0, 6),
        ]
        .join("\n");
        assert_eq!(
            replayed([pull.as_str()], day).all(),
            live(&[(500, 400, 5), (600, 0, 6)]).all()
        );
    }

    /// Words in the journal, in other lines or after a chat answer's
    /// tokens, are never in what is printed, and an answer that gave no
    /// count is counted as one.
    #[test]
    fn words_are_never_printed() {
        let day = day_named("2026-11-18").unwrap();
        let pull = [
            "2026-11-18T10:00:00+0700 lamp-52e6 lamp-voice[7]: lamp-voice second: heard \"buy oat milk for Jay\"".to_owned(),
            format!("{} buy oat milk", answer("2026-11-18T10:00:01+0700", 7, 10, 0, 1)),
            "2026-11-18T10:00:02+0700 lamp-52e6 lamp-voice[7]: lamp-voice second: chat answer: no words or call and its end 9 ms after it was asked; no usage given".to_owned(),
        ]
        .join("\n");
        let counted = replayed([pull.as_str()], day);
        assert_eq!(counted.all().answers, 1);
        assert_eq!(counted.all().uncounted, 1);
        let printed = format!("{}\n{}", told(&counted), serde_json::to_string(&counted).unwrap());
        for word in ["oat", "milk", "Jay", "buy"] {
            assert!(!printed.contains(word), "{printed}");
        }
    }
}
