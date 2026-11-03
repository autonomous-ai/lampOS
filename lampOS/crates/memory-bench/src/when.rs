//! A record's time as the voice says it, never a raw epoch (experiment
//! version 2, 3 Nov). Runs 2, 3 and 5 showed each record only as
//! `at_unix_ms`: with thinking off the answerer converted it in its head and
//! came out two days early in 26 of 39 wrong "when" answers. The voice never
//! shows an epoch; its recall says "3 days ago, Saturday 31 October"
//! (`remember.rs` `when_said`). These helpers copy that wording.
use crate::{Result, invalid};
use lamp_memory::store::LocalTime;
use serde_json::{Value, json};

const DAYS: [&str; 7] = [
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
];
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// Year, month and day of a count of days since 1970-01-01.
fn civil(days: i64) -> (i64, usize, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_part + 2) / 5 + 1;
    let month = if month_part < 10 {
        month_part + 3
    } else {
        month_part - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month as usize, day)
}

/// How long ago, as the voice's `when` says it.
fn ago(days_ago: i64) -> String {
    match days_ago {
        i64::MIN..=0 => "today".into(),
        1 => "yesterday".into(),
        2..=6 => format!("{days_ago} days ago"),
        7..=13 => "last week".into(),
        14..=59 => format!("{} weeks ago", days_ago / 7),
        _ => format!("about {} months ago", days_ago / 30),
    }
}

/// "3 days ago, Saturday 31 October", with the year when it is not this
/// one, both moments read in `zone`. A moment after `now` is today.
pub fn said(now_unix_ms: u64, at_unix_ms: u64, zone: &LocalTime) -> String {
    let today = zone.day(now_unix_ms);
    let day = zone.day(at_unix_ms).min(today);
    let (year, month, date) = civil(day);
    let weekday = DAYS[day.rem_euclid(7) as usize];
    let named = format!("{weekday} {date} {}", MONTHS[month - 1]);
    if year == civil(today).0 {
        format!("{}, {named}", ago(today - day))
    } else {
        format!("{}, {named} {year}", ago(today - day))
    }
}

/// A `YYYY-MM-DD` day as days since 1970-01-01, as the voice reads a
/// store's `on` (`local_time::day_of`); `None` when it isn't one.
pub fn day_of(name: &str) -> Option<i64> {
    let bytes = name.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !bytes
            .iter()
            .enumerate()
            .all(|(at, byte)| at == 4 || at == 7 || byte.is_ascii_digit())
    {
        return None;
    }
    let year: i64 = name[0..4].parse().ok()?;
    let month: i64 = name[5..7].parse().ok()?;
    let day: i64 = name[8..10].parse().ok()?;
    if !(1..=12).contains(&month) || day < 1 {
        return None;
    }
    // Days from civil (Howard Hinnant), the inverse of `civil`.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    (civil(days) == (year, month as usize, day)).then_some(days)
}

/// `YYYY-MM-DD`, as the voice names a day (`remember.rs` `day_name`).
pub fn day_name(day: i64) -> String {
    let (year, month, date) = civil(day);
    format!("{year:04}-{month:02}-{date:02}")
}

/// What a line's `strength` tells the answerer, copied from the voice's
/// `remember.rs` `strength_told`: how it sounded when said, and on a past
/// day that whether it happened is not known. Any other label is not told.
pub fn strength_told(label: &str, past: bool) -> Option<&'static str> {
    match (label, past) {
        ("maybe", false) => Some("maybe: said as a maybe; not known to be decided or booked"),
        ("maybe", true) => {
            Some("maybe: said as a maybe for that day; whether it happened is not known")
        }
        ("plan", false) => Some("plan: said as a plan; not known to be done"),
        ("plan", true) => {
            Some("plan: said as a plan for that day; whether it happened is not known")
        }
        _ => None,
    }
}

/// The zone a run's persona names; refused, never UTC, when unknown.
pub fn zone(name: &str) -> Result<LocalTime> {
    LocalTime::zone(name).map_err(|_| invalid("memory timezone unavailable"))
}

/// A record object with its epoch fields replaced by `when`.
pub fn dated(record: Value, now_unix_ms: u64, zone: &LocalTime) -> Result<Value> {
    let Value::Object(mut fields) = record else {
        return Err(invalid("dated record is not an object"));
    };
    let at = fields
        .remove("at_unix_ms")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| invalid("record has no time"))?;
    // A kept thing said again carries its latest mention, as the voice reads it.
    let at = fields
        .remove("last_said_unix_ms")
        .and_then(|v| v.as_u64())
        .map_or(at, |last| last.max(at));
    let mut out = serde_json::Map::new();
    out.insert("when".into(), json!(said(now_unix_ms, at, zone)));
    out.extend(fields);
    Ok(Value::Object(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    // 2030-04-07 18:00 in Ho Chi Minh City (UTC+7), the month's "when" moment.
    const NOW: u64 = 1_901_790_000_000;
    #[test]
    fn said_is_how_long_ago_and_the_local_day() {
        let z = zone("Asia/Ho_Chi_Minh").unwrap();
        let hour = 3_600_000;
        assert_eq!(said(NOW, NOW, &z), "today, Sunday 7 April");
        assert_eq!(
            said(NOW, NOW - 24 * hour, &z),
            "yesterday, Saturday 6 April"
        );
        assert_eq!(
            said(NOW, NOW - 6 * 24 * hour, &z),
            "6 days ago, Monday 1 April"
        );
        // 00:30 local on 7 April is 17:30 UTC on 6 April: the local day counts.
        assert_eq!(
            said(NOW, NOW - 17 * hour - 30 * 60_000, &z),
            "today, Sunday 7 April"
        );
        assert_eq!(
            said(NOW, NOW - 120 * 24 * hour, &z),
            "about 4 months ago, Saturday 8 December 2029"
        );
        assert_eq!(said(NOW, NOW + 48 * hour, &z), "today, Sunday 7 April");
        assert!(zone("Nowhere/Atlantis").is_err());
    }
    #[test]
    fn dated_replaces_every_epoch_with_when() {
        let z = zone("Asia/Ho_Chi_Minh").unwrap();
        let day = 86_400_000;
        let record = json!({"at_unix_ms":NOW-3*day,"last_said_unix_ms":NOW-day,"heard":"tea"});
        let out = dated(record, NOW, &z).unwrap();
        assert_eq!(
            out,
            json!({"when":"yesterday, Saturday 6 April","heard":"tea"})
        );
        assert!(dated(json!({"heard":"tea"}), NOW, &z).is_err());
    }

    #[test]
    fn a_day_reads_back_as_the_voice_names_it() {
        for name in ["1970-01-01", "2030-04-07", "2028-02-29", "1999-12-31"] {
            assert_eq!(day_name(day_of(name).unwrap()), name);
        }
        assert_eq!(day_of("1970-01-02"), Some(1));
        for bad in [
            "2030-02-31",
            "2030-13-01",
            "2030-4-07",
            "soon",
            "2029-02-29",
        ] {
            assert_eq!(day_of(bad), None, "{bad}");
        }
    }
}
