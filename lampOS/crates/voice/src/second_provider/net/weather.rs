//! The weather for any place and day, asked of Open-Meteo only when the
//! model calls [`NAME`] (`LAMP_SECOND_WEATHER=on`). The place asked about is
//! looked up by name, the lamp's own when none is named, then its forecast
//! now and for the week ahead is fetched and answered in words for the day
//! asked. Each place's forecast is kept [`KEPT_FOR`], so "and Saturday?"
//! is answered at once; nothing is fetched ahead and no task runs.
//!
//! The owner on 22 Oct: fetching the lamp's own place every 15 minutes and
//! telling it in every request was "a really dumb approach, both fetching
//! weather every 15 minutes and only one location"; "what's the weather in
//! Hanoi tomorrow?" had been answered with Ho Chi Minh City's. The first
//! builder on #1712 (15:18): Open-Meteo, free and with no key. Only the
//! place's name, the lamp's country's two-letter code with it
//! ([`Weather::with_country`], #7593), and the place's coordinates leave
//! the lamp, to Open-Meteo; with `LAMP_WEATHER_BACKUP=on` (off unless set,
//! #7593), when its forecast can't be reached, the coordinates alone, to 4
//! decimals, go to MET Norway (`api.met.no`) for the same forecast ([`met`]).
pub mod met;

use super::{Error, Result, get_public_json_within, public::OPEN_WITHIN};
use futures_util::future::BoxFuture;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::Instant;

/// The switch, in the voice service's environment.
pub const ENVIRONMENT: &str = "LAMP_SECOND_WEATHER";
/// The function's name, as the model calls it.
pub const NAME: &str = "weather";
/// A call waits this long for its place and its forecast together, then
/// answers that the weather couldn't be reached. Longer than a search's
/// ([`super::search::WITHIN`], 3 s): on lamp-4ace on the night of 22 Oct, 2 of
/// 5 Open-Meteo calls took past 3 s and one 2.4 s, and the person was told
/// the weather wouldn't come through while it was on its way. The lamp says
/// it is checking meanwhile, and a right answer a little later is the owner's
/// choice ("quality first").
pub const WITHIN: Duration = Duration::from_secs(6);
/// What a forecast's open bound leaves of [`WITHIN`] for its answer once
/// its connection is open: the request sent and the answer read over it,
/// about two round trips of lamp-9adf's 0.33 s (18 Nov) and Open-Meteo's
/// own work; an estimate, not timed on its own on a lamp.
pub const FORECAST_ANSWERED_IN: Duration = Duration::from_millis(800);
/// A place's forecast is answered from what was kept while it is this
/// fresh, never older.
pub const KEPT_FOR: Duration = Duration::from_secs(10 * 60);
/// The most places kept at once; a new one puts out the oldest.
pub const MOST_PLACES: usize = 16;
/// Where a place's name was found is kept this long: a place doesn't move,
/// so a second look for it, or the first after its midnight, asks only its
/// forecast. One request in place of two: an estimate of about half a
/// look's 2.3–2.4 s on lamp-4ace on 22 Oct (the owner's try), not yet timed
/// on a lamp.
pub const PLACE_KEPT_FOR: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// At most this many places' names are kept with where they are.
const MOST_FOUND: usize = 32;
/// The most names one kept place is found by ("Hanoi", "Ha Noi").
const MOST_NAMES: usize = 8;
/// The longest place name looked up, in characters.
const MOST_NAME_CHARS: usize = 100;
/// A place found anywhere goes over the first of its name in the lamp's
/// own country ([`Weather::with_country`]) only when both say how many
/// live there and it is a city of at least [`LARGE_CITY`] people and
/// [`FAR_LARGER`] times as many as that one: "Sapa" asked of a lamp in
/// Vietnam is Sa Pa, Lao Cai, not Sapa, Romania (lamp-9adf, 18 Nov,
/// #7593), while "Paris" asked of one in the US is still Paris, France,
/// not Paris, Texas. A place at home with no count is chosen: unknown is
/// not small.
const FAR_LARGER: u64 = 10;
/// See [`FAR_LARGER`].
const LARGE_CITY: u64 = 100_000;
const GEOCODING: &str = "https://geocoding-api.open-meteo.com/v1/search";
const FORECAST: &str = "https://api.open-meteo.com/v1/forecast";

/// What the model is told of the function.
pub(crate) const DESCRIPTION: &str = "The weather for any place and day, from a forecast: now, today, tomorrow and each day of the week ahead. Call this for every question about the weather, the temperature or rain, wherever and whenever they ask about, rather than a search or a guess. Leave out place for here, where the lamp is. Answer from what it gives in a sentence or two, and say which place you are answering for, by the name it gives.";

/// The error a place not found answers. Never another place's weather.
pub const NOT_FOUND: &str = "I couldn't find that place";
/// The error a failed lookup or fetch answers. Never weather kept past
/// [`KEPT_FOR`], nor another place's.
pub const UNREACHED: &str = "the weather couldn't be reached just now";
/// The error a call naming no place answers when the lamp's own is not
/// known.
pub const NO_HERE: &str = "I don't know where the lamp is: ask them which place";
/// The error a day the forecast does not reach answers.
pub const NO_SUCH_DAY: &str = "the forecast doesn't reach that day";

/// Whether the switch is on: `on` exactly.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// The function as the model is told it ([`DESCRIPTION`]): two optional
/// words, the place and the day.
pub fn declaration() -> lamp_gemini::Tool {
    lamp_gemini::Tool {
        name: NAME.into(),
        description: DESCRIPTION.into(),
        parameters: Some(json!({
            "type": "object",
            "properties": {
                "place": {
                    "type": "string",
                    "description": "The place asked about, as they said it: \"Hanoi\", \"Paris, France\". Leave it out for here."
                },
                "day": {
                    "type": "string",
                    "enum": ["today", "tomorrow", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"],
                    "description": "The day asked about. Leave it out for now and the days ahead, or for a weekend or a week."
                }
            }
        })),
        silent: false,
    }
}

/// How a request's JSON answer is fetched, its connection open within the
/// bound given: from Open-Meteo on the lamp ([`open_meteo`]), and a canned
/// one in a test.
pub type Fetch = Arc<dyn Fn(String, Duration) -> BoxFuture<'static, Result<Value>> + Send + Sync>;

/// Open-Meteo's answers, fetched over HTTPS with no key.
pub fn open_meteo() -> Fetch {
    Arc::new(|url, open_within| {
        Box::pin(async move { get_public_json_within(&url, open_within).await })
    })
}

/// How long a forecast fetched with `left` of the call's [`WITHIN`] still
/// to come may take to open its connection: all but
/// [`FORECAST_ANSWERED_IN`] of it, never less than a place's own
/// [`OPEN_WITHIN`]. lamp-9adf on 18 Nov (#7593): a place looked up in
/// 1.2 s left its forecast 4.8 s, but the forecast's one address opened
/// none of its tries in a fixed 2.5 s and the call failed in 3.7 s; with
/// this it has 4 s. A forecast of a place known before has 5.2 s.
pub fn forecast_open_within(left: Duration) -> Duration {
    left.saturating_sub(FORECAST_ANSWERED_IN).max(OPEN_WITHIN)
}

/// The name a place is looked up by: its first part ("Hanoi" for "Hanoi,
/// Vietnam").
pub fn place_query(place: &str) -> Option<String> {
    let name = place.split(',').next()?.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

/// The geocoding request for `name`, anywhere, or only in `country` (its
/// two-letter code, `countryCode`).
pub fn geocoding_url(name: &str, country: Option<&str>) -> Option<String> {
    let mut url = reqwest::Url::parse_with_params(
        GEOCODING,
        [
            ("name", name),
            ("count", "1"),
            ("language", "en"),
            ("format", "json"),
        ],
    )
    .ok()?;
    if let Some(country) = country {
        url.query_pairs_mut().append_pair("countryCode", country);
    }
    Some(url.into())
}

/// How many people live at the first place found, as the geocoding says;
/// `None` when it doesn't say (unknown, not none).
fn population(answer: &Value) -> Option<u64> {
    answer["results"][0]["population"].as_u64()
}

/// The place a name finds from its two answers, anywhere and in the
/// lamp's own country: the one in the lamp's country while there is one,
/// unless the one found anywhere is known to be a far larger city than
/// the one at home is known to be ([`FAR_LARGER`]).
fn chosen(anywhere: &Value, at_home: &Value) -> Option<Place> {
    let Some(home) = found(at_home) else {
        return found(anywhere);
    };
    if let (Some(far), Some(near)) = (population(anywhere), population(at_home))
        && far >= LARGE_CITY
        && far >= near.saturating_mul(FAR_LARGER)
        && let Some(far) = found(anywhere)
    {
        return Some(far);
    }
    Some(home)
}

/// A country code as the geocoding takes it: two capital letters.
fn country_code(code: &str) -> Option<String> {
    let code = code.trim();
    (code.len() == 2 && code.bytes().all(|b| b.is_ascii_uppercase())).then(|| code.to_owned())
}

/// The first place found: its latitude and longitude.
pub fn coordinates(answer: &Value) -> Option<(f64, f64)> {
    let first = answer["results"].as_array()?.first()?;
    Some((first["latitude"].as_f64()?, first["longitude"].as_f64()?))
}

/// A place the geocoding found: its name with its country, as the model
/// says it ("Hanoi, Vietnam"), and where it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
}

/// The first place found, named with its country when the answer gives
/// one. `None` when nothing was found.
pub fn found(answer: &Value) -> Option<Place> {
    let (latitude, longitude) = coordinates(answer)?;
    let first = &answer["results"][0];
    let words = |field: &str| {
        first[field]
            .as_str()
            .map(|text| {
                text.trim()
                    .chars()
                    .take(MOST_NAME_CHARS)
                    .collect::<String>()
            })
            .filter(|text| !text.is_empty())
    };
    let name = words("name")?;
    let name = match words("country").filter(|country| *country != name) {
        Some(country) => format!("{name}, {country}"),
        None => name,
    };
    Some(Place {
        name,
        latitude,
        longitude,
    })
}

/// The forecast request for a place: now, and each day of the week ahead.
pub fn forecast_url(latitude: f64, longitude: f64) -> Option<String> {
    reqwest::Url::parse_with_params(
        FORECAST,
        [
            ("latitude", latitude.to_string().as_str()),
            ("longitude", longitude.to_string().as_str()),
            (
                "current",
                "temperature_2m,apparent_temperature,relative_humidity_2m,weather_code",
            ),
            (
                "daily",
                "weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max",
            ),
            ("timezone", "auto"),
            ("forecast_days", "7"),
        ],
    )
    .ok()
    .map(String::from)
}

/// A WMO weather code in words.
fn sky(code: i64) -> &'static str {
    match code {
        0 => "clear",
        1 => "mainly clear",
        2 => "partly cloudy",
        3 => "overcast",
        45 | 48 => "foggy",
        51 | 53 | 55 | 56 | 57 => "drizzle",
        61 | 66 => "light rain",
        63 => "rain",
        65 | 67 => "heavy rain",
        71 | 73 | 75 | 77 | 85 | 86 => "snow",
        80 => "light showers",
        81 => "showers",
        82 => "heavy showers",
        95 => "thunderstorms",
        96 | 99 => "thunderstorms with hail",
        _ => "mixed weather",
    }
}

/// The days of the week, Sunday first.
const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// The days since 1 January 1970 of a `YYYY-MM-DD` date, as the forecast
/// dates its days (Hinnant's `days_from_civil`).
fn day_number(date: &str) -> Option<i64> {
    let mut parts = date.splitn(3, '-').map(|part| part.parse::<i64>().ok());
    let (year, month, day) = (parts.next()??, parts.next()??, parts.next()??);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let of_era = year - era * 400;
    let of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    Some(era * 146_097 + of_era * 365 + of_era / 4 - of_era / 100 + of_year - 719_468)
}

/// The day of the week of a `YYYY-MM-DD` date, as the forecast names its
/// days. 1 January 1970 was a Thursday.
fn weekday(date: &str) -> Option<&'static str> {
    day_number(date).map(|day| WEEKDAYS[(day + 4).rem_euclid(7) as usize])
}

/// The place's own day at `unix_now` (seconds since 1970, UTC), as a
/// [`day_number`]: the time moved by the forecast's offset
/// (`utc_offset_seconds`, the place's with `timezone=auto`), never the
/// lamp's. `None` for a forecast without one.
pub fn place_day(forecast: &Value, unix_now: i64) -> Option<i64> {
    let offset = forecast["utc_offset_seconds"].as_i64()?;
    Some((unix_now + offset).div_euclid(86_400))
}

/// The day a call asks about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Day {
    /// Now and every day of the forecast: no day given, or one not read.
    Ahead,
    Today,
    Tomorrow,
    /// The first day of the forecast from today on with this weekday,
    /// today's included.
    Weekday(&'static str),
}

impl Day {
    /// The day a call's `day` names: `today`, `tomorrow` or a weekday, in
    /// any case, a weekday's first three letters or more too. Anything
    /// else is [`Day::Ahead`], so the model is given every day by its name
    /// and picks.
    pub fn asked(value: Option<&str>) -> Self {
        let Some(value) = value.map(|value| value.trim().to_lowercase()) else {
            return Day::Ahead;
        };
        match value.as_str() {
            "today" | "tonight" => Day::Today,
            "tomorrow" => Day::Tomorrow,
            _ => WEEKDAYS
                .into_iter()
                .find(|day| value.len() >= 3 && day.to_lowercase().starts_with(&value))
                .map_or(Day::Ahead, Day::Weekday),
        }
    }

    /// The day in the journal's words.
    fn said(self) -> &'static str {
        match self {
            Day::Ahead => "now and the days ahead",
            Day::Today => "today",
            Day::Tomorrow => "tomorrow",
            Day::Weekday(day) => day,
        }
    }
}

/// The weather now in a few words for `place`, in degrees Celsius. `None`
/// without the weather now.
fn now_said(answer: &Value, place: &str) -> Option<String> {
    let now = &answer["current"];
    let mut said = format!(
        "In {place} now: {}°C, {}",
        degrees(&now["temperature_2m"])?,
        sky(now["weather_code"].as_i64()?)
    );
    if let Some(feels) = degrees(&now["apparent_temperature"]) {
        said.push_str(&format!(", feels like {feels}°C"));
    }
    if let Some(humidity) = now["relative_humidity_2m"].as_i64() {
        said.push_str(&format!(", humidity {humidity}%"));
    }
    said.push('.');
    Some(said)
}

fn degrees(value: &Value) -> Option<i64> {
    value.as_f64().map(|value| value.round() as i64)
}

/// The forecast's days from the place's `today` on (a [`day_number`]):
/// each row and how many days after today it is, from the row's own date.
/// The reviewers on #2969: a forecast kept from 23:59 at the place said
/// the day before as today after the place's midnight, when today was the
/// first row. Without `today`, a forecast just as it came, its first row
/// is today. A row without a date counts from its place. `None` when
/// `today` is given and no row has its date: the forecast is too old to
/// tell today from.
fn days_from(daily: &Value, today: Option<i64>) -> Option<Vec<(usize, i64)>> {
    let rows = daily["weather_code"].as_array().map_or(0, Vec::len);
    let dated = |row: usize| {
        daily["time"]
            .get(row)
            .and_then(Value::as_str)
            .and_then(day_number)
    };
    let (first, today) = match today {
        Some(today) => (
            (0..rows).position(|row| dated(row) == Some(today))?,
            Some(today),
        ),
        None => (0, dated(0)),
    };
    Some(
        (first..rows)
            .filter_map(|row| {
                let after = match (dated(row), today) {
                    (Some(day), Some(today)) => day - today,
                    _ => (row - first) as i64,
                };
                (after >= 0).then_some((row, after))
            })
            .collect(),
    )
}

/// The forecast's row `row`, `after` days after today, in a few words: its
/// name, its low and high, its sky and its chance of rain. Today and
/// tomorrow carry their weekday too, so "this weekend" asked on a Friday is
/// plain (the reviewer on #2185). `None` for a row it lacks, or a later day
/// without a date to name it by.
fn day_said(daily: &Value, row: usize, after: i64) -> Option<String> {
    let at = |field: &str| daily[field].get(row).cloned().unwrap_or(Value::Null);
    let named = at("time").as_str().and_then(weekday);
    let name = match (after, named) {
        (0, Some(named)) => format!("Today, {named},"),
        (0, None) => "Today".to_owned(),
        (1, Some(named)) => format!("Tomorrow, {named},"),
        (1, None) => "Tomorrow".to_owned(),
        (_, Some(named)) => named.to_owned(),
        (_, None) => return None,
    };
    let (low, high, code) = (
        degrees(&at("temperature_2m_min"))?,
        degrees(&at("temperature_2m_max"))?,
        at("weather_code").as_i64()?,
    );
    let mut said = format!("{name} {low} to {high}°C, {}", sky(code));
    if let Some(rain) = at("precipitation_probability_max").as_i64() {
        said.push_str(&format!(", {rain}% chance of rain"));
    } else if let Some(rain) = at("precipitation_sum").as_f64() {
        said.push_str(&rain_said(rain));
    }
    said.push('.');
    Some(said)
}

/// A day's rain in mm, for a forecast that gives no chance of it (MET
/// Norway's, [`met`]): "no rain" under 0.05 mm, then to a tenth of a mm
/// below 10 and to a whole one from there.
fn rain_said(mm: f64) -> String {
    if mm < 0.05 {
        return ", no rain".into();
    }
    let amount = if mm < 10.0 {
        format!("{mm:.1}").trim_end_matches(".0").to_owned()
    } else {
        format!("{mm:.0}")
    };
    format!(", {amount} mm of rain")
}

/// The forecast in a few words for `place`, as it came: now, today and
/// tomorrow (with their weekdays when dated) and each later day by its
/// name, in degrees Celsius. `None` without the weather now.
pub fn summary(answer: &Value, place: &str) -> Option<String> {
    said_for(answer, place, Day::Ahead, None)
}

/// The forecast in a few words for `place` and the day asked, its days
/// told from the place's `today` ([`days_from`]): every day from today with
/// the weather now for [`Day::Ahead`], today's with the weather now for
/// today, and the one day alone for any other. `None` for a day the
/// forecast does not reach, and for [`Day::Ahead`] without the weather now.
pub fn said_for(answer: &Value, place: &str, day: Day, today: Option<i64>) -> Option<String> {
    let daily = &answer["daily"];
    let days = days_from(daily, today)?;
    if day == Day::Ahead {
        let mut said = now_said(answer, place)?;
        for line in days
            .iter()
            .filter_map(|&(row, after)| day_said(daily, row, after))
        {
            said.push(' ');
            said.push_str(&line);
        }
        return Some(said);
    }
    let &(row, after) = days.iter().find(|&&(row, after)| match day {
        Day::Today => after == 0,
        Day::Tomorrow => after == 1,
        Day::Weekday(name) => {
            daily["time"]
                .get(row)
                .and_then(Value::as_str)
                .and_then(weekday)
                == Some(name)
        }
        Day::Ahead => false,
    })?;
    let line = day_said(daily, row, after)?;
    match now_said(answer, place).filter(|_| after == 0) {
        Some(now) => Some(format!("{now} {line}")),
        None => Some(format!("In {place}: {line}")),
    }
}

/// One place's forecast as it came, the names it was asked by, the
/// place's own day it came on, and when.
struct Kept {
    names: Vec<String>,
    place: Place,
    forecast: Arc<Value>,
    day: i64,
    at: Instant,
}

/// The time now, in seconds since 1970 (UTC): the lamp's clock, and a set
/// one in a test.
pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

/// The lamp's own clock.
pub fn wall_clock() -> Clock {
    Arc::new(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs() as i64)
    })
}

/// What a call was answered, and for the journal what it asked and how it
/// was answered, in words of the lamp's own: never the place's name or
/// anything else the person said.
pub struct Answered {
    pub told: Value,
    pub asked: String,
    pub how: String,
}

/// The weather the model asks for, the lamp's own place for a call that
/// names none, and each place's forecast kept [`KEPT_FOR`] and never past
/// that place's midnight, at most [`MOST_PLACES`] of them.
pub struct Weather {
    here: Option<String>,
    /// The owner's place from the app (`LAMP_OWNER_PLACE=on`, #4333), read
    /// at each call that names no place: kept, it goes over `here`.
    owner_place: Option<std::path::PathBuf>,
    /// The lamp's own country's code ("VN"), whose places a name finds
    /// first ([`chosen`]).
    country: Option<String>,
    fetch: Fetch,
    /// The forecast's backup, MET Norway, and the lamp's zone its days are
    /// told in ([`Weather::with_backup`]).
    backup: Option<(Fetch, met::Offset)>,
    clock: Clock,
    kept: Mutex<Vec<Kept>>,
    /// The places names were found as, by name, kept [`PLACE_KEPT_FOR`].
    found: Mutex<Vec<(String, Place, Instant)>>,
}

impl Weather {
    /// Nothing is fetched until a call asks. `clock` tells each call's
    /// place its own day ([`place_day`]).
    pub fn new(here: Option<String>, fetch: Fetch, clock: Clock) -> Self {
        Self {
            here,
            owner_place: None,
            country: None,
            fetch,
            backup: None,
            clock,
            kept: Mutex::new(Vec::new()),
            found: Mutex::new(Vec::new()),
        }
    }

    /// The same, a call that names no place answered for the owner's
    /// place kept at `file` while there is one: its rounded position, never
    /// looked up by name, said by its name (#4333). None kept, `here`.
    pub fn with_owner_place(mut self, file: std::path::PathBuf) -> Self {
        self.owner_place = Some(file);
        self
    }

    /// The same, a name looked up both anywhere and in `country` (its
    /// two-letter code, as the lamp's zone gives it,
    /// `crate::local_time::country_here`), at once, the place in the
    /// lamp's country chosen while there is one ([`chosen`]). Anything
    /// but two capital letters is no country.
    pub fn with_country(mut self, country: Option<String>) -> Self {
        self.country = country.as_deref().and_then(country_code);
        self
    }

    /// The same, a forecast Open-Meteo can't be reached for, or answers
    /// only with a server error for ([`met::backs_up`]), fetched with `backup` (MET Norway) in what is
    /// left of the call's [`WITHIN`], its days the lamp's own by `offset`,
    /// only while `switch` (`LAMP_WEATHER_BACKUP`) is exactly `on` (#7593).
    /// While it is on, Open-Meteo's forecast leaves it
    /// [`met::LEFT_FOR_IT`]; off, Open-Meteo has the whole 6 s as before.
    pub fn with_backup(mut self, switch: Option<&str>, backup: Fetch, offset: met::Offset) -> Self {
        self.backup = met::on(switch).then_some((backup, offset));
        self
    }

    /// The owner's place now, as the weather looks it up.
    fn owner_here(&self) -> Option<Place> {
        let place = lamp_msgs::owner_place::read(self.owner_place.as_deref()?)?;
        Some(Place {
            name: place.name,
            latitude: place.lat,
            longitude: place.lon,
        })
    }

    /// The lamp's own place, for a call that names none.
    pub fn here(&self) -> Option<&str> {
        self.here.as_deref()
    }

    /// The function's answer for the model's `args`, within [`WITHIN`]:
    /// the place's name and its weather for the day asked, or what went
    /// wrong in a few words.
    pub async fn answer(&self, args: &Value) -> Answered {
        let place = args["place"]
            .as_str()
            .map(str::trim)
            .filter(|place| !place.is_empty());
        let day = Day::asked(args["day"].as_str());
        let asked = format!(
            "for {}, {}",
            if place.is_some() {
                "a place named"
            } else {
                "the lamp's place"
            },
            day.said()
        );
        let now = (self.clock)();
        let until = Instant::now() + WITHIN;
        let (told, how) =
            match tokio::time::timeout_at(until, self.looked_up(place, day, now, until)).await {
                Ok(answered) => answered,
                Err(_) => (
                    json!({"error": UNREACHED}),
                    format!("not reached within {} ms", WITHIN.as_millis()),
                ),
            };
        Answered { told, asked, how }
    }

    /// The call's answer, its days told from the place's own day at `now`,
    /// the call cut at `until`.
    async fn looked_up(
        &self,
        place: Option<&str>,
        day: Day,
        now: i64,
        until: Instant,
    ) -> (Value, String) {
        if place.is_none()
            && let Some(owner) = self.owner_here()
        {
            let key = owner.name.to_lowercase();
            return self.forecast_of(key, owner, true, (day, now), until).await;
        }
        let Some(name) = place.or(self.here.as_deref()) else {
            return (json!({"error": NO_HERE}), "no place, and none here".into());
        };
        let Some(query) =
            place_query(name).filter(|query| query.chars().count() <= MOST_NAME_CHARS)
        else {
            return (json!({"error": NOT_FOUND}), "no such place".into());
        };
        let key = query.to_lowercase();
        if let Some((place, forecast, today)) = self.kept_by_name(&key, now) {
            return (
                answer_for(&place, &forecast, day, today),
                "from what was kept".into(),
            );
        }
        let known = self.found_before(&key);
        let place = match known.clone() {
            Some(place) => place,
            None => {
                let place = match self.place_found(&query).await {
                    Ok(place) => place,
                    Err(error) => {
                        return (
                            json!({"error": UNREACHED}),
                            format!("the place was not looked up: {error}"),
                        );
                    }
                };
                let Some(place) = place else {
                    return (json!({"error": NOT_FOUND}), "no such place".into());
                };
                self.remember_place(key.clone(), place.clone());
                place
            }
        };
        self.forecast_of(key, place, known.is_some(), (day, now), until)
            .await
    }

    /// Where `query` is: looked up anywhere and, with the lamp's country
    /// known, in it too, both at once ([`chosen`]). With one of the two
    /// unreached, the other's; `None` when nothing was found.
    async fn place_found(&self, query: &str) -> Result<Option<Place>> {
        let Some(anywhere) = geocoding_url(query, None) else {
            return Ok(None);
        };
        let at_home = self
            .country
            .as_deref()
            .and_then(|country| geocoding_url(query, Some(country)));
        let Some(at_home) = at_home else {
            return (self.fetch)(anywhere, OPEN_WITHIN)
                .await
                .map(|answer| found(&answer));
        };
        let (anywhere, at_home) = tokio::join!(
            (self.fetch)(anywhere, OPEN_WITHIN),
            (self.fetch)(at_home, OPEN_WITHIN)
        );
        match (anywhere, at_home) {
            (Ok(anywhere), Ok(at_home)) => Ok(chosen(&anywhere, &at_home)),
            (Ok(anywhere), Err(_)) => Ok(found(&anywhere)),
            (Err(error), Ok(at_home)) => found(&at_home).map(Some).ok_or(error),
            (Err(error), Err(_)) => Err(error),
        }
    }

    /// The forecast of `place`, found by the name `key`, from what was kept
    /// or fetched, its connection open within what is left before `until`
    /// ([`forecast_open_within`]); `known`, its place was known before this
    /// call.
    async fn forecast_of(
        &self,
        key: String,
        place: Place,
        known: bool,
        (day, now): (Day, i64),
        until: Instant,
    ) -> (Value, String) {
        if let Some((forecast, today)) = self.kept_by_place(&place, &key, now) {
            return (
                answer_for(&place, &forecast, day, today),
                "its place known, its weather from what was kept".into(),
            );
        }
        let Some(url) = forecast_url(place.latitude, place.longitude) else {
            return (json!({"error": UNREACHED}), "no forecast request".into());
        };
        let left = until.saturating_duration_since(Instant::now());
        // With a backup, its share of the 6 s is left to it (#7593).
        let left = match self.backup {
            Some(_) => left.saturating_sub(met::LEFT_FOR_IT),
            None => left,
        };
        let (forecast, backed_up) = match (self.fetch)(url, forecast_open_within(left)).await {
            Ok(forecast) => (Arc::new(forecast), None),
            Err(error) => match self.backed_up(&place, &error, now, until).await {
                Some(Ok((forecast, took))) => (Arc::new(forecast), Some(took)),
                Some(Err(how)) => return (json!({"error": UNREACHED}), how),
                None => {
                    return (
                        json!({"error": UNREACHED}),
                        format!("the weather was not fetched: {error}"),
                    );
                }
            },
        };
        // The place's today must be one of its rows, with the weather now:
        // a forecast without either is no answer, and is not kept.
        let Some(today) = place_day(&forecast, now)
            .filter(|&today| said_for(&forecast, &place.name, Day::Ahead, Some(today)).is_some())
        else {
            return (
                json!({"error": UNREACHED}),
                "the forecast came with nothing for today there".into(),
            );
        };
        let answered = answer_for(&place, &forecast, day, today);
        self.keep(key, place, forecast, (today, now));
        let how = match backed_up {
            Some(took) => format!("the forecast came from the backup service after {took} ms"),
            None if known => "fetched, its place known".into(),
            None => "fetched".into(),
        };
        (answered, how)
    }

    /// The forecast of `place` from the backup, when there is one and
    /// Open-Meteo's `error` is that it couldn't be reached or answered
    /// only with a server error ([`met::backs_up`]): fetched within what is left before `until`
    /// less its answer's [`FORECAST_ANSWERED_IN`], in Open-Meteo's shape
    /// ([`met::as_open_meteo`]), with how many ms it took. `None` when it
    /// isn't asked; the journal's words when it fails too, never the place.
    async fn backed_up(
        &self,
        place: &Place,
        error: &Error,
        now: i64,
        until: Instant,
    ) -> Option<std::result::Result<(Value, u128), String>> {
        let (fetch, offset) = self.backup.as_ref().filter(|_| met::backs_up(error))?;
        let failed = |why: &dyn std::fmt::Display| {
            format!("the weather was not fetched: {error}; nor from the backup service: {why}")
        };
        let Some(url) = met::url(place.latitude, place.longitude) else {
            return Some(Err(failed(&"no forecast request")));
        };
        let began = Instant::now();
        let open_within = until
            .saturating_duration_since(began)
            .saturating_sub(FORECAST_ANSWERED_IN);
        Some(match fetch(url, open_within).await {
            Ok(answer) => met::as_open_meteo(&answer, offset(now), now)
                .map(|forecast| (forecast, began.elapsed().as_millis()))
                .ok_or_else(|| failed(&"its answer was not read")),
            Err(why) => Err(failed(&why)),
        })
    }

    /// Where `name` was found before, while under [`PLACE_KEPT_FOR`] old.
    fn found_before(&self, name: &str) -> Option<Place> {
        let mut found = self
            .found
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        found.retain(|(_, _, at)| at.elapsed() < PLACE_KEPT_FOR);
        found
            .iter()
            .find(|(kept, _, _)| kept == name)
            .map(|(_, place, _)| place.clone())
    }

    /// Keep where `name` was found, the oldest put out past [`MOST_FOUND`].
    fn remember_place(&self, name: String, place: Place) {
        let mut found = self
            .found
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        found.retain(|(kept, _, _)| *kept != name);
        while found.len() >= MOST_FOUND {
            found.remove(0);
        }
        found.push((name, place, Instant::now()));
    }

    /// The kept places still fresh at `now`: under [`KEPT_FOR`] old, and
    /// fetched on the place's own day now, so the first call after that
    /// place's midnight fetches again. The others are put out.
    fn fresh(&self, now: i64) -> std::sync::MutexGuard<'_, Vec<Kept>> {
        let mut kept = self
            .kept
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        kept.retain(|kept| {
            kept.at.elapsed() < KEPT_FOR && place_day(&kept.forecast, now) == Some(kept.day)
        });
        kept
    }

    /// The fresh place a call's name found before, its forecast, and the
    /// place's day.
    fn kept_by_name(&self, name: &str, now: i64) -> Option<(Place, Arc<Value>, i64)> {
        let kept = self.fresh(now);
        let hit = kept
            .iter()
            .find(|kept| kept.names.iter().any(|kept_name| kept_name == name))?;
        Some((hit.place.clone(), hit.forecast.clone(), hit.day))
    }

    /// The fresh forecast of a place a call's name has just found, and the
    /// place's day, the name kept with it from now on.
    fn kept_by_place(&self, place: &Place, name: &str, now: i64) -> Option<(Arc<Value>, i64)> {
        let mut kept = self.fresh(now);
        let hit = kept.iter_mut().find(|kept| kept.place == *place)?;
        if hit.names.len() >= MOST_NAMES {
            hit.names.remove(0);
        }
        hit.names.push(name.to_owned());
        Some((hit.forecast.clone(), hit.day))
    }

    /// Keep a place's forecast just fetched on the place's day `day` at
    /// `now`, in place of one kept before for it, the oldest put out past
    /// [`MOST_PLACES`].
    fn keep(&self, name: String, place: Place, forecast: Arc<Value>, (day, now): (i64, i64)) {
        let mut kept = self.fresh(now);
        kept.retain(|other| other.place != place);
        for other in kept.iter_mut() {
            other.names.retain(|other_name| *other_name != name);
        }
        while kept.len() >= MOST_PLACES {
            let oldest = (0..kept.len()).min_by_key(|&at| kept[at].at).unwrap_or(0);
            kept.remove(oldest);
        }
        kept.push(Kept {
            names: vec![name],
            place,
            forecast,
            day,
            at: Instant::now(),
        });
    }
}

/// The model's answer for a place's forecast and the day asked, its days
/// told from the place's `today`.
fn answer_for(place: &Place, forecast: &Value, day: Day, today: i64) -> Value {
    match said_for(forecast, &place.name, day, Some(today)) {
        Some(said) => json!({"place": place.name, "weather": said}),
        None => json!({"place": place.name, "error": NO_SUCH_DAY}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_forecast_is_said_now_today_and_tomorrow() {
        let answer = json!({
            "current": {"temperature_2m": 31.2, "apparent_temperature": 36.4,
                        "relative_humidity_2m": 70, "weather_code": 2},
            "daily": {"weather_code": [2, 95], "temperature_2m_max": [33.1, 34.0],
                      "temperature_2m_min": [26.0, 27.4], "precipitation_probability_max": [40, 60]}
        });
        assert_eq!(
            summary(&answer, "Hanoi").unwrap(),
            "In Hanoi now: 31°C, partly cloudy, feels like 36°C, humidity 70%. \
             Today 26 to 33°C, partly cloudy, 40% chance of rain. \
             Tomorrow 27 to 34°C, thunderstorms, 60% chance of rain."
        );
        assert_eq!(summary(&json!({"daily": {}}), "Hanoi"), None, "nothing now");
        let bare = json!({"current": {"temperature_2m": 20.0, "weather_code": 3}});
        assert_eq!(
            summary(&bare, "Hue").unwrap(),
            "In Hue now: 20°C, overcast."
        );
    }

    /// "Will it rain this weekend" is answered from the forecast: the days
    /// after tomorrow go by their names, and one without a date is left out.
    #[test]
    fn the_days_after_tomorrow_are_said_by_their_names() {
        let answer = json!({
            "current": {"temperature_2m": 30.0, "weather_code": 0},
            "daily": {
                "time": ["2026-10-20", "2026-10-21", "2026-10-22", "2026-10-23",
                         "2026-10-24", "2026-10-25", "x"],
                "weather_code": [0, 0, 3, 61, 95, 80, 0],
                "temperature_2m_max": [33, 33, 32, 31, 30, 31, 32],
                "temperature_2m_min": [25, 25, 25, 24, 24, 24, 25]
            }
        });
        let said = summary(&answer, "Ho Chi Minh").unwrap();
        assert!(said.contains(" Thursday 25 to 32°C, overcast."), "{said}");
        assert!(
            said.contains(" Saturday 24 to 30°C, thunderstorms."),
            "{said}"
        );
        assert!(
            said.ends_with(" Sunday 24 to 31°C, light showers."),
            "{said}"
        );
        assert!(
            said.contains(" Today, Tuesday, 25 to 33°C, clear."),
            "{said}"
        );
        assert!(said.contains(" Tomorrow, Wednesday, 25 to 33°C"), "{said}");
        assert_eq!(weekday("2026-10-24"), Some("Saturday"));
        assert_eq!(weekday("2024-02-29"), Some("Thursday"));
        assert_eq!(weekday("2026-13-01"), None);
        assert_eq!(weekday("soon"), None);
    }

    #[test]
    fn a_place_is_looked_up_by_its_first_part() {
        assert_eq!(place_query("Hanoi, Vietnam").as_deref(), Some("Hanoi"));
        assert_eq!(place_query(" , x"), None);
        let url = geocoding_url("Ho Chi Minh City", None).unwrap();
        assert!(
            url.starts_with(
                "https://geocoding-api.open-meteo.com/v1/search?name=Ho+Chi+Minh+City&count=1"
            ),
            "{url}"
        );
        assert!(!url.contains("countryCode"), "{url}");
        let url = geocoding_url("Sapa", Some("VN")).unwrap();
        assert!(url.ends_with("&countryCode=VN"), "{url}");
        let found =
            json!({"results": [{"name": "Hanoi", "latitude": 21.0245, "longitude": 105.84117}]});
        assert_eq!(coordinates(&found), Some((21.0245, 105.84117)));
        assert_eq!(coordinates(&json!({"generationtime_ms": 0.5})), None);
        let forecast = forecast_url(21.0245, 105.84117).unwrap();
        assert!(
            forecast.starts_with(
                "https://api.open-meteo.com/v1/forecast?latitude=21.0245&longitude=105.84117"
            ),
            "{forecast}"
        );
        assert!(forecast.contains("forecast_days=7"));
    }

    #[test]
    fn the_switch_is_on_with_on_alone() {
        assert!(on(Some("on")));
        assert!(!on(Some("yes")) && !on(Some("")) && !on(None));
    }

    /// Open-Meteo's geocoding answer for one place, as it gives it.
    fn geocoded(name: &str, country: &str, latitude: f64, longitude: f64) -> Value {
        json!({"results": [{"id": 1, "name": name, "latitude": latitude,
                            "longitude": longitude, "country": country,
                            "admin1": name}],
               "generationtime_ms": 0.6})
    }

    /// A forecast from Tuesday 20 Oct 2026 at a place 7 hours ahead of
    /// UTC, its temperature now `now`, so each place's is told apart.
    fn forecast(now: f64) -> Value {
        json!({
            "utc_offset_seconds": 25_200, "timezone": "Asia/Bangkok",
            "current": {"temperature_2m": now, "apparent_temperature": now + 4.0,
                        "relative_humidity_2m": 70, "weather_code": 2},
            "daily": {
                "time": ["2026-10-20", "2026-10-21", "2026-10-22", "2026-10-23",
                         "2026-10-24", "2026-10-25", "2026-10-26"],
                "weather_code": [2, 95, 3, 61, 63, 80, 0],
                "temperature_2m_max": [33, 34, 32, 31, 30, 31, 32],
                "temperature_2m_min": [26, 27, 25, 24, 24, 24, 25],
                "precipitation_probability_max": [40, 60, 10, 70, 80, 50, 0]
            }
        })
    }

    /// Seconds since 1970 (UTC) at `hour`:`minute` on `date` at a place
    /// `offset` seconds ahead of UTC.
    fn unix(date: &str, hour: i64, minute: i64, offset: i64) -> i64 {
        day_number(date).unwrap() * 86_400 + hour * 3_600 + minute * 60 - offset
    }

    /// A clock set at noon on Tuesday 20 Oct 2026 at [`forecast`]'s place.
    fn tuesday_noon() -> Clock {
        let now = unix("2026-10-20", 12, 0, 25_200);
        Arc::new(move || now)
    }

    /// A wall clock that moves only when a test moves it, with the
    /// runtime's paused time beside it.
    struct Wall(Arc<std::sync::atomic::AtomicI64>);

    impl Wall {
        fn at(unix: i64) -> Self {
            Self(Arc::new(std::sync::atomic::AtomicI64::new(unix)))
        }

        fn clock(&self) -> Clock {
            let now = self.0.clone();
            Arc::new(move || now.load(std::sync::atomic::Ordering::Relaxed))
        }

        async fn pass(&self, seconds: u64) {
            self.0
                .fetch_add(seconds as i64, std::sync::atomic::Ordering::Relaxed);
            tokio::time::advance(Duration::from_secs(seconds)).await;
        }
    }

    /// A fetcher with canned answers that keeps each URL asked: Hanoi and
    /// Ho Chi Minh City found, any other name not; each forecast by its
    /// latitude; `failing` answers every forecast with an error.
    struct Canned {
        asked: Arc<Mutex<Vec<String>>>,
        failing: Arc<std::sync::atomic::AtomicBool>,
    }

    impl Canned {
        fn new() -> (Self, Fetch) {
            let asked = Arc::new(Mutex::new(Vec::new()));
            let failing = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let (urls, fails) = (asked.clone(), failing.clone());
            let fetch: Fetch = Arc::new(move |url: String, _| {
                urls.lock().unwrap().push(url.clone());
                let fails = fails.load(std::sync::atomic::Ordering::Relaxed);
                Box::pin(async move { canned(&url, fails) })
            });
            (Self { asked, failing }, fetch)
        }

        fn count(&self) -> usize {
            self.asked.lock().unwrap().len()
        }

        fn urls(&self) -> Vec<String> {
            self.asked.lock().unwrap().clone()
        }
    }

    fn canned(url: &str, failing: bool) -> Result<Value> {
        let url = reqwest::Url::parse(url).unwrap();
        let param = |name: &str| {
            url.query_pairs()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.into_owned())
        };
        if url.host_str() == Some("geocoding-api.open-meteo.com") {
            let name = param("name").unwrap_or_default();
            return Ok(match name.as_str() {
                "Hanoi" => geocoded("Hanoi", "Vietnam", 21.0245, 105.84117),
                "Ho Chi Minh City" => geocoded("Ho Chi Minh City", "Vietnam", 10.82302, 106.62965),
                other if other.starts_with("Town ") => {
                    let n: f64 = other[5..].parse().unwrap();
                    geocoded(other, "Nowhere", n, n)
                }
                _ => json!({"generationtime_ms": 0.5}),
            });
        }
        if failing {
            return Err(super::super::Error::Timeout("request"));
        }
        let latitude: f64 = param("latitude").unwrap().parse().unwrap();
        Ok(forecast(if latitude > 20.0 { 21.0 } else { 31.0 }))
    }

    /// The owner on 22 Oct: "what's the weather in Hanoi tomorrow?" was
    /// answered with Ho Chi Minh City's. A call naming Hanoi asks the
    /// geocoding for Hanoi and answers Hanoi's forecast, never the lamp's
    /// place's.
    #[tokio::test]
    async fn the_weather_of_a_place_named_is_that_places() {
        let (canned, fetch) = Canned::new();
        let weather = Weather::new(
            Some("Ho Chi Minh City, Vietnam".into()),
            fetch,
            tuesday_noon(),
        );
        let answered = weather.answer(&json!({"place": "Hanoi"})).await;
        assert_eq!(
            answered.told["place"], "Hanoi, Vietnam",
            "{}",
            answered.told
        );
        let said = answered.told["weather"].as_str().unwrap();
        assert!(said.starts_with("In Hanoi, Vietnam now: 21°C"), "{said}");
        assert!(!said.contains("Ho Chi Minh"), "{said}");
        let urls = canned.urls();
        assert_eq!(urls.len(), 2, "{urls:?}");
        assert!(urls[0].contains("geocoding-api") && urls[0].contains("name=Hanoi&"));
        assert!(urls[1].contains("latitude=21.0245&longitude=105.84117"));
        assert_eq!(answered.asked, "for a place named, now and the days ahead");
        assert_eq!(answered.how, "fetched");
        // "Hanoi, Vietnam" is looked up by its first part, and is the same
        // place.
        let again = weather.answer(&json!({"place": "Hanoi, Vietnam"})).await;
        assert_eq!(again.told, answered.told);
    }

    /// A call naming no place is the lamp's own (`LAMP_PLACE`); with none
    /// known it says so, and nothing is fetched.
    #[tokio::test]
    async fn the_weather_with_no_place_is_the_lamps_own() {
        let (canned, fetch) = Canned::new();
        let weather = Weather::new(
            Some("Ho Chi Minh City, Vietnam".into()),
            fetch,
            tuesday_noon(),
        );
        for args in [json!({}), json!({"place": "  ", "day": null})] {
            let answered = weather.answer(&args).await;
            assert_eq!(answered.told["place"], "Ho Chi Minh City, Vietnam");
            assert!(
                answered.told["weather"]
                    .as_str()
                    .unwrap()
                    .starts_with("In Ho Chi Minh City, Vietnam now: 31°C")
            );
            assert_eq!(
                answered.asked,
                "for the lamp's place, now and the days ahead"
            );
        }
        assert!(canned.urls()[0].contains("name=Ho+Chi+Minh+City&"));
        let (nothing, fetch) = Canned::new();
        let nowhere = Weather::new(None, fetch, tuesday_noon());
        assert_eq!(
            nowhere.answer(&json!({})).await.told,
            json!({"error": NO_HERE})
        );
        assert_eq!(nothing.count(), 0);
    }

    /// #4333: the owner's place from the app goes over the lamp's own for
    /// a call that names none: its rounded position, never looked up by
    /// name, said by its name. A place named is still that place; a clear
    /// puts the lamp's own back.
    #[tokio::test]
    async fn the_weather_with_no_place_is_the_owners_place_while_one_is_kept() {
        let dir = std::env::temp_dir().join(format!("lamp-weather-owner-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("place");
        std::fs::write(
            &file,
            r#"{"name":"Thao Dien, Thu Duc","lat":10.8,"lon":106.74,"set_at":"2026-10-29T08:30:00Z"}"#,
        )
        .unwrap();
        let (canned, fetch) = Canned::new();
        let weather = Weather::new(
            Some("Ho Chi Minh City, Vietnam".into()),
            fetch,
            tuesday_noon(),
        )
        .with_owner_place(file.clone());
        let answered = weather.answer(&json!({})).await;
        assert_eq!(answered.told["place"], "Thao Dien, Thu Duc");
        assert!(
            answered.told["weather"]
                .as_str()
                .unwrap()
                .starts_with("In Thao Dien, Thu Duc now: 31°C"),
            "{}",
            answered.told
        );
        assert_eq!(canned.count(), 1);
        assert!(canned.urls()[0].contains("latitude=10.8&longitude=106.74"));
        let named = weather.answer(&json!({"place": "Hanoi"})).await;
        assert_eq!(named.told["place"], "Hanoi, Vietnam");
        std::fs::remove_file(&file).unwrap();
        let own = weather.answer(&json!({})).await;
        assert_eq!(own.told["place"], "Ho Chi Minh City, Vietnam");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// "Tomorrow" answers tomorrow alone, by its weekday; a weekday its
    /// first such day; today the weather now and today's.
    #[tokio::test]
    async fn the_weather_for_a_day_is_that_days() {
        let (_canned, fetch) = Canned::new();
        let weather = Weather::new(None, fetch, tuesday_noon());
        let asked = |day: &str| json!({"place": "Hanoi", "day": day});
        let tomorrow = weather.answer(&asked("tomorrow")).await;
        assert_eq!(
            tomorrow.told,
            json!({"place": "Hanoi, Vietnam",
                   "weather": "In Hanoi, Vietnam: Tomorrow, Wednesday, 27 to 34°C, thunderstorms, 60% chance of rain."})
        );
        assert_eq!(tomorrow.asked, "for a place named, tomorrow");
        assert_eq!(
            weather.answer(&asked("Saturday")).await.told["weather"],
            "In Hanoi, Vietnam: Saturday 24 to 30°C, rain, 80% chance of rain."
        );
        assert_eq!(
            weather.answer(&asked("today")).await.told["weather"],
            "In Hanoi, Vietnam now: 21°C, partly cloudy, feels like 25°C, humidity 70%. \
             Today, Tuesday, 26 to 33°C, partly cloudy, 40% chance of rain."
        );
        let week = weather.answer(&asked("next week")).await;
        assert!(
            week.told["weather"]
                .as_str()
                .unwrap()
                .ends_with(" Monday 25 to 32°C, clear, 0% chance of rain."),
            "{}",
            week.told
        );
        assert_eq!(Day::asked(Some(" SAT ")), Day::Weekday("Saturday"));
        assert_eq!(Day::asked(Some("thurs")), Day::Weekday("Thursday"));
        assert_eq!(Day::asked(Some("Tonight")), Day::Today);
        assert_eq!(Day::asked(Some("s")), Day::Ahead);
        assert_eq!(Day::asked(None), Day::Ahead);
        let short = json!({"current": {"temperature_2m": 20.0, "weather_code": 3},
                           "daily": {"weather_code": [3], "temperature_2m_max": [24],
                                     "temperature_2m_min": [18]}});
        assert_eq!(said_for(&short, "Hue", Day::Tomorrow, None), None);
        assert_eq!(said_for(&short, "Hue", Day::Weekday("Friday"), None), None);
    }

    /// A place the geocoding doesn't find answers so, never another
    /// place's weather, and nothing more is fetched for it.
    #[tokio::test]
    async fn the_weather_of_an_unknown_place_is_an_error() {
        let (canned, fetch) = Canned::new();
        let weather = Weather::new(Some("Hanoi, Vietnam".into()), fetch, tuesday_noon());
        let answered = weather.answer(&json!({"place": "Atlantis"})).await;
        assert_eq!(
            answered.told,
            json!({"error": "I couldn't find that place"})
        );
        assert_eq!(answered.how, "no such place");
        assert_eq!(canned.count(), 1, "the geocoding alone");
        let long = "x".repeat(101);
        assert_eq!(
            weather.answer(&json!({"place": long})).await.told,
            json!({"error": NOT_FOUND})
        );
        assert_eq!(canned.count(), 1, "too long to look up");
    }

    /// A failed fetch says the weather couldn't be reached: nothing kept
    /// from it, so the next call fetches again.
    #[tokio::test]
    async fn the_weather_not_reached_is_an_error_and_not_kept() {
        let (canned, fetch) = Canned::new();
        let weather = Weather::new(None, fetch, tuesday_noon());
        canned
            .failing
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let answered = weather.answer(&json!({"place": "Hanoi"})).await;
        assert_eq!(answered.told, json!({"error": UNREACHED}));
        assert_eq!(
            answered.how,
            "the weather was not fetched: request timed out"
        );
        canned
            .failing
            .store(false, std::sync::atomic::Ordering::Relaxed);
        let answered = weather.answer(&json!({"place": "Hanoi"})).await;
        assert_eq!(answered.told["place"], "Hanoi, Vietnam");
        assert_eq!(answered.how, "fetched, its place known");
        assert_eq!(
            canned.count(),
            3,
            "where Hanoi is was kept: its forecast alone again"
        );
    }

    /// A fetch that never answers is cut at the call's bound, 6 s, and says
    /// the weather couldn't be reached.
    #[tokio::test(start_paused = true)]
    async fn the_weather_not_reached_in_time_is_an_error() {
        let slow: Fetch = Arc::new(|_, _| Box::pin(std::future::pending()));
        let waited = Instant::now();
        let answered = Weather::new(None, slow, tuesday_noon())
            .answer(&json!({"place": "Hanoi"}))
            .await;
        assert_eq!(answered.told, json!({"error": UNREACHED}));
        assert_eq!(answered.how, "not reached within 6000 ms");
        assert_eq!(waited.elapsed(), WITHIN);
    }

    /// A fetcher whose place is found after `looked_up_in` and whose
    /// forecast connection never opens: each fails as the race does once
    /// its open bound has passed. Each bound asked with is kept, the
    /// place's first.
    fn never_opening_forecast(looked_up_in: Duration) -> (Arc<Mutex<Vec<Duration>>>, Fetch) {
        let bounds = Arc::new(Mutex::new(Vec::new()));
        let kept = bounds.clone();
        let fetch: Fetch = Arc::new(move |url: String, open_within| {
            kept.lock().unwrap().push(open_within);
            Box::pin(async move {
                if url.contains("geocoding-api") {
                    tokio::time::sleep(looked_up_in).await;
                    return canned(&url, false);
                }
                tokio::time::sleep(open_within).await;
                Err(super::super::Error::Timeout("connection"))
            })
        });
        (bounds, fetch)
    }

    /// lamp-9adf on 18 Nov (#7593): a place looked up in 1.2 s, its
    /// forecast's connection unopened in a fixed 2.5 s, and the call failed
    /// in 3.7 s. The place's lookup opens within [`OPEN_WITHIN`] as before;
    /// the forecast within what is left of the 6 s less
    /// [`FORECAST_ANSWERED_IN`], 4 s, and a forecast that never opens ends
    /// there, inside the call's bound. A place known before leaves its
    /// forecast 5.2 s.
    #[tokio::test(start_paused = true)]
    async fn the_forecast_may_open_within_what_is_left_of_the_six_seconds() {
        let (bounds, fetch) = never_opening_forecast(Duration::from_millis(1200));
        let weather = Weather::new(None, fetch, tuesday_noon());
        let waited = Instant::now();
        let answered = weather.answer(&json!({"place": "Hanoi"})).await;
        assert_eq!(answered.told, json!({"error": UNREACHED}));
        assert_eq!(
            answered.how,
            "the weather was not fetched: connection timed out"
        );
        assert_eq!(waited.elapsed(), Duration::from_millis(5200));
        assert!(waited.elapsed() < WITHIN);
        assert_eq!(
            *bounds.lock().unwrap(),
            [OPEN_WITHIN, Duration::from_millis(4000)]
        );
        let waited = Instant::now();
        let answered = weather.answer(&json!({"place": "Hanoi"})).await;
        assert_eq!(
            answered.how,
            "the weather was not fetched: connection timed out"
        );
        assert_eq!(waited.elapsed(), WITHIN - FORECAST_ANSWERED_IN);
        assert_eq!(
            bounds.lock().unwrap()[2],
            WITHIN - FORECAST_ANSWERED_IN,
            "where Hanoi is was kept: its forecast alone, 5.2 s to open"
        );
    }

    /// A place looked up slowly (4 s) leaves its forecast no less than
    /// [`OPEN_WITHIN`] to open, and the call is cut at its 6 s all the same.
    #[tokio::test(start_paused = true)]
    async fn a_slow_place_leaves_its_forecast_the_usual_bound_and_the_call_ends_at_six_seconds() {
        let (bounds, fetch) = never_opening_forecast(Duration::from_secs(4));
        let waited = Instant::now();
        let answered = Weather::new(None, fetch, tuesday_noon())
            .answer(&json!({"place": "Hanoi"}))
            .await;
        assert_eq!(answered.told, json!({"error": UNREACHED}));
        assert_eq!(answered.how, "not reached within 6000 ms");
        assert_eq!(waited.elapsed(), WITHIN);
        assert_eq!(*bounds.lock().unwrap(), [OPEN_WITHIN, OPEN_WITHIN]);
    }

    /// The forecast's open bound: all but [`FORECAST_ANSWERED_IN`] of what
    /// is left, never less than [`OPEN_WITHIN`].
    #[test]
    fn the_forecast_opens_within_what_is_left_less_its_answer() {
        assert_eq!(forecast_open_within(WITHIN), Duration::from_millis(5200));
        assert_eq!(
            forecast_open_within(Duration::from_millis(4800)),
            Duration::from_millis(4000)
        );
        assert_eq!(forecast_open_within(Duration::from_secs(2)), OPEN_WITHIN);
        assert_eq!(forecast_open_within(Duration::ZERO), OPEN_WITHIN);
    }

    /// A follow-up within 10 minutes ("and Saturday?") is answered from what
    /// was kept, with no fetch; after 10 minutes it is fetched again.
    #[tokio::test(start_paused = true)]
    async fn the_weather_of_a_place_is_kept_ten_minutes() {
        let (canned, fetch) = Canned::new();
        let wall = Wall::at(unix("2026-10-20", 12, 0, 25_200));
        let weather = Weather::new(Some("Hanoi, Vietnam".into()), fetch, wall.clock());
        weather.answer(&json!({"place": "Hanoi"})).await;
        assert_eq!(canned.count(), 2);
        wall.pass(9 * 60).await;
        let saturday = weather
            .answer(&json!({"place": "hanoi", "day": "Saturday"}))
            .await;
        assert_eq!(saturday.told["place"], "Hanoi, Vietnam");
        assert_eq!(saturday.how, "from what was kept");
        let here = weather.answer(&json!({"day": "tomorrow"})).await;
        assert_eq!(here.how, "from what was kept", "the lamp's place is Hanoi");
        assert_eq!(canned.count(), 2, "no fetch for either");
        wall.pass(60).await;
        let later = weather.answer(&json!({"place": "Hanoi"})).await;
        assert_eq!(later.how, "fetched, its place known");
        assert_eq!(canned.count(), 3, "its forecast again, not where it is");
    }

    /// Where a place is is kept a week and no longer, and at most 32 names:
    /// the 33rd puts out the oldest (El's P3 on #2996). Read straight from
    /// what is kept, since a canned week's forecast has no today a week on.
    #[tokio::test(start_paused = true)]
    async fn where_a_place_is_is_kept_a_week_and_32_names() {
        let (canned, fetch) = Canned::new();
        let weather = Weather::new(None, fetch, tuesday_noon());
        weather.answer(&json!({"place": "Hanoi"})).await;
        assert_eq!(canned.count(), 2);
        assert!(weather.found_before("hanoi").is_some());
        tokio::time::advance(PLACE_KEPT_FOR - Duration::from_secs(1)).await;
        assert!(
            weather.found_before("hanoi").is_some(),
            "just before the week"
        );
        tokio::time::advance(Duration::from_secs(2)).await;
        assert!(
            weather.found_before("hanoi").is_none(),
            "a week on, asked again"
        );
        let place = || Place {
            name: "Town".into(),
            latitude: 1.0,
            longitude: 2.0,
        };
        weather.remember_place("hanoi".into(), place());
        for n in 1..=MOST_FOUND {
            weather.remember_place(format!("town {n}"), place());
        }
        assert!(
            weather.found_before("hanoi").is_none(),
            "the oldest put out"
        );
        assert!(
            weather
                .found_before(&format!("town {MOST_FOUND}"))
                .is_some()
        );
        assert!(weather.found_before("town 1").is_some());
    }

    /// At most 16 places are kept: a seventeenth puts out the oldest.
    #[tokio::test(start_paused = true)]
    async fn the_weather_keeps_sixteen_places_the_oldest_out() {
        let (canned, fetch) = Canned::new();
        let weather = Weather::new(None, fetch, tuesday_noon());
        for n in 1..=MOST_PLACES + 1 {
            weather.answer(&json!({"place": format!("Town {n}")})).await;
            tokio::time::advance(Duration::from_secs(1)).await;
        }
        assert_eq!(canned.count(), 2 * (MOST_PLACES + 1));
        let second = weather.answer(&json!({"place": "Town 2"})).await;
        assert_eq!(second.how, "from what was kept");
        let first = weather.answer(&json!({"place": "Town 1"})).await;
        assert_eq!(
            first.how, "fetched, its place known",
            "the oldest forecast was put out, but where it is is still kept"
        );
        assert_eq!(weather.fresh(tuesday_noon()()).len(), MOST_PLACES);
    }

    /// The function the model is told: its name, both words optional, and
    /// a description that asks for every weather question and the place
    /// said.
    #[test]
    fn the_weather_function_is_declared_with_two_optional_words() {
        let tool = declaration();
        assert_eq!(tool.name, NAME);
        let parameters = tool.parameters.unwrap();
        assert!(parameters.get("required").is_none(), "{parameters}");
        assert_eq!(parameters["properties"]["place"]["type"], "string");
        assert_eq!(parameters["properties"]["day"]["enum"][1], "tomorrow");
        assert!(
            tool.description
                .contains("every question about the weather")
        );
        assert!(
            tool.description
                .contains("say which place you are answering for")
        );
        assert!(tool.description.len() <= 1_024);
        let hanoi = found(&geocoded("Hanoi", "Vietnam", 21.0245, 105.84117)).unwrap();
        assert_eq!(hanoi.name, "Hanoi, Vietnam");
        let bare = json!({"results": [{"name": "Singapore", "country": "Singapore",
                                       "latitude": 1.29, "longitude": 103.85}]});
        assert_eq!(found(&bare).unwrap().name, "Singapore");
    }

    /// Honolulu's week from Thursday 22 Oct 2026, ten hours behind UTC, each
    /// day told apart by its numbers and its sky.
    fn honolulu() -> Value {
        json!({
            "utc_offset_seconds": -36_000, "timezone": "Pacific/Honolulu",
            "current": {"temperature_2m": 24.0, "apparent_temperature": 26.0,
                        "relative_humidity_2m": 75, "weather_code": 1},
            "daily": {
                "time": ["2026-10-22", "2026-10-23", "2026-10-24", "2026-10-25",
                         "2026-10-26", "2026-10-27", "2026-10-28"],
                "weather_code": [0, 1, 2, 3, 61, 63, 80],
                "temperature_2m_max": [22, 23, 24, 25, 26, 27, 28],
                "temperature_2m_min": [18, 19, 20, 21, 22, 23, 24],
                "precipitation_probability_max": [0, 10, 20, 30, 40, 50, 60]
            }
        })
    }

    /// A fetcher that finds every name as Honolulu and answers its week
    /// as [`honolulu`] has it, whenever it is asked, counting each fetch.
    fn honolulu_fetch() -> (Arc<std::sync::atomic::AtomicUsize>, Fetch) {
        let fetched = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = fetched.clone();
        let fetch: Fetch = Arc::new(move |url: String, _| {
            count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Box::pin(async move {
                Ok(if url.contains("geocoding-api") {
                    geocoded("Honolulu", "United States", 21.30694, -157.85833)
                } else {
                    honolulu()
                })
            })
        });
        (fetched, fetch)
    }

    /// The reviewers on #2969: a forecast kept from 23:59 at the place said
    /// "tomorrow" for the wrong day after the place's midnight, and "today"
    /// for the day before. Honolulu at 23:59 on Thursday is Friday in UTC
    /// and 16:59 on Friday in Hanoi, the lamp's place: its days are told
    /// from its own date, and two minutes later, past its midnight, it is
    /// fetched again and Friday is today, though its rows still begin on
    /// Thursday.
    #[tokio::test(start_paused = true)]
    async fn the_weather_after_the_places_midnight_is_for_its_new_day() {
        use std::sync::atomic::Ordering;
        let (fetched, fetch) = honolulu_fetch();
        let wall = Wall::at(unix("2026-10-22", 23, 59, -36_000));
        let weather = Weather::new(Some("Hanoi, Vietnam".into()), fetch, wall.clock());
        let asked = |day: &str| json!({"place": "Honolulu", "day": day});
        let said = |answered: Answered| answered.told["weather"].as_str().unwrap().to_owned();
        assert_eq!(
            said(weather.answer(&asked("tomorrow")).await),
            "In Honolulu, United States: Tomorrow, Friday, 19 to 23°C, mainly clear, 10% chance of rain."
        );
        let today = said(weather.answer(&asked("today")).await);
        assert!(
            today.ends_with(" Today, Thursday, 18 to 22°C, clear, 0% chance of rain."),
            "{today}"
        );
        assert_eq!(fetched.load(Ordering::Relaxed), 2);
        wall.pass(2 * 60).await;
        let tomorrow = weather.answer(&asked("tomorrow")).await;
        assert_eq!(
            tomorrow.how, "fetched, its place known",
            "the place's midnight passed: its forecast again, not where it is"
        );
        assert_eq!(
            said(tomorrow),
            "In Honolulu, United States: Tomorrow, Saturday, 20 to 24°C, partly cloudy, 20% chance of rain."
        );
        assert_eq!(fetched.load(Ordering::Relaxed), 3);
        let today = weather.answer(&asked("today")).await;
        assert_eq!(today.how, "from what was kept");
        let today = said(today);
        assert!(
            today.starts_with("In Honolulu, United States now: 24°C, mainly clear")
                && today.ends_with(" Today, Friday, 19 to 23°C, mainly clear, 10% chance of rain."),
            "{today}"
        );
        let ahead = said(weather.answer(&json!({"place": "Honolulu"})).await);
        assert!(ahead.contains(" Today, Friday,"), "{ahead}");
        assert!(ahead.contains(" Tomorrow, Saturday,"), "{ahead}");
        assert!(
            !ahead.contains("Thursday"),
            "yesterday is never said: {ahead}"
        );
        // A weekday is the next one from today on: Saturday tomorrow, and
        // no Thursday, which is yesterday's row and next week's past the
        // forecast.
        assert_eq!(
            said(weather.answer(&asked("Saturday")).await),
            "In Honolulu, United States: Tomorrow, Saturday, 20 to 24°C, partly cloudy, 20% chance of rain."
        );
        assert_eq!(
            weather.answer(&asked("Thursday")).await.told,
            json!({"place": "Honolulu, United States", "error": NO_SUCH_DAY})
        );
        assert_eq!(
            fetched.load(Ordering::Relaxed),
            3,
            "the new day's forecast kept"
        );
    }

    /// A place's day is its own: 10:30 UTC on 22 Oct is the 23rd 14 hours
    /// ahead and still the 22nd 10 hours behind. A forecast whose rows lack
    /// the place's today is no answer, and is not kept, so the next call
    /// fetches again.
    #[tokio::test]
    async fn the_weather_is_told_from_the_places_own_date() {
        let utc = unix("2026-10-22", 10, 30, 0);
        let ahead = json!({"utc_offset_seconds": 50_400});
        assert_eq!(place_day(&ahead, utc), day_number("2026-10-23"));
        assert_eq!(place_day(&honolulu(), utc), day_number("2026-10-22"));
        assert_eq!(place_day(&json!({}), utc), None);
        let week = honolulu();
        let friday = day_number("2026-10-23");
        assert!(
            said_for(&week, "Honolulu", Day::Today, friday)
                .unwrap()
                .ends_with(" Today, Friday, 19 to 23°C, mainly clear, 10% chance of rain.")
        );
        let late = day_number("2026-11-05");
        assert_eq!(said_for(&week, "Honolulu", Day::Ahead, late), None);
        let (fetched, fetch) = honolulu_fetch();
        let weather = Weather::new(None, fetch, Arc::new(move || unix("2026-11-05", 9, 0, 0)));
        let answered = weather.answer(&json!({"place": "Honolulu"})).await;
        assert_eq!(answered.told, json!({"error": UNREACHED}));
        assert_eq!(
            answered.how,
            "the forecast came with nothing for today there"
        );
        weather.answer(&json!({"place": "Honolulu"})).await;
        assert_eq!(
            fetched.load(std::sync::atomic::Ordering::Relaxed),
            3,
            "the second call asks its forecast again, not where it is"
        );
    }

    /// One place found, as the geocoding gives it, with its country's code
    /// and how many live there when known.
    fn geocoded_in(name: &str, country: (&str, &str), people: Option<u64>, at: f64) -> Value {
        let mut found = json!({"id": 1, "name": name, "latitude": at, "longitude": at,
                               "country": country.0, "country_code": country.1});
        if let Some(people) = people {
            found["population"] = json!(people);
        }
        json!({"results": [found]})
    }

    /// A geocoding as Open-Meteo's answered on 18 Nov: "Sapa" anywhere is
    /// Sapa, Romania, in Vietnam Sa Pa; "Paris" anywhere is Paris, France,
    /// in the US Paris, Texas, and in Vietnam nothing. Each URL is kept.
    fn geocoding_by_country() -> (Arc<Mutex<Vec<String>>>, Fetch) {
        let asked = Arc::new(Mutex::new(Vec::new()));
        let urls = asked.clone();
        let fetch: Fetch = Arc::new(move |url: String, _| {
            urls.lock().unwrap().push(url.clone());
            Box::pin(async move {
                let parsed = reqwest::Url::parse(&url).unwrap();
                let param = |name: &str| {
                    parsed
                        .query_pairs()
                        .find(|(key, _)| key == name)
                        .map(|(_, value)| value.into_owned())
                };
                if parsed.host_str() != Some("geocoding-api.open-meteo.com") {
                    return Ok(forecast(25.0));
                }
                let name = param("name").unwrap_or_default();
                Ok(match (name.as_str(), param("countryCode").as_deref()) {
                    ("Sapa", None) => geocoded_in("Sapa", ("Romania", "RO"), None, 46.0),
                    ("Sapa", Some("VN")) => geocoded_in("Sapa", ("Vietnam", "VN"), None, 22.3),
                    ("Paris", None) => {
                        geocoded_in("Paris", ("France", "FR"), Some(2_138_551), 48.8)
                    }
                    ("Paris", Some("US")) => {
                        geocoded_in("Paris", ("United States", "US"), Some(24_782), 33.6)
                    }
                    _ => json!({"generationtime_ms": 0.5}),
                })
            })
        });
        (asked, fetch)
    }

    /// lamp-9adf on 18 Nov (#7593): "Sa Pa" asked of a lamp in Vietnam was
    /// answered for Sapa, Romania. With the lamp's country known the name
    /// is looked up anywhere and there at once, and the place there is
    /// chosen; a place with no match there is the one found anywhere, and
    /// a far larger city anywhere goes over a small one there.
    #[tokio::test]
    async fn a_place_in_the_lamps_country_is_chosen_over_a_foreign_one() {
        let (asked, fetch) = geocoding_by_country();
        let in_vietnam =
            Weather::new(None, fetch.clone(), tuesday_noon()).with_country(Some("VN".into()));
        let answered = in_vietnam.answer(&json!({"place": "Sapa"})).await;
        assert_eq!(answered.told["place"], "Sapa, Vietnam", "{}", answered.told);
        {
            let urls = asked.lock().unwrap();
            assert_eq!(urls.len(), 3, "{urls:?}");
            assert_eq!(
                urls.iter()
                    .filter(|url| url.contains("countryCode=VN"))
                    .count(),
                1,
                "{urls:?}"
            );
        }
        let answered = in_vietnam.answer(&json!({"place": "Paris"})).await;
        assert_eq!(answered.told["place"], "Paris, France", "{}", answered.told);
        let in_the_us =
            Weather::new(None, fetch.clone(), tuesday_noon()).with_country(Some("US".into()));
        let answered = in_the_us.answer(&json!({"place": "Paris"})).await;
        assert_eq!(answered.told["place"], "Paris, France", "{}", answered.told);
        // With no country known, one lookup, anywhere, as before.
        asked.lock().unwrap().clear();
        let nowhere =
            Weather::new(None, fetch.clone(), tuesday_noon()).with_country(Some("vn".into()));
        let answered = nowhere.answer(&json!({"place": "Sapa"})).await;
        assert_eq!(answered.told["place"], "Sapa, Romania", "{}", answered.told);
        assert_eq!(asked.lock().unwrap().len(), 2);
    }

    /// The place chosen of two answers: the one in the lamp's country,
    /// unless none, or the one anywhere is a city of 100,000 people or
    /// more and ten times as many, both counts known.
    #[test]
    fn the_place_in_the_lamps_country_goes_first_unless_far_smaller() {
        let named =
            |anywhere: &Value, at_home: &Value| chosen(anywhere, at_home).map(|place| place.name);
        let sapa_ro = geocoded_in("Sapa", ("Romania", "RO"), None, 46.0);
        let sapa_vn = geocoded_in("Sa Pa", ("Vietnam", "VN"), Some(10_554), 22.3);
        let none = json!({"generationtime_ms": 0.5});
        assert_eq!(named(&sapa_ro, &sapa_vn).as_deref(), Some("Sa Pa, Vietnam"));
        assert_eq!(named(&sapa_ro, &none).as_deref(), Some("Sapa, Romania"));
        assert_eq!(named(&none, &none), None);
        let town = geocoded_in("Sapa", ("Philippines", "PH"), Some(99_999), 13.0);
        assert_eq!(named(&town, &sapa_vn).as_deref(), Some("Sa Pa, Vietnam"));
        let city = geocoded_in("Sapa", ("Philippines", "PH"), Some(105_540), 13.0);
        assert_eq!(named(&city, &sapa_vn).as_deref(), Some("Sapa, Philippines"));
        let near_city = geocoded_in("Sa Pa", ("Vietnam", "VN"), Some(10_555), 22.3);
        assert_eq!(named(&city, &near_city).as_deref(), Some("Sa Pa, Vietnam"));
    }

    /// El on #7700: a place at home the geocoding gives no count for is
    /// unknown, not empty, and is chosen over a foreign namesake of any
    /// size, as a home place with a count of 0 is not.
    #[test]
    fn a_place_at_home_with_no_count_is_chosen_over_a_foreign_city() {
        let named =
            |anywhere: &Value, at_home: &Value| chosen(anywhere, at_home).map(|place| place.name);
        let home = geocoded_in("Sapa", ("Vietnam", "VN"), None, 22.3);
        let city = geocoded_in("Sapa", ("Philippines", "PH"), Some(2_000_000), 13.0);
        assert_eq!(named(&city, &home).as_deref(), Some("Sapa, Vietnam"));
        let empty = geocoded_in("Sapa", ("Vietnam", "VN"), Some(0), 22.3);
        assert_eq!(named(&city, &empty).as_deref(), Some("Sapa, Philippines"));
        let unknown_far = geocoded_in("Sapa", ("Romania", "RO"), None, 46.0);
        assert_eq!(
            named(&unknown_far, &empty).as_deref(),
            Some("Sapa, Vietnam")
        );
    }

    /// The whole call on lamp-9adf on 18 Nov (#7593): a place looked up in
    /// 1.2 s, then a forecast host whose one address never opens. Looked
    /// up again another way, its forecast comes within the 6 s; with no
    /// other way the call still ends within them, the weather unreached.
    #[tokio::test]
    async fn a_forecast_whose_only_address_never_opens_is_reached_another_way_within_six_seconds() {
        let fetch_by = |elsewhere: bool| -> Fetch {
            Arc::new(move |url: String, open_within| {
                Box::pin(async move {
                    if url.contains("geocoding-api") {
                        tokio::time::sleep(Duration::from_millis(1200)).await;
                        return canned(&url, false);
                    }
                    super::super::public::tests::one_address_dead(
                        forecast(21.0).to_string(),
                        open_within,
                        elsewhere,
                    )
                    .await
                })
            })
        };
        let reached = Weather::new(None, fetch_by(true), tuesday_noon());
        let unreached = Weather::new(None, fetch_by(false), tuesday_noon());
        let asked = Instant::now();
        let ((reached, reached_in), (unreached, unreached_in)) = tokio::join!(
            async {
                let answered = reached.answer(&json!({"place": "Hanoi"})).await;
                (answered, asked.elapsed())
            },
            async {
                let answered = unreached.answer(&json!({"place": "Hanoi"})).await;
                (answered, asked.elapsed())
            },
        );
        assert_eq!(reached.told["place"], "Hanoi, Vietnam", "{}", reached.told);
        assert_eq!(reached.how, "fetched");
        assert!(reached_in < WITHIN, "{reached_in:?}");
        assert_eq!(unreached.told, json!({"error": UNREACHED}));
        assert_eq!(
            unreached.how,
            "the weather was not fetched: connection timed out"
        );
        assert!(unreached_in <= WITHIN, "{unreached_in:?}");
    }
}
