//! Apple Health (`apple_health`): the phone app reads HealthKit and pushes
//! daily summaries to the lamp (`health.summary` in lamp-cloud), with no
//! sign-in, no token and no network. One copy of the rules, read by
//! lamp-cloud, which checks the app's request before handing it on, and by
//! this program, which checks it again and keeps it
//! (`store::Store::keep_local`, `/var/lib/lampos-services/apple_health.json`,
//! root's, 0600) until a remove, a sign-out or a reset wipes it.
//!
//! The owner's voice reads it with `service_read` (`reads::local_read`),
//! answered from the kept file alone. Each answer carries the summary's
//! `as_of`; a summary older than [`STALE_AFTER_S`] gives no values, only
//! that it is out of date. Nothing here invents a value: a day or a field
//! the phone did not send is said to be missing.
//!
//! Health values are in no journal line, error or `Debug` ([`Summary`]'s
//! shows the number of days alone), and a read's items name a day only by
//! `date`, so the voice's service record of a read keeps the date and none
//! of the numbers (`lamp_voice::service_records::found` keeps an item's
//! title, sender, date, link, id and text, none of which an item here has).
//! A made skill or a skill's run never reads it (`serve.rs`).
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{fmt, ops::RangeInclusive};

/// Its catalog code, the store's file name and the tools' `service`.
pub const CODE: &str = "apple_health";
/// The switch, read by lamp-cloud (the kind) and this program (the
/// service listed, kept and read): off unless `on`.
pub const ENVIRONMENT: &str = "LAMP_APPLE_HEALTH";
/// The app's kind on lamp-cloud's channel.
pub const KIND: &str = "health.summary";
/// The most bytes the app's `data` may take, as compact JSON.
pub const DATA_MOST: usize = 16_384;
/// The most days one summary holds.
pub const DAYS_MOST: usize = 14;
/// The most workouts one day holds.
pub const WORKOUTS_MOST: usize = 10;
/// A summary older than this, by its `as_of`, is out of date: no value of
/// it is given.
pub const STALE_AFTER_S: i64 = 48 * 3_600;
/// How far past the lamp's clock an `as_of` may be.
pub const AHEAD_MOST_S: i64 = 86_400;
/// The longest workout `type`, in letters.
pub const TYPE_MOST: usize = 40;
/// The longest `tz`, in bytes.
pub const TZ_MOST: usize = 64;
/// The steps of one day.
pub const STEPS: RangeInclusive<u32> = 0..=200_000;
/// The active kilocalories of one day.
pub const ACTIVE_KCAL: RangeInclusive<u32> = 0..=20_000;
/// A resting heart rate, in beats a minute.
pub const RESTING_HR: RangeInclusive<u32> = 20..=250;
/// Minutes asleep in one night, and a workout's minutes.
pub const MINUTES: RangeInclusive<u32> = 0..=1_440;
/// A workout's kilometres.
pub const KM_MOST: f64 = 1_000.0;
/// What a read of a week covers, in days ending today.
pub const WEEK_DAYS: u32 = 7;

/// What a read is told while the lamp keeps no summary.
pub const NO_DATA: &str = "Apple Health has no data on this lamp yet: the owner shares it from the phone app (open this lamp, then Settings, then Linked accounts). Say so; never guess a value.";
/// What a read is told for a summary past [`STALE_AFTER_S`].
pub const STALE: &str = "The Apple Health data the phone last shared is more than 48 hours old, so the lamp gives none of it: say it is out of date and that opening the app on the phone shares it again. Never guess a value.";
/// Told beside every answer that has its values.
pub const AS_OF_NOTE: &str =
    "Values are as of as_of, when the phone shared them; say that time with them.";

/// One workout.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Workout {
    /// A lowercase word, `running`, `cycling`, `traditional_strength_training`.
    #[serde(rename = "type")]
    pub kind: String,
    /// `HH:MM`, the owner's local time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub km: Option<f64>,
}

/// One day's summary; every field but `date` optional.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Day {
    /// `YYYY-MM-DD`, the owner's local date.
    pub date: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_kcal: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resting_hr: Option<u32>,
    /// Minutes asleep in the night that ended on `date`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sleep_min: Option<u32>,
    /// `HH:MM`: when that night's sleep began (often on the day before).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sleep_start: Option<String>,
    /// `HH:MM`: when it ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sleep_end: Option<String>,
    /// [`SLEEP_IN_BED`] when the night came from time in bed alone (an
    /// iPhone with no Apple Watch), so `sleep_min` is time in bed, not
    /// measured sleep; absent when the phone had asleep stages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sleep_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workouts: Option<Vec<Workout>>,
}

/// The app's summary, checked ([`fit`]): its days by date, oldest first.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    /// RFC 3339 with the phone's offset: when the phone read HealthKit.
    pub as_of: String,
    /// The owner's IANA zone, which says what "today" is.
    pub tz: String,
    pub days: Vec<Day>,
}

impl fmt::Debug for Summary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never a value: the number of days alone.
        f.debug_struct("Summary")
            .field("days", &self.days.len())
            .finish_non_exhaustive()
    }
}

/// What the store keeps: the summary and when it was kept.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Kept {
    pub connector: String,
    pub stored_at: i64,
    pub summary: Summary,
}

impl Kept {
    /// The kept file's JSON for `summary`, kept at `now`.
    pub fn record(summary: &Summary, now: i64) -> Value {
        json!({"connector": CODE, "stored_at": now, "summary": summary})
    }

    /// The kept file's JSON read back, if it is Apple Health's and fits.
    pub fn from_record(record: &Value) -> Option<Self> {
        let kept: Kept = serde_json::from_value(record.clone()).ok()?;
        (kept.connector == CODE).then_some(kept)
    }
}

/// A field's name as a refusal may say it: at most 32 letters, digits or
/// `_`, else "a field". Never a value.
fn shown(key: &str) -> &str {
    if !key.is_empty()
        && key.len() <= 32
        && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    {
        key
    } else {
        "a field"
    }
}

fn only(object: &Map<String, Value>, allowed: &[&str], at: &str) -> Result<(), String> {
    match object.keys().find(|key| !allowed.contains(&key.as_str())) {
        Some(key) => Err(format!("{at} has a field it doesn't take: {}", shown(key))),
        None => Ok(()),
    }
}

/// An optional whole number in `range`; `null` is none.
fn whole(
    object: &Map<String, Value>,
    key: &str,
    range: &RangeInclusive<u32>,
    at: &str,
) -> Result<Option<u32>, String> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| range.contains(n))
            .map(Some)
            .ok_or_else(|| {
                format!(
                    "{at}.{key} must be a whole number from {} to {}",
                    range.start(),
                    range.end()
                )
            }),
    }
}

/// Whether `text` is `HH:MM`, 00:00 to 23:59.
pub fn clock_ok(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 5
        && bytes[2] == b':'
        && [0, 1, 3, 4].iter().all(|at| bytes[*at].is_ascii_digit())
        && text[..2].parse::<u8>().is_ok_and(|hour| hour < 24)
        && text[3..].parse::<u8>().is_ok_and(|minute| minute < 60)
}

/// An optional `HH:MM`; `null` is none.
fn clock(object: &Map<String, Value>, key: &str, at: &str) -> Result<Option<String>, String> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) if clock_ok(text) => Ok(Some(text.clone())),
        Some(_) => Err(format!("{at}.{key} must be a time as HH:MM")),
    }
}

/// `text` as a calendar date, `YYYY-MM-DD` exactly.
pub fn date_of(text: &str) -> Option<jiff::civil::Date> {
    let bytes = text.as_bytes();
    let shaped = bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(at, b)| at == 4 || at == 7 || b.is_ascii_digit());
    shaped.then(|| text.parse().ok()).flatten()
}

/// Whether `text` could be an IANA zone's name (`Asia/Ho_Chi_Minh`,
/// `America/Argentina/Buenos_Aires`, `UTC`, `Etc/GMT+7`).
fn zone_name_ok(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= TZ_MOST
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/_+-".contains(&b))
        && text
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

/// `as_of` as seconds since the epoch: RFC 3339 with an offset.
pub fn as_of_secs(as_of: &str) -> Option<i64> {
    if as_of.len() > 40 {
        return None;
    }
    as_of
        .parse::<jiff::Timestamp>()
        .ok()
        .map(|at| at.as_second())
}

fn workout(value: &Value, at: &str) -> Result<Workout, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("{at} must be an object"))?;
    only(object, &["type", "start", "min", "km"], at)?;
    let kind = object
        .get("type")
        .and_then(Value::as_str)
        .filter(|kind| {
            !kind.is_empty()
                && kind.len() <= TYPE_MOST
                && kind.starts_with(|c: char| c.is_ascii_lowercase())
                && kind.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
        })
        .ok_or_else(|| {
            format!(
                "{at}.type must be one lowercase word (a to z and _), at most {TYPE_MOST} letters"
            )
        })?;
    let km = match object.get("km") {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            value
                .as_f64()
                .filter(|km| km.is_finite() && (0.0..=KM_MOST).contains(km))
                // Kept to ten metres.
                .map(|km| (km * 100.0).round() / 100.0)
                .ok_or_else(|| format!("{at}.km must be a number from 0 to {KM_MOST}"))?,
        ),
    };
    Ok(Workout {
        kind: kind.to_owned(),
        start: clock(object, "start", at)?,
        min: whole(object, "min", &MINUTES, at)?,
        km,
    })
}

/// The one `sleep_kind` the app sends: the night is time in bed only.
pub const SLEEP_IN_BED: &str = "in_bed";

/// Said with a night that is time in bed, not measured sleep.
const IN_BED_NOTE: &str = "This night is time in bed only (the phone has no asleep stages, as an iPhone without an Apple Watch): say it as time in bed, like \"about 7 hours in bed\", never as hours asleep.";

impl Day {
    /// Whether this day's night is time in bed alone ([`SLEEP_IN_BED`]).
    pub fn in_bed(&self) -> bool {
        self.sleep_kind.as_deref() == Some(SLEEP_IN_BED)
    }
}

fn day(value: &Value, at: &str) -> Result<Day, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("{at} must be an object"))?;
    only(
        object,
        &[
            "date",
            "steps",
            "active_kcal",
            "resting_hr",
            "sleep_min",
            "sleep_start",
            "sleep_end",
            "sleep_kind",
            "workouts",
        ],
        at,
    )?;
    let date = object
        .get("date")
        .and_then(Value::as_str)
        .filter(|date| date_of(date).is_some())
        .ok_or_else(|| format!("{at}.date must be a date as YYYY-MM-DD"))?;
    let workouts = match object.get("workouts") {
        None | Some(Value::Null) => None,
        Some(Value::Array(list)) if list.len() <= WORKOUTS_MOST => Some(
            list.iter()
                .enumerate()
                .map(|(n, value)| workout(value, &format!("{at}.workouts[{n}]")))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Some(_) => {
            return Err(format!(
                "{at}.workouts must be a list of at most {WORKOUTS_MOST}"
            ));
        }
    };
    Ok(Day {
        date: date.to_owned(),
        steps: whole(object, "steps", &STEPS, at)?,
        active_kcal: whole(object, "active_kcal", &ACTIVE_KCAL, at)?,
        resting_hr: whole(object, "resting_hr", &RESTING_HR, at)?,
        sleep_min: whole(object, "sleep_min", &MINUTES, at)?,
        sleep_start: clock(object, "sleep_start", at)?,
        sleep_end: clock(object, "sleep_end", at)?,
        sleep_kind: match object.get("sleep_kind") {
            None | Some(Value::Null) => None,
            Some(Value::String(kind)) if kind == SLEEP_IN_BED => Some(SLEEP_IN_BED.to_owned()),
            Some(_) => {
                return Err(format!(
                    "{at}.sleep_kind must be \"{SLEEP_IN_BED}\" or absent"
                ));
            }
        },
        workouts,
    })
}

/// The app's `data` checked strictly, at the lamp's `now`: at most
/// [`DATA_MOST`] bytes, `as_of`, `tz` and 1 to [`DAYS_MOST`] days, each
/// date once, no field the contract lacks, every number in its range. Its
/// days come back oldest first. A refusal names the field, never a value.
pub fn fit(data: &Value, now: i64) -> Result<Summary, String> {
    if data.to_string().len() > DATA_MOST {
        return Err(format!(
            "health.summary is over {DATA_MOST} bytes: send at most {DAYS_MOST} days"
        ));
    }
    let object = data
        .as_object()
        .ok_or("health.summary's data must be an object, or null to disconnect")?;
    only(object, &["as_of", "tz", "days"], "data")?;
    let as_of = object
        .get("as_of")
        .and_then(Value::as_str)
        .ok_or("data.as_of must be a time in RFC 3339 with its offset")?;
    let at = as_of_secs(as_of).ok_or("data.as_of must be a time in RFC 3339 with its offset")?;
    if at > now + AHEAD_MOST_S {
        return Err("data.as_of is more than a day after the lamp's clock".into());
    }
    let tz = object
        .get("tz")
        .and_then(Value::as_str)
        .filter(|tz| zone_name_ok(tz))
        .ok_or("data.tz must be an IANA zone, such as Asia/Ho_Chi_Minh")?;
    let days = match object.get("days") {
        Some(Value::Array(days)) if !days.is_empty() && days.len() <= DAYS_MOST => days,
        _ => return Err(format!("data.days must be a list of 1 to {DAYS_MOST} days")),
    };
    let mut days = days
        .iter()
        .enumerate()
        .map(|(n, value)| day(value, &format!("data.days[{n}]")))
        .collect::<Result<Vec<_>, _>>()?;
    days.sort_by(|a, b| a.date.cmp(&b.date));
    if days.windows(2).any(|pair| pair[0].date == pair[1].date) {
        return Err("data.days names one date twice".into());
    }
    Ok(Summary {
        as_of: as_of.to_owned(),
        tz: tz.to_owned(),
        days,
    })
}

/// The owner's zone: the summary's `tz`, else the offset its `as_of` has.
fn zone(summary: &Summary) -> jiff::tz::TimeZone {
    if let Ok(zone) = jiff::tz::TimeZone::get(&summary.tz) {
        return zone;
    }
    let offset = summary
        .as_of
        .get(summary.as_of.len().saturating_sub(6)..)
        .filter(|tail| tail.starts_with(['+', '-']) && clock_ok(&tail[1..]))
        .map_or(0, |tail| {
            let minutes =
                tail[1..3].parse::<i32>().unwrap_or(0) * 60 + tail[4..].parse::<i32>().unwrap_or(0);
            if tail.starts_with('-') {
                -minutes
            } else {
                minutes
            }
        });
    jiff::tz::Offset::from_seconds(offset * 60)
        .map(jiff::tz::TimeZone::fixed)
        .unwrap_or(jiff::tz::TimeZone::UTC)
}

/// The owner's date now, in their zone.
pub fn today(summary: &Summary, now: i64) -> jiff::civil::Date {
    jiff::Timestamp::from_second(now)
        .unwrap_or(jiff::Timestamp::UNIX_EPOCH)
        .to_zoned(zone(summary))
        .date()
}

/// `minutes` as said: `6 h 52 min`, `45 min`.
pub fn duration(minutes: u32) -> String {
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

/// A read's `date`: `YYYY-MM-DD`, `today` or `yesterday`; none when not
/// given.
pub fn asked_date(
    args: &Value,
    today: jiff::civil::Date,
) -> Result<Option<jiff::civil::Date>, String> {
    let Some(text) = args
        .get("date")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
    else {
        return Ok(None);
    };
    match text {
        "today" => Ok(Some(today)),
        "yesterday" => Ok(today.yesterday().ok()),
        text => date_of(text)
            .map(Some)
            .ok_or_else(|| "date is a day as YYYY-MM-DD, today or yesterday".to_owned()),
    }
}

/// A read's `days`: 1 to [`DAYS_MOST`]; none when not given.
pub fn asked_days(args: &Value) -> Result<Option<u32>, String> {
    match args.get("days") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| (1..=DAYS_MOST as u32).contains(n))
            .map(Some)
            .ok_or_else(|| format!("days is a whole number from 1 to {DAYS_MOST}")),
    }
}

/// The answer's frame: quoted data from `name`, its `as_of` and, when it
/// has none, why.
fn framed(name: &str, summary: &Summary, items: Vec<Value>, note: Option<String>) -> Value {
    let mut answer = json!({
        "source": name,
        "about": crate::reads::about(name),
        "as_of": summary.as_of,
        "tz": summary.tz,
        "quoted": items,
        "note": note.unwrap_or_else(|| AS_OF_NOTE.to_owned()),
    });
    if answer["quoted"].as_array().is_some_and(Vec::is_empty) {
        answer["found"] = json!("none");
    }
    answer
}

/// The days of `summary` from `first` to `last`, both kept, oldest first.
fn between(summary: &Summary, first: jiff::civil::Date, last: jiff::civil::Date) -> Vec<&Day> {
    summary
        .days
        .iter()
        .filter(|day| date_of(&day.date).is_some_and(|date| date >= first && date <= last))
        .collect()
}

/// The `n` days ending `last`, as their first day.
fn first_of(last: jiff::civil::Date, n: u32) -> jiff::civil::Date {
    last.checked_sub(jiff::Span::new().days(i64::from(n.saturating_sub(1))))
        .unwrap_or(last)
}

fn day_named(summary: &Summary, date: jiff::civil::Date) -> Option<&Day> {
    summary
        .days
        .iter()
        .find(|day| date_of(&day.date) == Some(date))
}

/// The kept summary, or why the lamp gives none of it: none kept, or one
/// past [`STALE_AFTER_S`] (answered, with no value, as [`STALE`]).
pub fn usable(name: &str, kept: Option<&Kept>, now: i64) -> Result<Result<Summary, Value>, String> {
    let summary = &kept.ok_or(NO_DATA)?.summary;
    match as_of_secs(&summary.as_of) {
        Some(at) if now - at <= STALE_AFTER_S => Ok(Ok(summary.clone())),
        _ => {
            let mut answer = framed(name, summary, Vec::new(), Some(STALE.to_owned()));
            answer["stale"] = json!(true);
            Ok(Err(answer))
        }
    }
}

/// `sleep {date}`: the night that ended on `date`, by default the latest
/// night recorded (last night when the phone has shared it).
pub fn sleep(name: &str, summary: &Summary, args: &Value, now: i64) -> Result<Value, String> {
    let today = today(summary, now);
    let asked = asked_date(args, today)?;
    let night = match asked {
        Some(date) => day_named(summary, date).filter(|day| day.sleep_min.is_some()),
        None => summary
            .days
            .iter()
            .rev()
            .find(|day| day.sleep_min.is_some()),
    };
    let Some(day) = night else {
        let when = asked.map_or_else(
            || "any night".to_owned(),
            |date| format!("the night ending {date}"),
        );
        return Ok(framed(
            name,
            summary,
            Vec::new(),
            Some(format!(
                "The phone shared no sleep for {when}. Say so; never guess a value."
            )),
        ));
    };
    let asleep = day.sleep_min.unwrap_or(0);
    let mut item = if day.in_bed() {
        json!({
            "date": day.date,
            "in_bed": duration(asleep),
            "in_bed_min": asleep,
            "sleep_kind": SLEEP_IN_BED,
        })
    } else {
        json!({
            "date": day.date,
            "asleep": duration(asleep),
            "asleep_min": asleep,
        })
    };
    if let Some(start) = &day.sleep_start {
        item["fell_asleep"] = json!(start);
    }
    if let Some(end) = &day.sleep_end {
        item["woke"] = json!(end);
    }
    let mut note = (asked.is_none() && date_of(&day.date) != Some(today)).then(|| {
        format!(
            "No sleep is shared for last night yet: this is the latest night recorded, which ended on {}. {AS_OF_NOTE}",
            day.date
        )
    });
    if day.in_bed() {
        note = Some(format!(
            "{IN_BED_NOTE} {}",
            note.as_deref().unwrap_or(AS_OF_NOTE)
        ));
    }
    Ok(framed(name, summary, vec![item], note))
}

/// `steps {date, days}`: one day's steps (today by default; today's count
/// runs on after `as_of`), or the total of the `days` ending today.
pub fn steps(name: &str, summary: &Summary, args: &Value, now: i64) -> Result<Value, String> {
    let today = today(summary, now);
    if let Some(n) = asked_days(args)? {
        let first = first_of(today, n);
        let counted: Vec<(&str, u32)> = between(summary, first, today)
            .into_iter()
            .filter_map(|day| day.steps.map(|steps| (day.date.as_str(), steps)))
            .collect();
        if counted.is_empty() {
            return Ok(framed(
                name,
                summary,
                Vec::new(),
                Some(format!(
                    "The phone shared no steps for the {n} days from {first} to {today}. Say so; never guess a value."
                )),
            ));
        }
        let total: u64 = counted.iter().map(|(_, steps)| u64::from(*steps)).sum();
        let average = total / counted.len() as u64;
        let item = json!({
            "period": format!("{first} to {today}"),
            "days_asked": n,
            "days_with_steps": counted.len(),
            "steps_total": total,
            "steps_daily_average": average,
        });
        let note = (counted.len() < n as usize).then(|| {
            format!(
                "Only {} of the {n} days have steps shared; the total counts those alone. {AS_OF_NOTE}",
                counted.len()
            )
        });
        return Ok(framed(name, summary, vec![item], note));
    }
    let date = asked_date(args, today)?.unwrap_or(today);
    match day_named(summary, date).and_then(|day| day.steps) {
        Some(steps) => {
            let note = (date == today).then(|| {
                format!("Today's steps run on after as_of: say them as so far. {AS_OF_NOTE}")
            });
            Ok(framed(
                name,
                summary,
                vec![json!({"date": date.to_string(), "steps": steps})],
                note,
            ))
        }
        None => Ok(framed(
            name,
            summary,
            Vec::new(),
            Some(format!(
                "The phone shared no steps for {date}. Say so; never guess a value."
            )),
        )),
    }
}

/// `heart {date}`: the resting heart rate of `date`, by default the latest
/// day that has one.
pub fn heart(name: &str, summary: &Summary, args: &Value, now: i64) -> Result<Value, String> {
    let today = today(summary, now);
    let asked = asked_date(args, today)?;
    let found = match asked {
        Some(date) => day_named(summary, date).filter(|day| day.resting_hr.is_some()),
        None => summary
            .days
            .iter()
            .rev()
            .find(|day| day.resting_hr.is_some()),
    };
    let Some(day) = found else {
        let when = asked.map_or_else(|| "any day".to_owned(), |date| date.to_string());
        return Ok(framed(
            name,
            summary,
            Vec::new(),
            Some(format!(
                "The phone shared no resting heart rate for {when}. Say so; never guess a value."
            )),
        ));
    };
    let item = json!({
        "date": day.date,
        "resting_heart_rate_bpm": day.resting_hr,
    });
    Ok(framed(name, summary, vec![item], None))
}

fn workout_item(date: &str, workout: &Workout) -> Value {
    let mut item = json!({"date": date, "type": workout.kind});
    if let Some(start) = &workout.start {
        item["started"] = json!(start);
    }
    if let Some(min) = workout.min {
        item["minutes"] = json!(min);
    }
    if let Some(km) = workout.km {
        item["km"] = json!(km);
    }
    item
}

/// `workouts {days}`: the workouts of the `days` ending today, newest
/// first, at most [`WORKOUTS_MOST`]; without `days`, the latest one.
pub fn workouts(name: &str, summary: &Summary, args: &Value, now: i64) -> Result<Value, String> {
    let today = today(summary, now);
    let days = asked_days(args)?;
    let first = days.map_or(jiff::civil::Date::MIN, |n| first_of(today, n));
    let mut all: Vec<Value> = between(summary, first, today)
        .into_iter()
        .rev()
        .flat_map(|day| {
            day.workouts
                .iter()
                .flatten()
                .rev()
                .map(|workout| workout_item(&day.date, workout))
        })
        .collect();
    let found = all.len();
    all.truncate(if days.is_some() { WORKOUTS_MOST } else { 1 });
    if all.is_empty() {
        let when = days.map_or_else(
            || "any day".to_owned(),
            |n| format!("the {n} days from {first} to {today}"),
        );
        return Ok(framed(
            name,
            summary,
            Vec::new(),
            Some(format!(
                "The phone shared no workouts for {when}. Say so; never guess one."
            )),
        ));
    }
    let note = (days.is_some() && found > all.len()).then(|| {
        format!(
            "{found} workouts in all; the {} newest are here. {AS_OF_NOTE}",
            all.len()
        )
    });
    Ok(framed(name, summary, all, note))
}

/// `week`: each of the 7 days ending today the phone shared, and their
/// totals and averages over the days that have each value.
pub fn week(name: &str, summary: &Summary, now: i64) -> Result<Value, String> {
    let today = today(summary, now);
    let first = first_of(today, WEEK_DAYS);
    let days = between(summary, first, today);
    if days.is_empty() {
        return Ok(framed(
            name,
            summary,
            Vec::new(),
            Some(format!(
                "The phone shared nothing for the week from {first} to {today}. Say so; never guess a value."
            )),
        ));
    }
    let items: Vec<Value> = days
        .iter()
        .map(|day| {
            let mut item = json!({"date": day.date});
            if let Some(steps) = day.steps {
                item["steps"] = json!(steps);
            }
            if let Some(asleep) = day.sleep_min {
                let key = if day.in_bed() { "in_bed" } else { "asleep" };
                item[key] = json!(duration(asleep));
            }
            if let Some(hr) = day.resting_hr {
                item["resting_heart_rate_bpm"] = json!(hr);
            }
            if let Some(kcal) = day.active_kcal {
                item["active_kcal"] = json!(kcal);
            }
            let workouts = day.workouts.as_ref().map_or(0, Vec::len);
            if workouts > 0 {
                item["workouts"] = json!(workouts);
            }
            item
        })
        .collect();
    let average = |values: Vec<u32>| -> Option<u64> {
        (!values.is_empty())
            .then(|| values.iter().map(|v| u64::from(*v)).sum::<u64>() / values.len() as u64)
    };
    let steps: Vec<u32> = days.iter().filter_map(|day| day.steps).collect();
    // Measured nights and time in bed are averaged apart, never mixed.
    let sleep: Vec<u32> = days
        .iter()
        .filter(|day| !day.in_bed())
        .filter_map(|day| day.sleep_min)
        .collect();
    let in_bed: Vec<u32> = days
        .iter()
        .filter(|day| day.in_bed())
        .filter_map(|day| day.sleep_min)
        .collect();
    let hr: Vec<u32> = days.iter().filter_map(|day| day.resting_hr).collect();
    let mut totals = json!({
        "period": format!("{first} to {today}"),
        "days_shared": days.len(),
        "workouts": days.iter().map(|day| day.workouts.as_ref().map_or(0, Vec::len)).sum::<usize>(),
    });
    if !steps.is_empty() {
        totals["steps_total"] = json!(steps.iter().map(|v| u64::from(*v)).sum::<u64>());
        totals["steps_daily_average"] = json!(average(steps));
    }
    if let Some(asleep) = average(sleep) {
        totals["asleep_nightly_average"] = json!(duration(asleep as u32));
    }
    let any_in_bed = !in_bed.is_empty();
    if let Some(in_bed) = average(in_bed) {
        totals["in_bed_nightly_average"] = json!(duration(in_bed as u32));
    }
    if let Some(hr) = average(hr) {
        totals["resting_heart_rate_average_bpm"] = json!(hr);
    }
    let note = any_in_bed.then(|| {
        format!("Nights with in_bed are time in bed only, not measured sleep: say them as time in bed. {AS_OF_NOTE}")
    });
    let mut answer = framed(name, summary, items, note);
    answer["week"] = totals;
    Ok(answer)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// 2026-11-18T07:30:00+07:00.
    pub const AS_OF: i64 = 1_794_961_800;

    pub fn contract() -> Value {
        json!({
            "as_of": "2026-11-18T07:30:00+07:00", "tz": "Asia/Ho_Chi_Minh",
            "days": [{"date": "2026-11-18",
                "steps": 8421, "active_kcal": 412, "resting_hr": 58,
                "sleep_min": 412, "sleep_start": "23:48", "sleep_end": "06:40",
                "workouts": [{"type": "running", "start": "06:55", "min": 32, "km": 5.1}]}]
        })
    }

    /// An iPhone with no Apple Watch has time in bed, not asleep stages:
    /// the app says `"sleep_kind": "in_bed"`, nothing else is taken, and
    /// the reads say it is time in bed.
    #[test]
    fn a_night_in_bed_is_said_as_time_in_bed() {
        let mut data = contract();
        data["days"][0]["sleep_kind"] = json!("in_bed");
        let summary = fit(&data, AS_OF).unwrap();
        assert!(summary.days[0].in_bed());
        assert_eq!(serde_json::to_value(&summary).unwrap(), data);
        for wrong in [
            json!("asleep"),
            json!("IN_BED"),
            json!(""),
            json!(1),
            json!(true),
        ] {
            let mut data = contract();
            data["days"][0]["sleep_kind"] = wrong;
            let refused = fit(&data, AS_OF).unwrap_err();
            assert_eq!(
                refused,
                "data.days[0].sleep_kind must be \"in_bed\" or absent"
            );
        }
        let mut absent = contract();
        absent["days"][0]["sleep_kind"] = Value::Null;
        assert!(!fit(&absent, AS_OF).unwrap().days[0].in_bed());

        let answer = sleep(CODE, &summary, &json!({}), AS_OF).unwrap();
        assert_eq!(
            answer["quoted"],
            json!([{"date": "2026-11-18", "in_bed": "6 h 52 min", "in_bed_min": 412,
                "sleep_kind": "in_bed", "fell_asleep": "23:48", "woke": "06:40"}])
        );
        assert!(answer["quoted"][0].get("asleep").is_none());
        let note = answer["note"].as_str().unwrap();
        assert!(
            note.contains("time in bed") && note.contains("never as hours asleep"),
            "{note}"
        );
        // A measured night keeps its words.
        let measured = fit(&contract(), AS_OF).unwrap();
        let answer = sleep(CODE, &measured, &json!({}), AS_OF).unwrap();
        assert_eq!(answer["quoted"][0]["asleep"], "6 h 52 min");
        assert!(!answer["note"].as_str().unwrap().contains("in bed"));
        // The week keeps the two apart.
        let mut two = contract();
        two["days"] = json!([
            {"date": "2026-11-17", "sleep_min": 420},
            {"date": "2026-11-18", "sleep_min": 480, "sleep_kind": "in_bed"}
        ]);
        let answer = week(CODE, &fit(&two, AS_OF).unwrap(), AS_OF).unwrap();
        assert_eq!(answer["quoted"][1]["in_bed"], "8 h");
        assert_eq!(answer["week"]["asleep_nightly_average"], "7 h");
        assert_eq!(answer["week"]["in_bed_nightly_average"], "8 h");
        assert!(answer["note"].as_str().unwrap().contains("time in bed"));
    }

    #[test]
    fn the_contract_fits_and_comes_back_whole() {
        assert_eq!(
            jiff::Timestamp::from_second(AS_OF).unwrap().to_string(),
            "2026-11-18T00:30:00Z"
        );
        let summary = fit(&contract(), AS_OF).unwrap();
        assert_eq!(serde_json::to_value(&summary).unwrap(), contract());
        // Only the date is needed in a day; nulls are absent fields.
        let bare = json!({"as_of": "2026-11-18T07:30:00+07:00", "tz": "UTC",
            "days": [{"date": "2026-11-17", "steps": null, "workouts": null}, {"date": "2026-11-16"}]});
        let summary = fit(&bare, AS_OF).unwrap();
        assert_eq!(summary.days[0].date, "2026-11-16");
        assert_eq!(
            summary.days[1],
            Day {
                date: "2026-11-17".into(),
                ..Day::default()
            }
        );
        // Its Debug says no value.
        let shown = format!("{summary:?} {:?}", fit(&contract(), AS_OF).unwrap());
        assert!(
            !shown.contains("8421") && !shown.contains("running"),
            "{shown}"
        );
    }

    #[test]
    fn a_summary_that_does_not_fit_is_refused_by_its_field_alone() {
        let with = |path: &[&str], value: Value| {
            let mut data = contract();
            let mut at = &mut data;
            for key in &path[..path.len() - 1] {
                at = match key.parse::<usize>() {
                    Ok(n) => &mut at[n],
                    Err(_) => &mut at[*key],
                };
            }
            at[path[path.len() - 1]] = value;
            data
        };
        for (data, field) in [
            (
                with(&["planted"], json!(1)),
                "data has a field it doesn't take: planted",
            ),
            (
                with(&["days", "0", "spo2"], json!(97)),
                "data.days[0] has a field",
            ),
            (
                with(&["days", "0", "workouts", "0", "kcal"], json!(9)),
                "data.days[0].workouts[0] has a field",
            ),
            (
                with(&["days", "0", "steps"], json!(-1)),
                "data.days[0].steps",
            ),
            (
                with(&["days", "0", "steps"], json!(200_001)),
                "data.days[0].steps",
            ),
            (
                with(&["days", "0", "steps"], json!(84.5)),
                "data.days[0].steps",
            ),
            (
                with(&["days", "0", "steps"], json!("8421")),
                "data.days[0].steps",
            ),
            (
                with(&["days", "0", "resting_hr"], json!(19)),
                "data.days[0].resting_hr",
            ),
            (
                with(&["days", "0", "resting_hr"], json!(251)),
                "data.days[0].resting_hr",
            ),
            (
                with(&["days", "0", "active_kcal"], json!(20_001)),
                "data.days[0].active_kcal",
            ),
            (
                with(&["days", "0", "sleep_min"], json!(1_441)),
                "data.days[0].sleep_min",
            ),
            (
                with(&["days", "0", "sleep_start"], json!("24:00")),
                "data.days[0].sleep_start",
            ),
            (
                with(&["days", "0", "sleep_end"], json!("6:40")),
                "data.days[0].sleep_end",
            ),
            (
                with(&["days", "0", "date"], json!("2026-02-30")),
                "data.days[0].date",
            ),
            (
                with(&["days", "0", "date"], json!("18/11/2026")),
                "data.days[0].date",
            ),
            (
                with(&["days", "0", "workouts", "0", "type"], json!("Running")),
                "workouts[0].type",
            ),
            (
                with(&["days", "0", "workouts", "0", "type"], json!("running 5k")),
                "workouts[0].type",
            ),
            (
                with(&["days", "0", "workouts", "0", "type"], json!("")),
                "workouts[0].type",
            ),
            (
                with(&["days", "0", "workouts", "0", "km"], json!(1_000.5)),
                "workouts[0].km",
            ),
            (
                with(&["days", "0", "workouts", "0", "min"], json!(1_441)),
                "workouts[0].min",
            ),
            (
                with(
                    &["days", "0", "workouts"],
                    Value::Array(vec![json!({"type": "yoga"}); 11]),
                ),
                "data.days[0].workouts",
            ),
            (with(&["as_of"], json!("2026-11-18T07:30:00")), "data.as_of"),
            (
                with(&["as_of"], json!("2026-11-20T07:30:00+07:00")),
                "data.as_of",
            ),
            (with(&["tz"], json!("../etc/passwd")), "data.tz"),
            (with(&["days"], json!([])), "data.days"),
            (json!([]), "data must be an object"),
        ] {
            let why = fit(&data, AS_OF).unwrap_err();
            assert!(why.contains(field), "{why} for {field}");
            // A refusal never repeats a value.
            for value in ["8421", "200001", "Running", "passwd", "planted-4710"] {
                assert!(!why.contains(value) || field.contains(value), "{why}");
            }
        }
        // Fifteen days, or one date twice.
        let days: Vec<Value> = (1..=15)
            .map(|d| json!({"date": format!("2026-11-{d:02}")}))
            .collect();
        let too_many = json!({"as_of": "2026-11-18T07:30:00+07:00", "tz": "UTC", "days": days});
        assert!(fit(&too_many, AS_OF).unwrap_err().contains("1 to 14 days"));
        let twice = json!({"as_of": "2026-11-18T07:30:00+07:00", "tz": "UTC",
            "days": [{"date": "2026-11-18"}, {"date": "2026-11-18"}]});
        assert!(fit(&twice, AS_OF).unwrap_err().contains("twice"));
        // Over 16 KB: refused for its size before anything else is read.
        let day = json!({"date": "2026-11-18", "workouts": Value::Array(vec![
            json!({"type": "running", "start": "06:55", "min": 32, "km": 5.1}); 30])});
        let big = json!({"as_of": "2026-11-18T07:30:00+07:00", "tz": "UTC",
            "days": Value::Array(vec![day; 14])});
        assert!(big.to_string().len() > DATA_MOST);
        assert!(fit(&big, AS_OF).unwrap_err().contains("over 16384 bytes"));
        // A field's name the app made up is said only when it is plain.
        let odd = with(&["x\"y z"], json!(1));
        assert!(fit(&odd, AS_OF).unwrap_err().ends_with("a field"));
    }

    fn kept(data: Value) -> Kept {
        Kept::from_record(&Kept::record(&fit(&data, AS_OF).unwrap(), AS_OF)).unwrap()
    }

    #[test]
    fn today_is_the_owners_date_in_their_zone() {
        let summary = fit(&contract(), AS_OF).unwrap();
        // 00:30 UTC is 07:30 in Ho Chi Minh City.
        assert_eq!(today(&summary, AS_OF).to_string(), "2026-11-18");
        let mut far = summary.clone();
        far.tz = "America/Los_Angeles".into();
        assert_eq!(today(&far, AS_OF).to_string(), "2026-11-17");
        // A zone the lamp doesn't know: the offset as_of was written with.
        let mut unknown = summary;
        unknown.tz = "Nowhere/Planted".into();
        assert_eq!(today(&unknown, AS_OF - 3 * 3_600).to_string(), "2026-11-18");
        assert_eq!(duration(412), "6 h 52 min");
        assert_eq!(duration(45), "45 min");
        assert_eq!(duration(120), "2 h");
    }

    #[test]
    fn no_data_and_old_data_give_no_value() {
        assert_eq!(usable("Apple Health", None, AS_OF).unwrap_err(), NO_DATA);
        let kept = kept(contract());
        assert!(
            usable("Apple Health", Some(&kept), AS_OF + STALE_AFTER_S)
                .unwrap()
                .is_ok()
        );
        let stale = usable("Apple Health", Some(&kept), AS_OF + STALE_AFTER_S + 1)
            .unwrap()
            .unwrap_err();
        assert_eq!(stale["stale"], true);
        assert_eq!(stale["quoted"], json!([]));
        assert_eq!(stale["as_of"], "2026-11-18T07:30:00+07:00");
        assert!(!stale.to_string().contains("8421"));
    }

    /// A week as the phone shares it: today (the 18th) with last night's
    /// sleep and a run, the 17th with no sleep, the 16th with a ride, and
    /// older days, the 10th outside the week.
    fn week_kept() -> Kept {
        kept(json!({
            "as_of": "2026-11-18T07:30:00+07:00", "tz": "Asia/Ho_Chi_Minh",
            "days": [
                {"date": "2026-11-10", "steps": 99_999, "sleep_min": 600, "resting_hr": 70,
                    "workouts": [{"type": "swimming", "min": 40}]},
                {"date": "2026-11-14", "steps": 6000, "sleep_min": 420, "resting_hr": 60},
                {"date": "2026-11-16", "steps": 10000, "sleep_min": 450, "active_kcal": 500,
                    "workouts": [{"type": "cycling", "start": "17:10", "min": 60, "km": 20.25}]},
                {"date": "2026-11-17", "steps": 7000, "resting_hr": 57},
                {"date": "2026-11-18", "steps": 8421, "active_kcal": 412,
                    "sleep_min": 412, "sleep_start": "23:48", "sleep_end": "06:40",
                    "workouts": [{"type": "running", "start": "06:55", "min": 32, "km": 5.1}]}
            ]
        }))
    }

    fn read(what: &str, args: Value, kept: Option<&Kept>, now: i64) -> Result<Value, String> {
        let mut args = args;
        args["what"] = json!(what);
        crate::reads::local_read(CODE, &args, kept, now)
    }

    /// An hour after the phone shared it.
    const NOW: i64 = AS_OF + 3_600;

    #[test]
    fn each_read_answers_from_the_kept_summary_with_its_as_of() {
        let kept = week_kept();
        let kept = Some(&kept);
        // Last night's sleep, by default and by date.
        let sleep = read("sleep", json!({}), kept, NOW).unwrap();
        assert_eq!(sleep["source"], "Apple Health");
        assert_eq!(sleep["as_of"], "2026-11-18T07:30:00+07:00");
        assert_eq!(
            sleep["quoted"],
            json!([{"date": "2026-11-18", "asleep": "6 h 52 min", "asleep_min": 412,
                "fell_asleep": "23:48", "woke": "06:40"}])
        );
        assert_eq!(sleep["note"], AS_OF_NOTE);
        let on_16 = read("sleep", json!({"date": "2026-11-16"}), kept, NOW).unwrap();
        assert_eq!(on_16["quoted"][0]["asleep"], "7 h 30 min");
        assert_eq!(
            read("sleep", json!({"date": "yesterday"}), kept, NOW).unwrap()["found"],
            "none"
        );
        // Steps today (so far), a date, and a week's total over the days
        // that have them (the 10th is outside it).
        let today = read("steps", json!({}), kept, NOW).unwrap();
        assert_eq!(
            today["quoted"],
            json!([{"date": "2026-11-18", "steps": 8421}])
        );
        assert!(today["note"].as_str().unwrap().contains("so far"));
        let on_17 = read("steps", json!({"date": "2026-11-17"}), kept, NOW).unwrap();
        assert_eq!(on_17["quoted"][0]["steps"], 7000);
        let week_steps = read("steps", json!({"days": 7}), kept, NOW).unwrap();
        assert_eq!(
            week_steps["quoted"],
            json!([{"period": "2026-11-12 to 2026-11-18", "days_asked": 7, "days_with_steps": 4,
                "steps_total": 31_421, "steps_daily_average": 7_855}])
        );
        assert!(
            week_steps["note"]
                .as_str()
                .unwrap()
                .contains("Only 4 of the 7 days")
        );
        // The latest resting heart rate, and one asked by date.
        let heart = read("heart", json!({}), kept, NOW).unwrap();
        assert_eq!(
            heart["quoted"],
            json!([{"date": "2026-11-17", "resting_heart_rate_bpm": 57}])
        );
        let on_14 = read("heart", json!({"date": "2026-11-14"}), kept, NOW).unwrap();
        assert_eq!(on_14["quoted"][0]["resting_heart_rate_bpm"], 60);
        // The latest workout; within 3 days, newest first; within 14, all.
        let latest = read("workouts", json!({}), kept, NOW).unwrap();
        assert_eq!(
            latest["quoted"],
            json!([{"date": "2026-11-18", "type": "running", "started": "06:55",
                "minutes": 32, "km": 5.1}])
        );
        let three = read("workouts", json!({"days": 3}), kept, NOW).unwrap();
        let types: Vec<&str> = three["quoted"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["type"].as_str().unwrap())
            .collect();
        assert_eq!(types, ["running", "cycling"]);
        assert_eq!(three["quoted"][1]["km"], 20.25);
        let all = read("workouts", json!({"days": 14}), kept, NOW).unwrap();
        assert_eq!(all["quoted"].as_array().unwrap().len(), 3);
        assert_eq!(
            read(
                "workouts",
                json!({"days": 1, "date": "2026-11-17"}),
                kept,
                NOW
            )
            .unwrap()["quoted"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        // The week: each day shared in it, and its totals.
        let week = read("week", json!({}), kept, NOW).unwrap();
        let dates: Vec<&str> = week["quoted"]
            .as_array()
            .unwrap()
            .iter()
            .map(|day| day["date"].as_str().unwrap())
            .collect();
        assert_eq!(
            dates,
            ["2026-11-14", "2026-11-16", "2026-11-17", "2026-11-18"]
        );
        assert_eq!(
            week["week"],
            json!({"period": "2026-11-12 to 2026-11-18", "days_shared": 4, "workouts": 2,
                "steps_total": 31_421, "steps_daily_average": 7_855,
                "asleep_nightly_average": "7 h 7 min", "resting_heart_rate_average_bpm": 58})
        );
        assert!(week.to_string().len() < crate::reads::RESULT_MOST);
    }

    #[test]
    fn a_missing_night_or_day_is_said_missing_never_guessed() {
        let kept = kept(json!({
            "as_of": "2026-11-18T07:30:00+07:00", "tz": "Asia/Ho_Chi_Minh",
            "days": [{"date": "2026-11-16", "sleep_min": 450}, {"date": "2026-11-18"}]
        }));
        let kept = Some(&kept);
        // No sleep shared for last night: the latest night, said as such.
        let sleep = read("sleep", json!({}), kept, NOW).unwrap();
        assert_eq!(sleep["quoted"][0]["date"], "2026-11-16");
        assert!(
            sleep["note"]
                .as_str()
                .unwrap()
                .contains("No sleep is shared for last night")
        );
        for (what, args) in [
            ("steps", json!({})),
            ("steps", json!({"days": 7})),
            ("steps", json!({"date": "2026-11-01"})),
            ("heart", json!({})),
            ("workouts", json!({})),
            ("workouts", json!({"days": 14})),
            ("sleep", json!({"date": "2026-11-18"})),
        ] {
            let answer = read(what, args.clone(), kept, NOW).unwrap();
            assert_eq!(answer["quoted"], json!([]), "{what} {args}");
            assert_eq!(answer["found"], "none", "{what} {args}");
            assert!(
                answer["note"].as_str().unwrap().contains("never guess"),
                "{what}"
            );
            assert_eq!(answer["as_of"], "2026-11-18T07:30:00+07:00");
        }
        let week = read("week", json!({}), kept, NOW).unwrap();
        assert_eq!(
            week["quoted"],
            json!([{"date": "2026-11-16", "asleep": "7 h 30 min"}, {"date": "2026-11-18"}])
        );
        assert_eq!(week["week"]["days_shared"], 2);
        assert!(week["week"].get("steps_total").is_none());
        assert!(week["week"].get("resting_heart_rate_average_bpm").is_none());
    }

    #[test]
    fn no_data_stale_data_and_bad_asks_are_told_plainly() {
        // None kept: every read is refused with why.
        for what in ["sleep", "steps", "heart", "workouts", "week"] {
            assert_eq!(read(what, json!({}), None, NOW).unwrap_err(), NO_DATA);
        }
        // Past 48 hours: no value, only that it is out of date.
        let kept = week_kept();
        for what in ["sleep", "steps", "heart", "workouts", "week"] {
            let stale = read(what, json!({}), Some(&kept), AS_OF + STALE_AFTER_S + 60).unwrap();
            assert_eq!(stale["stale"], true, "{what}");
            assert_eq!(stale["quoted"], json!([]), "{what}");
            assert_eq!(stale["note"], STALE);
            assert!(!stale.to_string().contains("8421"));
        }
        // A kind it doesn't read, a bad date, a bad count of days.
        let other = read("calories", json!({}), Some(&kept), NOW).unwrap_err();
        assert!(crate::reads::is_no_such_kind(&other), "{other}");
        assert!(
            other.contains("sleep, steps, heart, workouts, week"),
            "{other}"
        );
        let date = read("sleep", json!({"date": "last tuesday"}), Some(&kept), NOW).unwrap_err();
        assert!(date.starts_with("date is a day"), "{date}");
        for days in [json!(0), json!(15), json!("7"), json!(2.5)] {
            let why = read("steps", json!({"days": days}), Some(&kept), NOW).unwrap_err();
            assert!(why.starts_with("days is a whole number"), "{why}");
        }
    }

    /// What the voice's service record keeps of a read (an item's title,
    /// sender, date, link, id and text, `lamp_voice::service_records::found`)
    /// holds no health value: an item names its day by `date` alone.
    #[test]
    fn a_reads_items_have_no_field_a_service_record_keeps_but_the_date() {
        const RECORDED: &[&str] = &[
            "title",
            "subject",
            "name",
            "summary",
            "from",
            "sender",
            "organizer",
            "owner",
            "author",
            "start",
            "modifiedTime",
            "updated",
            "created",
            "createdTime",
            "link",
            "url",
            "webViewLink",
            "htmlLink",
            "html_url",
            "id",
            "text",
            "snippet",
            "description",
            "body",
            "content",
        ];
        let kept = week_kept();
        for (what, args) in [
            ("sleep", json!({})),
            ("steps", json!({})),
            ("steps", json!({"days": 7})),
            ("heart", json!({})),
            ("workouts", json!({"days": 14})),
            ("week", json!({})),
        ] {
            let answer = read(what, args, Some(&kept), NOW).unwrap();
            for item in answer["quoted"].as_array().unwrap() {
                for key in item.as_object().unwrap().keys() {
                    assert!(!RECORDED.contains(&key.as_str()), "{what}: {key}");
                }
            }
        }
    }
}
