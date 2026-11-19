//! Apple Health (`apple_health`): the phone app reads HealthKit and pushes
//! daily summaries to the lamp (`health.summary` in lamp-cloud), with no
//! sign-in, no token, and nothing fetched from the network. One copy of the rules, read by
//! lamp-cloud, which checks the app's request before handing it on, and by
//! this program, which checks it again and keeps it
//! (`store::Store::keep_local`, `/var/lib/lampos-services/apple_health.json`,
//! root's, 0600) until a remove, a sign-out or a reset wipes it.
//!
//! The owner reads it with `service_read` (`reads::local_read`), answered
//! from the kept file alone, and only when the eyes see the owner's own
//! known face saying the line (`lamp_voice::services::health_read`): the
//! voice says the answer in the fixed words [`said`] builds here, in English
//! or Vietnamese, and no chat model is given a value. Those words are read
//! aloud through Lamp's voice services (its speech service renders them).
//! Each answer carries the summary's `as_of`; a summary older than
//! [`STALE_AFTER_S`] gives no values, only that it is out of date. Nothing
//! here invents a value: a day or a field the phone did not send is said to
//! be missing. A day older than [`DAYS_MOST`] days is dropped at each push
//! and each read ([`prune`]).
//!
//! Health values are in no journal line, error or `Debug` ([`Summary`]'s
//! shows the number of days alone), and a read's items name a day only by
//! `date`, so the voice's service record of a read keeps the date and none
//! of the numbers (`lamp_voice::service_records::found` keeps an item's
//! title, sender, date, link, id and text, none of which an item here has).
//! A made skill or a skill's run never reads it (`serve.rs`), and it is
//! no grant a skill can declare.
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{fmt, ops::RangeInclusive};

/// Its catalog code, the store's file name and the tools' `service`.
pub const CODE: &str = "apple_health";
/// The switch, read by lamp-cloud (the kind) and this program (the
/// service listed, kept and read): off unless `on`.
pub const ENVIRONMENT: &str = "LAMP_APPLE_HEALTH";
/// The voice's own call on the tools socket, never a model's or a skill's:
/// the owner's "forget everything" removes the kept summary.
pub const FORGET_CALL: &str = "apple_health_forget";
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
    let mut summary = Summary {
        as_of: as_of.to_owned(),
        tz: tz.to_owned(),
        days,
    };
    // Older days are dropped, not refused: the phone may send a day the
    // lamp has stopped keeping.
    prune(&mut summary, now);
    Ok(summary)
}

/// `summary` with every day before the [`DAYS_MOST`] days ending today (in
/// the owner's zone, at `now`) dropped: at each push ([`fit`]) and each read
/// (`serve.rs`, which keeps the file without them). Whether any went.
pub fn prune(summary: &mut Summary, now: i64) -> bool {
    let first = first_of(today(summary, now), DAYS_MOST as u32);
    let before = summary.days.len();
    summary
        .days
        .retain(|day| date_of(&day.date).is_some_and(|date| date >= first));
    summary.days.len() != before
}

/// The owner's zone: the summary's `tz`, else the offset its `as_of` has.
fn zone(summary: &Summary) -> jiff::tz::TimeZone {
    zone_of(&summary.tz, &summary.as_of)
}

/// The zone `tz` names, else the offset `as_of` has, else UTC.
fn zone_of(tz: &str, as_of: &str) -> jiff::tz::TimeZone {
    if let Ok(zone) = jiff::tz::TimeZone::get(tz) {
        return zone;
    }
    let offset = as_of
        .get(as_of.len().saturating_sub(6)..)
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
    let mut answer = framed(name, summary, all, note);
    answer["workouts_found"] = json!(found);
    Ok(answer)
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
        totals["asleep_nightly_average_min"] = json!(asleep);
    }
    let any_in_bed = !in_bed.is_empty();
    if let Some(in_bed) = average(in_bed) {
        totals["in_bed_nightly_average"] = json!(duration(in_bed as u32));
        totals["in_bed_nightly_average_min"] = json!(in_bed);
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

/// The language a health answer is said in ([`said`]): the owner's, English
/// for any the lamp has no health words for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spoken {
    English,
    Vietnamese,
}

impl Spoken {
    fn pick(self, english: &'static str, vietnamese: &'static str) -> &'static str {
        match self {
            Self::English => english,
            Self::Vietnamese => vietnamese,
        }
    }
}

/// What the voice says, in place of any read, when the line is not the
/// owner's known face seen saying it, or no face is enrolled: nothing of
/// the summary is read (the first builder's P1 on #7900).
pub fn not_yours(spoken: Spoken) -> &'static str {
    spoken.pick(
        "I keep health details for the person they belong to.",
        "Mình chỉ nói thông tin sức khỏe với đúng người sở hữu chúng.",
    )
}

/// What the voice says of a health read on a route that can't say fixed
/// words without a model (the Live route): nothing is read.
pub fn not_on_this_voice(spoken: Spoken) -> &'static str {
    spoken.pick(
        "I can't read Apple Health on this voice.",
        "Mình không đọc được Apple Health bằng giọng này.",
    )
}

/// A whole number as said: `8,421` in English, `8421` in Vietnamese.
fn number(n: u64, spoken: Spoken) -> String {
    let digits = n.to_string();
    if spoken == Spoken::Vietnamese {
        return digits;
    }
    let mut out = String::new();
    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// `minutes` as said: `6 hours and 52 minutes`, `6 tiếng 52 phút`.
fn spoken_duration(minutes: u64, spoken: Spoken) -> String {
    let (h, m) = (minutes / 60, minutes % 60);
    match spoken {
        Spoken::English => {
            let hours = format!("{h} hour{}", if h == 1 { "" } else { "s" });
            let mins = format!("{m} minute{}", if m == 1 { "" } else { "s" });
            match (h, m) {
                (0, _) => mins,
                (_, 0) => hours,
                _ => format!("{hours} and {mins}"),
            }
        }
        Spoken::Vietnamese => match (h, m) {
            (0, _) => format!("{m} phút"),
            (_, 0) => format!("{h} tiếng"),
            _ => format!("{h} tiếng {m} phút"),
        },
    }
}

/// `HH:MM` as said: `11:48 pm`, `7 am`; `23 giờ 48`, `7 giờ`.
fn spoken_clock(text: &str, spoken: Spoken) -> Option<String> {
    if !clock_ok(text) {
        return None;
    }
    let hour: u32 = text[..2].parse().ok()?;
    let minute: u32 = text[3..].parse().ok()?;
    Some(match spoken {
        Spoken::English => {
            let half = if hour < 12 { "am" } else { "pm" };
            let twelve = match hour % 12 {
                0 => 12,
                h => h,
            };
            if minute == 0 {
                format!("{twelve} {half}")
            } else {
                format!("{twelve}:{minute:02} {half}")
            }
        }
        Spoken::Vietnamese if minute == 0 => format!("{hour} giờ"),
        Spoken::Vietnamese => format!("{hour} giờ {minute:02}"),
    })
}

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

/// A date as said: `November 16`, `ngày 16 tháng 11`.
fn spoken_date(date: jiff::civil::Date, spoken: Spoken) -> String {
    let month = usize::try_from(date.month()).unwrap_or(1).clamp(1, 12);
    match spoken {
        Spoken::English => format!("{} {}", MONTHS[month - 1], date.day()),
        Spoken::Vietnamese => format!("ngày {} tháng {month}", date.day()),
    }
}

/// A day as said beside `today`: `today`, `yesterday`, `on November 16`.
fn spoken_day(text: &str, today: jiff::civil::Date, spoken: Spoken) -> String {
    let Some(date) = date_of(text) else {
        return spoken.pick("that day", "hôm đó").to_owned();
    };
    if date == today {
        spoken.pick("today", "hôm nay").to_owned()
    } else if today.yesterday().ok() == Some(date) {
        spoken.pick("yesterday", "hôm qua").to_owned()
    } else {
        match spoken {
            Spoken::English => format!("on {}", spoken_date(date, spoken)),
            Spoken::Vietnamese => spoken_date(date, spoken),
        }
    }
}

/// The night that ended on `text`, as said beside `today`.
fn spoken_night(text: &str, today: jiff::civil::Date, spoken: Spoken) -> String {
    let Some(date) = date_of(text) else {
        return spoken.pick("that night", "đêm đó").to_owned();
    };
    if date == today {
        spoken.pick("last night", "đêm qua").to_owned()
    } else if today.yesterday().ok() == Some(date) {
        spoken
            .pick("the night before last", "đêm hôm kia")
            .to_owned()
    } else {
        match spoken {
            Spoken::English => format!("the night ending {}", spoken_date(date, spoken)),
            Spoken::Vietnamese => format!("đêm kết thúc {}", spoken_date(date, spoken)),
        }
    }
}

/// When the phone shared it (`as_of`), as said beside `today`, in `zone`:
/// `as of 7:30 am`, `as of yesterday at 9:10 pm`; `lúc 7 giờ 30`.
fn spoken_as_of(
    as_of: &str,
    zone: &jiff::tz::TimeZone,
    today: jiff::civil::Date,
    spoken: Spoken,
) -> Option<String> {
    let at = as_of
        .parse::<jiff::Timestamp>()
        .ok()?
        .to_zoned(zone.clone());
    let clock = spoken_clock(&format!("{:02}:{:02}", at.hour(), at.minute()), spoken)?;
    let date = at.date();
    Some(match spoken {
        Spoken::English if date == today => format!("as of {clock}"),
        Spoken::English if today.yesterday().ok() == Some(date) => {
            format!("as of yesterday at {clock}")
        }
        Spoken::English => format!("as of {} at {clock}", spoken_date(date, spoken)),
        Spoken::Vietnamese if date == today => format!("lúc {clock}"),
        Spoken::Vietnamese if today.yesterday().ok() == Some(date) => {
            format!("lúc {clock} hôm qua")
        }
        Spoken::Vietnamese => format!("lúc {clock} {}", spoken_date(date, spoken)),
    })
}

/// A workout's kind as said: `running`, `strength training`; in Vietnamese
/// the common ones in Vietnamese, any other in its own words.
fn spoken_kind(kind: &str, spoken: Spoken) -> String {
    let english = kind.replace('_', " ");
    if spoken == Spoken::English {
        return english;
    }
    match kind {
        "running" => "chạy bộ",
        "walking" => "đi bộ",
        "cycling" => "đạp xe",
        "swimming" => "bơi",
        "hiking" => "leo núi",
        "yoga" => "yoga",
        "dance" | "cardio_dance" | "social_dance" => "nhảy",
        "traditional_strength_training" | "functional_strength_training" => "tập tạ",
        "high_intensity_interval_training" => "tập HIIT",
        "elliptical" => "tập máy elip",
        "rowing" => "chèo thuyền",
        "badminton" => "cầu lông",
        "tennis" => "quần vợt",
        "soccer" => "bóng đá",
        "basketball" => "bóng rổ",
        _ => return english,
    }
    .to_owned()
}

/// One workout item ([`workouts`]) as said.
fn spoken_workout(item: &Value, today: jiff::civil::Date, spoken: Spoken) -> String {
    let kind = spoken_kind(item["type"].as_str().unwrap_or("workout"), spoken);
    let day = spoken_day(item["date"].as_str().unwrap_or_default(), today, spoken);
    let mut said = format!("{kind} {day}");
    if let Some(min) = item["minutes"].as_u64() {
        said.push_str(&match spoken {
            Spoken::English => format!(" for {}", spoken_duration(min, spoken)),
            Spoken::Vietnamese => format!(" trong {}", spoken_duration(min, spoken)),
        });
    }
    if let Some(km) = item["km"].as_f64() {
        let km = format!("{km:.1}");
        let km = km.strip_suffix(".0").unwrap_or(&km).to_owned();
        said.push_str(&match spoken {
            Spoken::English => format!(", {km} kilometres"),
            Spoken::Vietnamese => format!(", {} ki-lô-mét", km.replace('.', ",")),
        });
    }
    said
}

/// `parts` joined as a list is said: `a; b; and c`, `a; b; và c`.
fn listed(parts: &[String], spoken: Spoken) -> String {
    match parts {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{}; {} {last}", rest.join("; "), spoken.pick("and", "và")),
    }
}

/// The most workouts said by name; any more are counted.
const WORKOUTS_SAID: usize = 3;

/// The exact words the voice says for a health read: built here from the
/// read's answer `told` (or its `{"error"}`) for `what` with `args`, at
/// `now`, in `spoken`, with no model asked. Every value said is one the
/// phone shared; a day, field or summary missing is said to be missing.
pub fn said(what: &str, args: &Value, told: &Value, now: i64, spoken: Spoken) -> String {
    if let Some(error) = told.get("error").and_then(Value::as_str) {
        return if error == NO_DATA {
            spoken
                .pick(
                    "Lamp has no Apple Health data yet. You can share it from the Lamp app on your phone.",
                    "Lamp chưa có dữ liệu Apple Health. Bạn có thể chia sẻ từ ứng dụng Lamp trên điện thoại.",
                )
                .to_owned()
        } else {
            spoken
                .pick(
                    "I couldn't read Apple Health just now.",
                    "Lúc này mình chưa đọc được Apple Health.",
                )
                .to_owned()
        };
    }
    if told["stale"] == true {
        return spoken
            .pick(
                "The Apple Health data on Lamp is more than two days old, so I won't read it. Opening the Lamp app on your phone shares it again.",
                "Dữ liệu Apple Health trên Lamp đã cũ hơn hai ngày nên mình không đọc. Mở ứng dụng Lamp trên điện thoại để chia sẻ lại nhé.",
            )
            .to_owned();
    }
    let as_of = told["as_of"].as_str().unwrap_or_default();
    let zone = zone_of(told["tz"].as_str().unwrap_or_default(), as_of);
    let today = jiff::Timestamp::from_second(now)
        .unwrap_or(jiff::Timestamp::UNIX_EPOCH)
        .to_zoned(zone.clone())
        .date();
    let items = told["quoted"].as_array().cloned().unwrap_or_default();
    let none = || {
        let kind = match what {
            "sleep" => spoken.pick("sleep", "dữ liệu giấc ngủ"),
            "steps" => spoken.pick("steps", "số bước"),
            "heart" => spoken.pick("resting heart rate", "nhịp tim lúc nghỉ"),
            "workouts" => spoken.pick("workouts", "buổi tập"),
            _ => {
                return spoken
                    .pick(
                        "Your phone hasn't shared anything from the last 7 days with Lamp.",
                        "Điện thoại chưa chia sẻ gì trong 7 ngày qua với Lamp.",
                    )
                    .to_owned();
            }
        };
        match spoken {
            Spoken::English => format!("Your phone hasn't shared any {kind} for that with Lamp."),
            Spoken::Vietnamese => {
                format!("Điện thoại chưa chia sẻ {kind} nào cho lúc đó với Lamp.")
            }
        }
    };
    if items.is_empty() {
        return none();
    }
    let Some(when) = spoken_as_of(as_of, &zone, today, spoken) else {
        return none();
    };
    let first = &items[0];
    let n = |value: &Value| value.as_u64().map(|n| number(n, spoken));
    let body = match what {
        "sleep" => {
            let in_bed = first.get("in_bed_min").is_some();
            let Some(minutes) = first["asleep_min"]
                .as_u64()
                .or(first["in_bed_min"].as_u64())
            else {
                return none();
            };
            let night = spoken_night(first["date"].as_str().unwrap_or_default(), today, spoken);
            let length = spoken_duration(minutes, spoken);
            let from_to = match (
                first["fell_asleep"]
                    .as_str()
                    .and_then(|at| spoken_clock(at, spoken)),
                first["woke"]
                    .as_str()
                    .and_then(|at| spoken_clock(at, spoken)),
            ) {
                (Some(from), Some(to)) => match spoken {
                    Spoken::English => format!(", from {from} to {to}"),
                    Spoken::Vietnamese => format!(", từ {from} đến {to}"),
                },
                _ => String::new(),
            };
            match (spoken, in_bed) {
                (Spoken::English, false) => format!("you slept {length} {night}{from_to}"),
                (Spoken::English, true) => {
                    format!("you were in bed for {length} {night}{from_to}")
                }
                (Spoken::Vietnamese, false) => format!("{night} bạn ngủ {length}{from_to}"),
                (Spoken::Vietnamese, true) => {
                    format!("{night} bạn nằm trên giường {length}{from_to}")
                }
            }
        }
        "steps" if first.get("steps_total").is_some() => {
            let (Some(total), Some(average), Some(asked)) = (
                n(&first["steps_total"]),
                n(&first["steps_daily_average"]),
                first["days_asked"].as_u64(),
            ) else {
                return none();
            };
            let with = first["days_with_steps"].as_u64().unwrap_or(asked);
            let mut said = match spoken {
                Spoken::English => format!(
                    "you walked {total} steps over the last {asked} days, about {average} a day"
                ),
                Spoken::Vietnamese => format!(
                    "{asked} ngày qua bạn đi tổng cộng {total} bước, khoảng {average} bước mỗi ngày"
                ),
            };
            if with < asked {
                said.push_str(&match spoken {
                    Spoken::English => format!(". Only {with} of those days were shared"),
                    Spoken::Vietnamese => {
                        format!(". Chỉ có {with} ngày trong số đó được chia sẻ")
                    }
                });
            }
            said
        }
        "steps" => {
            let Some(steps) = n(&first["steps"]) else {
                return none();
            };
            let date = first["date"].as_str().unwrap_or_default();
            let day = spoken_day(date, today, spoken);
            let is_today = date_of(date) == Some(today);
            match (spoken, is_today) {
                (Spoken::English, true) => format!("you have {steps} steps so far today"),
                (Spoken::English, false) => format!("you walked {steps} steps {day}"),
                (Spoken::Vietnamese, true) => {
                    format!("hôm nay đến giờ bạn đã đi {steps} bước")
                }
                (Spoken::Vietnamese, false) => format!("{day} bạn đi {steps} bước"),
            }
        }
        "heart" => {
            let Some(bpm) = n(&first["resting_heart_rate_bpm"]) else {
                return none();
            };
            let day = spoken_day(first["date"].as_str().unwrap_or_default(), today, spoken);
            match spoken {
                Spoken::English => {
                    format!("your resting heart rate {day} was {bpm} beats a minute")
                }
                Spoken::Vietnamese => {
                    format!("nhịp tim lúc nghỉ {day} của bạn là {bpm} nhịp mỗi phút")
                }
            }
        }
        "workouts" => {
            let found = told["workouts_found"]
                .as_u64()
                .unwrap_or(items.len() as u64);
            let mut parts: Vec<String> = items
                .iter()
                .take(WORKOUTS_SAID)
                .map(|item| spoken_workout(item, today, spoken))
                .collect();
            let more = found.saturating_sub(parts.len() as u64);
            if more > 0 {
                parts.push(match spoken {
                    Spoken::English => format!("{more} more"),
                    Spoken::Vietnamese => format!("{more} buổi khác"),
                });
            }
            match (spoken, asked_days(args).ok().flatten()) {
                (Spoken::English, None) => {
                    format!("your latest workout was {}", parts.join("; "))
                }
                (Spoken::Vietnamese, None) => {
                    format!("buổi tập gần nhất của bạn là {}", parts.join("; "))
                }
                (Spoken::English, Some(days)) => format!(
                    "you did {found} workout{} in the last {days} days: {}",
                    if found == 1 { "" } else { "s" },
                    listed(&parts, spoken)
                ),
                (Spoken::Vietnamese, Some(days)) => format!(
                    "{days} ngày qua bạn có {found} buổi tập: {}",
                    listed(&parts, spoken)
                ),
            }
        }
        "week" => {
            let week = &told["week"];
            let mut parts = Vec::new();
            if let (Some(total), Some(average)) =
                (n(&week["steps_total"]), n(&week["steps_daily_average"]))
            {
                parts.push(match spoken {
                    Spoken::English => format!("you walked {total} steps, about {average} a day"),
                    Spoken::Vietnamese => {
                        format!("bạn đi {total} bước, khoảng {average} bước mỗi ngày")
                    }
                });
            }
            if let Some(asleep) = week["asleep_nightly_average_min"].as_u64() {
                let length = spoken_duration(asleep, spoken);
                parts.push(match spoken {
                    Spoken::English => format!("slept about {length} a night"),
                    Spoken::Vietnamese => format!("ngủ khoảng {length} mỗi đêm"),
                });
            }
            if let Some(in_bed) = week["in_bed_nightly_average_min"].as_u64() {
                let length = spoken_duration(in_bed, spoken);
                parts.push(match spoken {
                    Spoken::English => format!("spent about {length} in bed a night"),
                    Spoken::Vietnamese => format!("nằm trên giường khoảng {length} mỗi đêm"),
                });
            }
            if let Some(bpm) = n(&week["resting_heart_rate_average_bpm"]) {
                parts.push(match spoken {
                    Spoken::English => {
                        format!("had a resting heart rate around {bpm} beats a minute")
                    }
                    Spoken::Vietnamese => {
                        format!("nhịp tim lúc nghỉ trung bình {bpm} nhịp mỗi phút")
                    }
                });
            }
            let workouts = week["workouts"].as_u64().unwrap_or(0);
            parts.push(match (spoken, workouts) {
                (Spoken::English, 0) => "did no workouts".to_owned(),
                (Spoken::English, 1) => "did 1 workout".to_owned(),
                (Spoken::English, w) => format!("did {w} workouts"),
                (Spoken::Vietnamese, 0) => "không có buổi tập nào".to_owned(),
                (Spoken::Vietnamese, w) => format!("có {w} buổi tập"),
            });
            let said = listed(&parts, spoken);
            match spoken {
                Spoken::English => format!("over the last 7 days {said}"),
                Spoken::Vietnamese => format!("7 ngày qua {said}"),
            }
        }
        _ => return none(),
    };
    match spoken {
        Spoken::English => format!("Apple Health, {when}: {body}."),
        Spoken::Vietnamese => format!("Theo Apple Health {when}, {body}."),
    }
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

    /// A day older than the 14 ending today is dropped at a push, not
    /// refused, and again at a read ([`prune`]): never kept on.
    #[test]
    fn days_older_than_fourteen_are_dropped() {
        let mut data = contract();
        data["days"] = json!([
            {"date": "2026-11-04", "steps": 1},
            {"date": "2026-11-05", "steps": 2},
            {"date": "2026-11-18", "steps": 3}
        ]);
        let summary = fit(&data, AS_OF).unwrap();
        let dates: Vec<&str> = summary.days.iter().map(|day| day.date.as_str()).collect();
        assert_eq!(dates, ["2026-11-05", "2026-11-18"]);
        // Kept, then read two days on: the 5th is past the 14 days now.
        let mut kept = summary.clone();
        assert!(prune(&mut kept, AS_OF + 2 * 86_400));
        assert_eq!(kept.days.len(), 1);
        assert!(!prune(&mut kept, AS_OF + 2 * 86_400));
    }

    /// The fixed words for each read, in English and Vietnamese: every
    /// value the phone's, nothing a model wrote.
    #[test]
    fn each_read_is_said_in_fixed_words() {
        let summary = fit(&contract(), AS_OF).unwrap();
        let now = AS_OF + 600;
        let say = |what: &str, args: Value, spoken: Spoken| {
            let told = match what {
                "sleep" => sleep(CODE, &summary, &args, now),
                "steps" => steps(CODE, &summary, &args, now),
                "heart" => heart(CODE, &summary, &args, now),
                "workouts" => workouts(CODE, &summary, &args, now),
                _ => week(CODE, &summary, now),
            }
            .unwrap();
            said(what, &args, &told, now, spoken)
        };
        use Spoken::{English, Vietnamese};
        assert_eq!(
            say("sleep", json!({}), English),
            "Apple Health, as of 7:30 am: you slept 6 hours and 52 minutes last night, from 11:48 pm to 6:40 am."
        );
        assert_eq!(
            say("sleep", json!({}), Vietnamese),
            "Theo Apple Health lúc 7 giờ 30, đêm qua bạn ngủ 6 tiếng 52 phút, từ 23 giờ 48 đến 6 giờ 40."
        );
        assert_eq!(
            say("steps", json!({}), English),
            "Apple Health, as of 7:30 am: you have 8,421 steps so far today."
        );
        assert_eq!(
            say("steps", json!({}), Vietnamese),
            "Theo Apple Health lúc 7 giờ 30, hôm nay đến giờ bạn đã đi 8421 bước."
        );
        assert_eq!(
            say("steps", json!({"days": 7}), English),
            "Apple Health, as of 7:30 am: you walked 8,421 steps over the last 7 days, about 8,421 a day. Only 1 of those days were shared."
        );
        assert_eq!(
            say("heart", json!({}), English),
            "Apple Health, as of 7:30 am: your resting heart rate today was 58 beats a minute."
        );
        assert_eq!(
            say("heart", json!({}), Vietnamese),
            "Theo Apple Health lúc 7 giờ 30, nhịp tim lúc nghỉ hôm nay của bạn là 58 nhịp mỗi phút."
        );
        assert_eq!(
            say("workouts", json!({}), English),
            "Apple Health, as of 7:30 am: your latest workout was running today for 32 minutes, 5.1 kilometres."
        );
        assert_eq!(
            say("workouts", json!({"days": 7}), Vietnamese),
            "Theo Apple Health lúc 7 giờ 30, 7 ngày qua bạn có 1 buổi tập: chạy bộ hôm nay trong 32 phút, 5,1 ki-lô-mét."
        );
        assert_eq!(
            say("week", json!({}), English),
            "Apple Health, as of 7:30 am: over the last 7 days you walked 8,421 steps, about 8,421 a day; slept about 6 hours and 52 minutes a night; had a resting heart rate around 58 beats a minute; and did 1 workout."
        );
        assert_eq!(
            say("week", json!({}), Vietnamese),
            "Theo Apple Health lúc 7 giờ 30, 7 ngày qua bạn đi 8421 bước, khoảng 8421 bước mỗi ngày; ngủ khoảng 6 tiếng 52 phút mỗi đêm; nhịp tim lúc nghỉ trung bình 58 nhịp mỗi phút; và có 1 buổi tập."
        );
        // Nothing for the day asked, nothing kept, out of date, or a
        // refused read: plain words, no value.
        assert_eq!(
            say("steps", json!({"date": "2026-11-10"}), English),
            "Your phone hasn't shared any steps for that with Lamp."
        );
        let no_data = json!({"error": NO_DATA});
        assert!(
            said("sleep", &json!({}), &no_data, now, English)
                .starts_with("Lamp has no Apple Health data yet.")
        );
        let stale = usable(
            CODE,
            Some(&Kept {
                connector: CODE.into(),
                stored_at: 0,
                summary: summary.clone(),
            }),
            AS_OF + STALE_AFTER_S + 1,
        )
        .unwrap()
        .unwrap_err();
        let old = said(
            "sleep",
            &json!({}),
            &stale,
            AS_OF + STALE_AFTER_S + 1,
            Vietnamese,
        );
        assert!(
            old.starts_with("Dữ liệu Apple Health trên Lamp đã cũ"),
            "{old}"
        );
        assert_eq!(
            said("sleep", &json!({}), &json!({"error": "x"}), now, English),
            "I couldn't read Apple Health just now."
        );
        // A summary shared yesterday says so.
        let next_day = AS_OF + 20 * 3_600;
        let told = steps(CODE, &summary, &json!({"date": "2026-11-18"}), next_day).unwrap();
        assert_eq!(
            said("steps", &json!({}), &told, next_day, English),
            "Apple Health, as of yesterday at 7:30 am: you walked 8,421 steps yesterday."
        );
    }

    /// The plain lines say no value.
    #[test]
    fn the_plain_lines_say_nothing_of_the_summary() {
        assert_eq!(
            not_yours(Spoken::English),
            "I keep health details for the person they belong to."
        );
        for line in [
            not_yours(Spoken::Vietnamese),
            not_on_this_voice(Spoken::English),
            not_on_this_voice(Spoken::Vietnamese),
        ] {
            assert!(!line.chars().any(|c| c.is_ascii_digit()), "{line}");
        }
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
                "asleep_nightly_average": "7 h 7 min", "asleep_nightly_average_min": 427,
                "resting_heart_rate_average_bpm": 58})
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
