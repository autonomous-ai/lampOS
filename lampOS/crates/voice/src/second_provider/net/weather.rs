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
//! place's name and its coordinates leave the lamp.
use super::{Result, get_public_json};
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

/// How a request's JSON answer is fetched: from Open-Meteo on the lamp
/// ([`open_meteo`]), and a canned one in a test.
pub type Fetch = Arc<dyn Fn(String) -> BoxFuture<'static, Result<Value>> + Send + Sync>;

/// Open-Meteo's answers, fetched over HTTPS with no key.
pub fn open_meteo() -> Fetch {
    Arc::new(|url| Box::pin(async move { get_public_json(&url).await }))
}

/// The name a place is looked up by: its first part ("Hanoi" for "Hanoi,
/// Vietnam").
pub fn place_query(place: &str) -> Option<String> {
    let name = place.split(',').next()?.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

/// The geocoding request for `name`.
pub fn geocoding_url(name: &str) -> Option<String> {
    reqwest::Url::parse_with_params(
        GEOCODING,
        [
            ("name", name),
            ("count", "1"),
            ("language", "en"),
            ("format", "json"),
        ],
    )
    .ok()
    .map(String::from)
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
    }
    said.push('.');
    Some(said)
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
    fetch: Fetch,
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
            fetch,
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
        let (told, how) = match tokio::time::timeout(WITHIN, self.looked_up(place, day, now)).await
        {
            Ok(answered) => answered,
            Err(_) => (
                json!({"error": UNREACHED}),
                format!("not reached within {} ms", WITHIN.as_millis()),
            ),
        };
        Answered { told, asked, how }
    }

    /// The call's answer, its days told from the place's own day at `now`.
    async fn looked_up(&self, place: Option<&str>, day: Day, now: i64) -> (Value, String) {
        if place.is_none()
            && let Some(owner) = self.owner_here()
        {
            let key = owner.name.to_lowercase();
            return self.forecast_of(key, owner, true, day, now).await;
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
                let Some(url) = geocoding_url(&query) else {
                    return (json!({"error": NOT_FOUND}), "no such place".into());
                };
                let place = match (self.fetch)(url).await {
                    Ok(answer) => found(&answer),
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
        self.forecast_of(key, place, known.is_some(), day, now)
            .await
    }

    /// The forecast of `place`, found by the name `key`, from what was kept
    /// or fetched; `known`, its place was known before this call.
    async fn forecast_of(
        &self,
        key: String,
        place: Place,
        known: bool,
        day: Day,
        now: i64,
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
        let forecast = match (self.fetch)(url).await {
            Ok(forecast) => Arc::new(forecast),
            Err(error) => {
                return (
                    json!({"error": UNREACHED}),
                    format!("the weather was not fetched: {error}"),
                );
            }
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
        let how = if known {
            "fetched, its place known"
        } else {
            "fetched"
        };
        (answered, how.into())
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
        let url = geocoding_url("Ho Chi Minh City").unwrap();
        assert!(
            url.starts_with(
                "https://geocoding-api.open-meteo.com/v1/search?name=Ho+Chi+Minh+City&count=1"
            ),
            "{url}"
        );
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
            let fetch: Fetch = Arc::new(move |url: String| {
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
        let slow: Fetch = Arc::new(|_| Box::pin(std::future::pending()));
        let waited = Instant::now();
        let answered = Weather::new(None, slow, tuesday_noon())
            .answer(&json!({"place": "Hanoi"}))
            .await;
        assert_eq!(answered.told, json!({"error": UNREACHED}));
        assert_eq!(answered.how, "not reached within 6000 ms");
        assert_eq!(waited.elapsed(), WITHIN);
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
        let fetch: Fetch = Arc::new(move |url: String| {
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
}
