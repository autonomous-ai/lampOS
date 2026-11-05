//! Calendar decisions using only caller-supplied clocks and bundled IANA rules.
//!
//! Elapsed timers prefer the same boot's elapsed clock. Calendar reminders use
//! trusted Unix time. This module never reads a clock, environment, or file.

use jiff::civil::{Date, DateTime, Time};
use jiff::tz::{AmbiguousOffset, TimeZone, TimeZoneDatabase};
use jiff::{Timestamp, ToSpan, Zoned};
use serde::{Deserialize, Serialize};
use std::fmt;

pub const MAX_AFTER_SECONDS: u64 = 31_536_000;
pub const MIN_UNIX_MS: i64 = 0;
/// Jiff reserves its largest legal UTC offset at the end of year 9999.
/// This is 9999-12-30T22:00:00.999Z, inclusive.
pub const MAX_UNIX_MS: i64 = 253_402_207_200_999;
const MAX_ID_BYTES: usize = 128;
const MAX_ZONE_BYTES: usize = 128;
const MAX_LOCAL_BYTES: usize = 192;
/// The shortest and longest `every` (first builder, 2 Nov 17:4x on #4710: a
/// lamp reminding someone every few minutes is a nag, not a companion).
pub const MIN_EVERY_MINUTES: u32 = 15;
pub const MAX_EVERY_MINUTES: u32 = 1440;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClockSample {
    pub unix_ms: i64,
    pub boot_ms: u64,
    pub boot_id: String,
    pub wall_trusted: bool,
}

impl ClockSample {
    /// Does not compare samples. The owning engine checks boot-clock ordering.
    pub fn check(&self) -> Result<(), TimeError> {
        check_id(&self.boot_id)?;
        timestamp(self.unix_ms)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum When {
    After {
        seconds: u64,
    },
    At {
        time: String,
        #[serde(default)]
        day: Option<String>,
        #[serde(default)]
        zone: Option<String>,
        #[serde(default)]
        fold: Option<Fold>,
    },
    Daily {
        time: String,
        #[serde(default)]
        zone: Option<String>,
    },
    Weekly {
        weekday: Weekday,
        time: String,
        #[serde(default)]
        zone: Option<String>,
    },
    /// Every so many minutes or hours, never both, 15 minutes to 24 hours
    /// in all (`LAMP_SCHEDULE_EVERY`). With a window, `from` to `to` (local
    /// `HH:MM`, either order: one across midnight is fine, and equal ones are
    /// the whole day), the occurrences are the window's start and every step
    /// after it inside it; the first after it closes is its next start.
    /// Without one, the first is a step from now, to the whole minute.
    Every {
        #[serde(default)]
        minutes: Option<u32>,
        #[serde(default)]
        hours: Option<u32>,
        #[serde(default)]
        from: Option<String>,
        #[serde(default)]
        to: Option<String>,
        #[serde(default)]
        zone: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Fold {
    Earlier,
    Later,
    Reject,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Weekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl Weekday {
    fn index(self) -> i8 {
        match self {
            Self::Monday => 0,
            Self::Tuesday => 1,
            Self::Wednesday => 2,
            Self::Thursday => 3,
            Self::Friday => 4,
            Self::Saturday => 5,
            Self::Sunday => 6,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Deadline {
    pub unix_ms: i64,
    pub elapsed: Option<Elapsed>,
    pub repeat: Option<Repeat>,
    /// Canonical actual zoned datetime, including offset and IANA name.
    pub local: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Elapsed {
    pub boot_id: String,
    pub at_boot_ms: u64,
}

/// The requested civil date and clock time survive a gap's forward shift.
/// `date` identifies this occurrence, not the next one or the delivery date.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Repeat {
    Daily {
        time: String,
        zone: String,
        date: String,
    },
    Weekly {
        weekday: Weekday,
        time: String,
        zone: String,
        date: String,
    },
    /// This occurrence's civil `date` and `time`, a step of `minutes`, and
    /// the window's `from` and `to`, both or neither.
    Every {
        minutes: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to: Option<String>,
        time: String,
        zone: String,
        date: String,
    },
}

impl Deadline {
    /// Validate a persisted descriptor before using it, without consulting the
    /// current time. Serialization itself does not establish this invariant.
    pub fn check(&self) -> Result<(), TimeError> {
        let at = timestamp(self.unix_ms)?;
        if self.local.len() > MAX_LOCAL_BYTES {
            return Err(TimeError::InvalidDeadline);
        }
        let (_, zone) = self
            .local
            .strip_suffix(']')
            .and_then(|s| s.rsplit_once('['))
            .ok_or(TimeError::InvalidDeadline)?;
        let zone = named_zone(zone)?;
        if at.to_zoned(zone).to_string() != self.local {
            return Err(TimeError::InvalidDeadline);
        }
        if let Some(elapsed) = &self.elapsed {
            check_id(&elapsed.boot_id)?;
            if self.repeat.is_some() {
                return Err(TimeError::InvalidDeadline);
            }
        }
        if let Some(repeat) = &self.repeat {
            let (time, zone, date, weekday) = repeat.parts();
            let date = parse_date(date)?;
            if weekday.is_some_and(|day| day.index() != date.weekday().to_monday_zero_offset()) {
                return Err(TimeError::InvalidDeadline);
            }
            let time = parse_time(time)?;
            if let Repeat::Every {
                minutes, from, to, ..
            } = repeat
            {
                if !(MIN_EVERY_MINUTES..=MAX_EVERY_MINUTES).contains(minutes) {
                    return Err(TimeError::InvalidDeadline);
                }
                if let Some(window) = window(from.as_deref(), to.as_deref())
                    .map_err(|_| TimeError::InvalidDeadline)?
                    && !window.holds(time)
                {
                    return Err(TimeError::InvalidDeadline);
                }
            }
            let actual = compatible(date, time, &named_zone(zone)?)?;
            if actual.timestamp().as_millisecond() != self.unix_ms
                || actual.to_string() != self.local
            {
                return Err(TimeError::InvalidDeadline);
            }
        }
        Ok(())
    }
}

impl Repeat {
    fn parts(&self) -> (&str, &str, &str, Option<Weekday>) {
        match self {
            Self::Daily { time, zone, date } => (time, zone, date, None),
            Self::Weekly {
                weekday,
                time,
                zone,
                date,
            } => (time, zone, date, Some(*weekday)),
            Self::Every {
                time, zone, date, ..
            } => (time, zone, date, None),
        }
    }
}

/// An `every` window: its start, as a time and as minutes into the day, and
/// how many minutes it lasts, 1 to 1439.
#[derive(Clone, Copy, Debug)]
struct Window {
    start: Time,
    from: i32,
    length: i32,
}

impl Window {
    /// How far `time` is into the window, if it is in it.
    fn offset(self, time: Time) -> Option<i32> {
        let offset = (minute_of_day(time) - self.from).rem_euclid(1440);
        (offset < self.length).then_some(offset)
    }

    fn holds(self, time: Time) -> bool {
        self.offset(time).is_some()
    }
}

fn minute_of_day(time: Time) -> i32 {
    i32::from(time.hour()) * 60 + i32::from(time.minute())
}

/// The window `from`..`to`: none for neither, or for equal ones (the whole
/// day); one alone is refused.
fn window(from: Option<&str>, to: Option<&str>) -> Result<Option<Window>, TimeError> {
    match (from, to) {
        (None, None) => Ok(None),
        (Some(from), Some(to)) => {
            let (start, end) = (parse_time(from)?, parse_time(to)?);
            let length = (minute_of_day(end) - minute_of_day(start)).rem_euclid(1440);
            Ok((length > 0).then_some(Window {
                start,
                from: minute_of_day(start),
                length,
            }))
        }
        _ => Err(TimeError::InvalidWindow),
    }
}

/// The step after the occurrence at `at`, in civil time: `minutes` on, or,
/// past the window's end, the window's next start.
fn every_step(at: DateTime, minutes: u32, window: Option<Window>) -> Result<DateTime, TimeError> {
    let on = at
        .checked_add(i64::from(minutes).minutes())
        .map_err(|_| TimeError::OutOfRange)?;
    let Some(window) = window else {
        return Ok(on);
    };
    let offset = window.offset(at.time()).ok_or(TimeError::InvalidDeadline)?;
    if offset + i32::try_from(minutes).map_err(|_| TimeError::OutOfRange)? < window.length {
        return Ok(on);
    }
    // The window began `offset` minutes before `at`: its next start is a day on.
    let began = at
        .checked_sub(i64::from(offset).minutes())
        .map_err(|_| TimeError::OutOfRange)?;
    Ok(add_days(began.date(), 1)?.to_datetime(window.start))
}

/// Most steps one `every` takes to find its next occurrence: a gap in the
/// zone's clock can map a few steps to the same or an earlier instant, and
/// a first one walks from yesterday's window start.
const MOST_EVERY_STEPS: usize = 400;

fn every_deadline(
    at: DateTime,
    zone: &TimeZone,
    minutes: u32,
    window: Option<(&str, &str)>,
) -> Result<Deadline, TimeError> {
    let actual = compatible(at.date(), at.time(), zone)?;
    let repeat = Repeat::Every {
        minutes,
        from: window.map(|(from, _)| from.to_owned()),
        to: window.map(|(_, to)| to.to_owned()),
        time: format!("{:02}:{:02}", at.hour(), at.minute()),
        zone: zone.iana_name().ok_or(TimeError::InvalidZone)?.to_owned(),
        date: at.date().to_string(),
    };
    deadline(actual, Some(repeat))
}

/// The step in minutes `every` asks for, 15 to 1440, in minutes or hours,
/// never both.
fn every_minutes(minutes: Option<u32>, hours: Option<u32>) -> Result<u32, TimeError> {
    let total = match (minutes, hours) {
        (Some(minutes), None) => minutes,
        (None, Some(hours)) => hours.checked_mul(60).ok_or(TimeError::InvalidInterval)?,
        _ => return Err(TimeError::InvalidInterval),
    };
    if !(MIN_EVERY_MINUTES..=MAX_EVERY_MINUTES).contains(&total) {
        return Err(TimeError::InvalidInterval);
    }
    Ok(total)
}

fn first_every(
    step: u32,
    from: Option<&str>,
    to: Option<&str>,
    zone_name: &str,
    now: &ClockSample,
) -> Result<Deadline, TimeError> {
    let zone = named_zone(zone_name)?;
    let window = window(from, to)?;
    // Equal ones are the whole day: kept as no window.
    let kept = window.and(from.zip(to));
    let here = timestamp(now.unix_ms)?.to_zoned(zone.clone());
    let mut at = match window {
        // A step from now, up to the next whole minute.
        None => {
            let on = here
                .datetime()
                .checked_add(i64::from(step).minutes())
                .map_err(|_| TimeError::OutOfRange)?;
            let whole = on.date().to_datetime(
                Time::new(on.hour(), on.minute(), 0, 0).map_err(|_| TimeError::OutOfRange)?,
            );
            if whole < on {
                whole
                    .checked_add(1.minute())
                    .map_err(|_| TimeError::OutOfRange)?
            } else {
                whole
            }
        }
        // Yesterday's window start, then its steps, to the first ahead.
        Some(window) => add_days(here.date(), -1)?.to_datetime(window.start),
    };
    for _ in 0..MOST_EVERY_STEPS {
        let candidate = every_deadline(at, &zone, step, kept)?;
        if candidate.unix_ms > now.unix_ms {
            return Ok(candidate);
        }
        at = every_step(at, step, window)?;
    }
    Err(TimeError::NonIncreasing)
}

fn next_every(previous: &Deadline, repeat: &Repeat) -> Result<Deadline, TimeError> {
    let Repeat::Every {
        minutes,
        from,
        to,
        time,
        zone,
        date,
    } = repeat
    else {
        return Err(TimeError::InvalidDeadline);
    };
    let zone = named_zone(zone)?;
    let window = window(from.as_deref(), to.as_deref())?;
    let kept = window.and(from.as_deref().zip(to.as_deref()));
    let mut at = parse_date(date)?.to_datetime(parse_time(time)?);
    for _ in 0..MOST_EVERY_STEPS {
        at = every_step(at, *minutes, window)?;
        // A gap in the zone's clock can put a step on the same instant as
        // the one before, or an earlier one: stepped over, never said twice.
        let next = every_deadline(at, &zone, *minutes, kept)?;
        if next.unix_ms > previous.unix_ms {
            return Ok(next);
        }
    }
    Err(TimeError::NonIncreasing)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimeError {
    InvalidClock,
    UntrustedClock,
    InvalidTime,
    InvalidDay,
    InvalidZone,
    AmbiguousTime,
    NonexistentTime,
    NotFuture,
    InvalidDeadline,
    OutOfRange,
    NonIncreasing,
    InvalidInterval,
    InvalidWindow,
}

impl fmt::Display for TimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidClock => "invalid clock identity",
            Self::UntrustedClock => "trusted wall clock required",
            Self::InvalidTime => "time must be HH:MM in the 24-hour clock",
            Self::InvalidDay => "day must be next, today, tomorrow, or YYYY-MM-DD",
            Self::InvalidZone => "unknown or invalid IANA time zone",
            Self::AmbiguousTime => "local time is ambiguous; choose earlier or later",
            Self::NonexistentTime => "local time does not exist in that time zone",
            Self::NotFuture => "requested time must be in the future",
            Self::InvalidDeadline => "invalid persisted deadline",
            Self::OutOfRange => "time is outside the supported range",
            Self::NonIncreasing => "calendar occurrence resolves to an earlier instant",
            Self::InvalidInterval => {
                "repeat every 15 minutes to 24 hours, in minutes or in hours, not both"
            }
            Self::InvalidWindow => "a window needs both from and to, each HH:MM",
        })
    }
}

impl std::error::Error for TimeError {}

impl Deadline {
    /// True when [`Self::check`] fails only for this build's zone rules: a
    /// zone they don't know, or a newer tzdb's rules for one they do, so the
    /// stored offset is not the one they give (#1680). The stored time must
    /// still hold together: its `local` names its own instant, `unix_ms`,
    /// with an offset, and a zone that could be a name. Anything else is a
    /// damaged deadline, not a newer one.
    pub fn zone_rules_differ(&self) -> bool {
        match self.check() {
            Err(TimeError::InvalidZone) => true,
            Err(TimeError::InvalidDeadline) => {
                let Some((stamp, zone)) = self
                    .local
                    .strip_suffix(']')
                    .and_then(|s| s.rsplit_once('['))
                else {
                    return false;
                };
                let weekday_fits = self.repeat.as_ref().is_none_or(|repeat| {
                    let (_, _, date, weekday) = repeat.parts();
                    parse_date(date).is_ok_and(|date| {
                        weekday
                            .is_none_or(|day| day.index() == date.weekday().to_monday_zero_offset())
                    })
                });
                named_zone(zone).is_ok()
                    && !(self.elapsed.is_some() && self.repeat.is_some())
                    && weekday_fits
                    && stamp
                        .parse::<Timestamp>()
                        .is_ok_and(|at| at.as_millisecond() == self.unix_ms)
            }
            _ => false,
        }
    }
}

pub fn validate_zone(zone: &str) -> Result<(), TimeError> {
    named_zone(zone).map(|_| ())
}

/// Resolve one new request. Even an elapsed timer needs trusted wall time to
/// establish a persisted fallback for a later boot.
pub fn resolve(when: &When, now: &ClockSample, default_zone: &str) -> Result<Deadline, TimeError> {
    now.check()?;
    if !now.wall_trusted {
        return Err(TimeError::UntrustedClock);
    }
    match when {
        When::After { seconds } => {
            if !(1..=MAX_AFTER_SECONDS).contains(seconds) {
                return Err(TimeError::OutOfRange);
            }
            let ms = seconds.checked_mul(1_000).ok_or(TimeError::OutOfRange)?;
            let unix_ms = now
                .unix_ms
                .checked_add(i64::try_from(ms).map_err(|_| TimeError::OutOfRange)?)
                .ok_or(TimeError::OutOfRange)?;
            let at_boot_ms = now.boot_ms.checked_add(ms).ok_or(TimeError::OutOfRange)?;
            let local = timestamp(unix_ms)?.to_zoned(named_zone(default_zone)?);
            Ok(Deadline {
                unix_ms,
                elapsed: Some(Elapsed {
                    boot_id: now.boot_id.clone(),
                    at_boot_ms,
                }),
                repeat: None,
                local: local.to_string(),
            })
        }
        When::At {
            time,
            day,
            zone,
            fold,
        } => {
            let time = parse_time(time)?;
            let zone = named_zone(zone.as_deref().unwrap_or(default_zone))?;
            let here = timestamp(now.unix_ms)?.to_zoned(zone.clone());
            let (date, next) = match day.as_deref() {
                None | Some("next") => (here.date(), true),
                Some("today") => (here.date(), false),
                Some("tomorrow") => (add_days(here.date(), 1)?, false),
                Some(day) => (parse_date(day)?, false),
            };
            let fold = fold.unwrap_or(Fold::Reject);
            let actual = if next {
                next_one_off(date, time, &zone, fold, &here)?
            } else {
                one_off(date, time, &zone, fold)?
            };
            let result = deadline(actual, None)?;
            if result.unix_ms <= now.unix_ms {
                return Err(TimeError::NotFuture);
            }
            Ok(result)
        }
        When::Daily { time, zone } => {
            first_repeat(time, zone.as_deref().unwrap_or(default_zone), None, now)
        }
        When::Weekly {
            weekday,
            time,
            zone,
        } => first_repeat(
            time,
            zone.as_deref().unwrap_or(default_zone),
            Some(*weekday),
            now,
        ),
        When::Every {
            minutes,
            hours,
            from,
            to,
            zone,
        } => first_every(
            every_minutes(*minutes, *hours)?,
            from.as_deref(),
            to.as_deref(),
            zone.as_deref().unwrap_or(default_zone),
            now,
        ),
    }
}

/// Most occurrences one catch-up steps over: about eleven years of a daily.
const MOST_STEPS: usize = 4096;

/// After time off, the newest occurrence of a repeat that has passed, so only
/// that one is delivered, marked late (#811, the first builder's 13:21: a
/// person who was away does not hear 'late' N times). A one-off, an untrusted
/// wall clock, or an occurrence not yet passed is `previous` as it is.
pub fn newest_due(previous: &Deadline, now: &ClockSample) -> Result<Deadline, TimeError> {
    let mut newest = previous.clone();
    if previous.repeat.is_none() || !now.wall_trusted {
        return Ok(newest);
    }
    for _ in 0..MOST_STEPS {
        match advance(&newest)? {
            Some(next) if next.unix_ms <= now.unix_ms => newest = next,
            _ => break,
        }
    }
    Ok(newest)
}

/// The occurrence after an acknowledged one that is still ahead: any that
/// passed while it waited are stepped over, not delivered after it. With an
/// untrusted wall clock, exactly the next one.
pub fn next_ahead(previous: &Deadline, now: &ClockSample) -> Result<Option<Deadline>, TimeError> {
    let Some(mut next) = advance(previous)? else {
        return Ok(None);
    };
    if now.wall_trusted {
        for _ in 0..MOST_STEPS {
            if next.unix_ms > now.unix_ms {
                break;
            }
            match advance(&next)? {
                Some(after) => next = after,
                None => break,
            }
        }
    }
    Ok(Some(next))
}

/// Advance exactly one requested calendar occurrence. Catching up after time
/// off is [`newest_due`]'s and [`next_ahead`]'s, which know the time now.
pub fn advance(previous: &Deadline) -> Result<Option<Deadline>, TimeError> {
    previous.check()?;
    let Some(repeat) = &previous.repeat else {
        return Ok(None);
    };
    if matches!(repeat, Repeat::Every { .. }) {
        return next_every(previous, repeat).map(Some);
    }
    let (time, zone_name, date, weekday) = repeat.parts();
    let date = add_days(parse_date(date)?, if weekday.is_some() { 7 } else { 1 })?;
    let zone = named_zone(zone_name)?;
    let next = repeated_deadline(date, parse_time(time)?, &zone, weekday)?;
    if next.unix_ms < previous.unix_ms {
        // The requested civil date always advances. A whole-day zone jump can
        // map two different dates to the same instant; retain both occurrences
        // for the owner's receipt-driven advancement instead of losing one.
        return Err(TimeError::NonIncreasing);
    }
    Ok(Some(next))
}

/// Signed milliseconds remaining, or `None` when a wall clock is needed but
/// untrusted. A nonpositive result means due; it is not a delivery receipt.
pub fn remaining_ms(deadline: &Deadline, now: &ClockSample) -> Result<Option<i64>, TimeError> {
    deadline.check()?;
    now.check()?;
    if let Some(elapsed) = &deadline.elapsed
        && elapsed.boot_id == now.boot_id
    {
        let remaining = i128::from(elapsed.at_boot_ms) - i128::from(now.boot_ms);
        return i64::try_from(remaining)
            .map(Some)
            .map_err(|_| TimeError::OutOfRange);
    }
    if !now.wall_trusted {
        return Ok(None);
    }
    deadline
        .unix_ms
        .checked_sub(now.unix_ms)
        .map(Some)
        .ok_or(TimeError::OutOfRange)
}

/// When an item was set, from its stored `created_unix_ms`, said in the
/// lamp's zone `zone` as it is now: its RFC 9557 local time, and its day
/// against `now`'s: `today`, `yesterday`, or its own `YYYY-MM-DD` (also
/// when `now`'s wall clock is untrusted, so no day is told as today on a
/// guess). Every Set needs a trusted wall clock ([`resolve`]), so each
/// stored item's set time is one; a zone this build can't name is an
/// error, told as unknown.
pub fn set_at(unix_ms: i64, now: &ClockSample, zone: &str) -> Result<(String, String), TimeError> {
    let zone = named_zone(zone)?;
    let set = timestamp(unix_ms)?.to_zoned(zone.clone());
    let date = set.date();
    let today = if now.wall_trusted {
        Some(timestamp(now.unix_ms)?.to_zoned(zone).date())
    } else {
        None
    };
    let day = match today {
        Some(today) if today == date => "today".to_owned(),
        Some(today) if today.yesterday().ok() == Some(date) => "yesterday".to_owned(),
        _ => date.to_string(),
    };
    Ok((set.to_string(), day))
}

/// Whether `unix_ms` falls on a day before `now`'s in the lamp's zone
/// `zone` (#5258, #5219: a reminder due before midnight is not said after
/// it). None while `now`'s wall clock is not trusted: no day is told on a
/// guess.
pub fn earlier_day(unix_ms: i64, now: &ClockSample, zone: &str) -> Result<Option<bool>, TimeError> {
    if !now.wall_trusted {
        return Ok(None);
    }
    let zone = named_zone(zone)?;
    let then = timestamp(unix_ms)?.to_zoned(zone.clone()).date();
    let today = timestamp(now.unix_ms)?.to_zoned(zone).date();
    Ok(Some(then < today))
}

fn first_repeat(
    raw_time: &str,
    zone_name: &str,
    weekday: Option<Weekday>,
    now: &ClockSample,
) -> Result<Deadline, TimeError> {
    let time = parse_time(raw_time)?;
    let zone = named_zone(zone_name)?;
    let mut date = timestamp(now.unix_ms)?.to_zoned(zone.clone()).date();
    if let Some(weekday) = weekday {
        let delta = (weekday.index() - date.weekday().to_monday_zero_offset()).rem_euclid(7);
        date = add_days(date, i64::from(delta))?;
    }
    let actual = compatible(date, time, &zone)?;
    if actual.timestamp().as_millisecond() > now.unix_ms {
        return repeated_deadline(date, time, &zone, weekday);
    }
    let date = add_days(date, if weekday.is_some() { 7 } else { 1 })?;
    let candidate = repeated_deadline(date, time, &zone, weekday)?;
    if candidate.unix_ms <= now.unix_ms {
        return Err(TimeError::NonIncreasing);
    }
    Ok(candidate)
}

fn repeated_deadline(
    date: Date,
    time: Time,
    zone: &TimeZone,
    weekday: Option<Weekday>,
) -> Result<Deadline, TimeError> {
    let actual = compatible(date, time, zone)?;
    let time = format!("{:02}:{:02}", time.hour(), time.minute());
    let zone = zone.iana_name().ok_or(TimeError::InvalidZone)?.to_owned();
    let date = date.to_string();
    let repeat = match weekday {
        None => Repeat::Daily { time, zone, date },
        Some(weekday) => Repeat::Weekly {
            weekday,
            time,
            zone,
            date,
        },
    };
    deadline(actual, Some(repeat))
}

fn next_one_off(
    date: Date,
    time: Time,
    zone: &TimeZone,
    fold: Fold,
    now: &Zoned,
) -> Result<Zoned, TimeError> {
    let civil = date.to_datetime(time);
    let ambiguous = zone.to_ambiguous_zoned(civil);
    // A next-occurrence request should not fail on a gap/fold that is already
    // wholly in the past. A future ambiguity still requires an explicit choice.
    let past = match ambiguous.offset() {
        AmbiguousOffset::Gap { .. } => civil <= now.datetime(),
        AmbiguousOffset::Fold { .. } => {
            ambiguous
                .later()
                .map_err(|_| TimeError::OutOfRange)?
                .timestamp()
                <= now.timestamp()
        }
        AmbiguousOffset::Unambiguous { .. } => false,
    };
    if !past {
        let candidate = one_off(date, time, zone, fold)?;
        if candidate.timestamp() > now.timestamp() {
            return Ok(candidate);
        }
    }
    one_off(add_days(date, 1)?, time, zone, fold)
}

fn one_off(date: Date, time: Time, zone: &TimeZone, fold: Fold) -> Result<Zoned, TimeError> {
    let ambiguous = zone.to_ambiguous_zoned(date.to_datetime(time));
    match ambiguous.offset() {
        AmbiguousOffset::Gap { .. } => Err(TimeError::NonexistentTime),
        AmbiguousOffset::Fold { .. } if fold == Fold::Reject => Err(TimeError::AmbiguousTime),
        _ => match fold {
            Fold::Earlier => ambiguous.earlier(),
            Fold::Later => ambiguous.later(),
            Fold::Reject => ambiguous.unambiguous(),
        }
        .map_err(|_| TimeError::OutOfRange),
    }
}

fn compatible(date: Date, time: Time, zone: &TimeZone) -> Result<Zoned, TimeError> {
    zone.to_ambiguous_zoned(date.to_datetime(time))
        .compatible()
        .map_err(|_| TimeError::OutOfRange)
}

fn deadline(actual: Zoned, repeat: Option<Repeat>) -> Result<Deadline, TimeError> {
    let unix_ms = actual.timestamp().as_millisecond();
    timestamp(unix_ms)?;
    Ok(Deadline {
        unix_ms,
        elapsed: None,
        repeat,
        local: actual.to_string(),
    })
}

fn timestamp(unix_ms: i64) -> Result<Timestamp, TimeError> {
    if !(MIN_UNIX_MS..=MAX_UNIX_MS).contains(&unix_ms) {
        return Err(TimeError::OutOfRange);
    }
    // Jiff 0.2.38's millisecond constructor excludes the fractional part of its
    // maximum second, even though Timestamp::MAX includes it. Construct the
    // checked seconds/subsecond pair so our documented inclusive bound holds.
    Timestamp::new(unix_ms / 1_000, (unix_ms % 1_000) as i32 * 1_000_000)
        .map_err(|_| TimeError::OutOfRange)
}

fn check_id(id: &str) -> Result<(), TimeError> {
    if id.is_empty()
        || id.len() > MAX_ID_BYTES
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
    {
        return Err(TimeError::InvalidClock);
    }
    Ok(())
}

fn named_zone(name: &str) -> Result<TimeZone, TimeError> {
    if name.is_empty()
        || name.len() > MAX_ZONE_BYTES
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/_-+".contains(&b))
    {
        return Err(TimeError::InvalidZone);
    }
    let zone = TimeZoneDatabase::bundled()
        .get(name)
        .map_err(|_| TimeError::InvalidZone)?;
    zone.iana_name().ok_or(TimeError::InvalidZone)?;
    Ok(zone)
}

fn parse_time(raw: &str) -> Result<Time, TimeError> {
    let b = raw.as_bytes();
    if b.len() != 5 || b[2] != b':' || ![b[0], b[1], b[3], b[4]].iter().all(u8::is_ascii_digit) {
        return Err(TimeError::InvalidTime);
    }
    let hour = ((b[0] - b'0') * 10 + b[1] - b'0') as i8;
    let minute = ((b[3] - b'0') * 10 + b[4] - b'0') as i8;
    Time::new(hour, minute, 0, 0).map_err(|_| TimeError::InvalidTime)
}

fn parse_date(raw: &str) -> Result<Date, TimeError> {
    let b = raw.as_bytes();
    if b.len() != 10
        || b[4] != b'-'
        || b[7] != b'-'
        || !b
            .iter()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit())
    {
        return Err(TimeError::InvalidDay);
    }
    raw.parse().map_err(|_| TimeError::InvalidDay)
}

fn add_days(date: Date, days: i64) -> Result<Date, TimeError> {
    date.checked_add(days.days())
        .map_err(|_| TimeError::OutOfRange)
}
