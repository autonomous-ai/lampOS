//! The forecast's backup: MET Norway's Locationforecast 2.0 (`api.met.no`),
//! asked only when Open-Meteo's forecast can't be reached, its connection
//! unopened (after its DNS-over-HTTPS lookup too, while `LAMP_WEATHER_DOH`
//! is on) or timed out, or it answers only with a server error (5xx,
//! Thirteen's P3 on #7732) ([`backs_up`]); never for another answer
//! Open-Meteo gave, a 4xx or JSON not read. The migrate on #7665 (#7593): fetched within what is left of the call's
//! [`super::WITHIN`] and said the same way, before "the weather couldn't be
//! reached". Only the place's coordinates go, to 4 decimals, with no key and
//! the lamp's name as its `User-Agent` ([`USER_AGENT`], as MET's terms ask);
//! never the place's name, which Open-Meteo's geocoding alone looks up. Its
//! answer is put in Open-Meteo's own shape ([`as_open_meteo`]), its days the
//! lamp's own, so the forecast is said, kept and told by day as Open-Meteo's.
use super::super::{Error, OPENING, public::get_public_json_as};
use super::{Fetch, day_number};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

/// The switch, in the voice service's environment: off unless exactly
/// `on` (El's ruling on #7700, 18 Nov: a new weather lookup is off in the
/// product), in no product unit.
pub const ENVIRONMENT: &str = "LAMP_WEATHER_BACKUP";
/// How the lamp names itself to MET Norway, as its terms ask of every
/// client: the program, its version and where it is from.
pub const USER_AGENT: &str = concat!(
    "lampOS/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/autonomous-ai/lampOS)"
);
const LOCATIONFORECAST: &str = "https://api.met.no/weatherapi/locationforecast/2.0/compact";
/// What Open-Meteo's forecast leaves of [`super::WITHIN`] for the backup
/// while there is one: a connection's tries begun within 1.5 s of its
/// bound ([`super::super::public::TRY_OPENS_IN`], from TLS ending 0.65 to
/// 1.37 s after a curl began on lamp-9adf, 18 Nov) and its answer
/// ([`super::FORECAST_ANSWERED_IN`]). An estimate, not timed on a lamp.
pub const LEFT_FOR_IT: Duration = Duration::from_millis(2300);
/// The days of the forecast said, today's first, as Open-Meteo's 7.
const DAYS: i64 = 7;

/// Whether the switch is on: `on` exactly; unset or anything else, off.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// The lamp's offset east of UTC, in seconds, at a time (seconds since
/// 1970 UTC): the lamp's own zone on the lamp, a set one in a test.
pub type Offset = Arc<dyn Fn(i64) -> i64 + Send + Sync>;

/// The lamp's own zone (`crate::local_time::system`), UTC while it can't be
/// read.
pub fn lamp_offset() -> Offset {
    Arc::new(|now| {
        let at = u64::try_from(now)
            .ok()
            .and_then(|now| now.checked_mul(1_000));
        crate::local_time::system()
            .zip(at)
            .map_or(0, |(zone, at)| i64::from(zone.offset_minutes_at(at)) * 60)
    })
}

/// MET Norway's answers, fetched over HTTPS as Open-Meteo's are
/// (`net::public`), each try naming the lamp ([`USER_AGENT`]).
pub fn met_norway() -> Fetch {
    Arc::new(|url, open_within| {
        Box::pin(async move { get_public_json_as(&url, open_within, USER_AGENT).await })
    })
}

/// The forecast request for a place's coordinates, to 4 decimals.
pub fn url(latitude: f64, longitude: f64) -> Option<String> {
    reqwest::Url::parse_with_params(
        LOCATIONFORECAST,
        [
            ("lat", format!("{latitude:.4}")),
            ("lon", format!("{longitude:.4}")),
        ],
    )
    .ok()
    .map(String::from)
}

/// Whether Open-Meteo's forecast failed so the backup is asked: for want
/// of reaching it (no connection opened, its name not found, or a step
/// timed out), or with a server error, 500 to 599, when the service is
/// reached but can't give a forecast (Thirteen's P3 on #7732). A 4xx, 429
/// among them, and JSON not read are answers of its own, and the backup
/// isn't asked.
pub fn backs_up(error: &Error) -> bool {
    matches!(
        error,
        Error::Timeout(_) | Error::Transport(OPENING | "address") | Error::Status(500..=599)
    )
}

/// A MET symbol (`partlycloudy_day`, its `_day`, `_night` or
/// `_polartwilight` aside) as the WMO code whose words Open-Meteo's forecast
/// is said in (`super::sky`), so either service says a sky in the same
/// words. Any thunder is thunderstorms; sleet, which no WMO code Open-Meteo
/// gives names, is snow as the wintry one of the two; a symbol not known is
/// -1, mixed weather, and the least severe of a day's.
pub fn code(symbol: &str) -> i64 {
    let name = symbol.split('_').next().unwrap_or(symbol);
    if name.contains("thunder") {
        return 95;
    }
    match name {
        "clearsky" => 0,
        "fair" => 1,
        "partlycloudy" => 2,
        "cloudy" => 3,
        "fog" => 45,
        "lightrain" => 61,
        "rain" => 63,
        "heavyrain" => 65,
        "lightsleet" | "lightsleetshowers" | "lightsnow" | "lightsnowshowers" => 71,
        "sleet" | "sleetshowers" | "snow" | "snowshowers" => 73,
        "heavysleet" | "heavysleetshowers" | "heavysnow" | "heavysnowshowers" => 75,
        "lightrainshowers" => 80,
        "rainshowers" => 81,
        "heavyrainshowers" => 82,
        _ => -1,
    }
}

/// The periods ahead of a point MET gives, the shortest first.
const WINDOWS: [&str; 3] = ["next_1_hours", "next_6_hours", "next_12_hours"];

/// Seconds since 1970 (UTC) of MET's `2026-10-20T05:00:00Z`.
fn unix_of(time: &str) -> Option<i64> {
    let (date, clock) = time.strip_suffix('Z')?.split_once('T')?;
    let mut parts = clock.splitn(3, ':').map(|part| part.parse::<i64>().ok());
    let (hour, minute) = (parts.next()??, parts.next()??);
    let second = match parts.next() {
        Some(second) => second?,
        None => 0,
    };
    Some(day_number(date)? * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// A point's sky: its shortest period's symbol as a WMO [`code`].
fn sky_of(data: &Value) -> Option<i64> {
    WINDOWS
        .iter()
        .find_map(|window| data[*window]["summary"]["symbol_code"].as_str())
        .map(code)
}

/// The rain a point gives until the next, in mm: its next hour's while the
/// next point is under 6 hours on (MET's hourly points), else its next 6
/// hours' (the six-hourly ones, and the last), else its next hour's.
fn rain_until_next(data: &Value, gap: Option<i64>) -> Option<f64> {
    let amount = |window: &str| data[window]["details"]["precipitation_amount"].as_f64();
    match gap {
        Some(gap) if gap < 6 * 3_600 => amount("next_1_hours"),
        _ => amount("next_6_hours").or_else(|| amount("next_1_hours")),
    }
}

/// MET's answer in Open-Meteo's shape, as `super::forecast_url` asks it,
/// at `now` (seconds since 1970 UTC) with the lamp `offset` seconds east
/// of UTC: `utc_offset_seconds` the lamp's, so its days are the lamp's own
/// as `super::place_day` tells them; `current` from the last point at or
/// before now (else the first): its temperature, its humidity and its
/// next hour's sky; and from today, at most [`DAYS`] days of `daily`, each
/// of the points on that day: its low and high (the points' and their next
/// 6 hours' own), its most severe sky (the highest code, as Open-Meteo's
/// daily `weather_code`), its highest chance of rain where MET gives one
/// (`probability_of_precipitation`, the `complete` answer's) and its rain
/// in mm. A field MET leaves out is left out (`null`), and what is said
/// leaves it out too. `None` for an answer with no points.
pub fn as_open_meteo(answer: &Value, offset: i64, now: i64) -> Option<Value> {
    let points: Vec<(i64, &Value)> = answer["properties"]["timeseries"]
        .as_array()?
        .iter()
        .filter_map(|point| Some((unix_of(point["time"].as_str()?)?, &point["data"])))
        .collect();
    let &(_, data) = points
        .iter()
        .rev()
        .find(|(at, _)| *at <= now)
        .or_else(|| points.first())?;
    let instant = &data["instant"]["details"];
    let mut current = json!({});
    if let Some(temperature) = instant["air_temperature"].as_f64() {
        current["temperature_2m"] = json!(temperature);
    }
    if let Some(code) = sky_of(data) {
        current["weather_code"] = json!(code);
    }
    if let Some(humidity) = instant["relative_humidity"].as_f64() {
        current["relative_humidity_2m"] = json!(humidity.round() as i64);
    }
    let day_of = |at: i64| (at + offset).div_euclid(86_400);
    let today = day_of(now);
    let mut daily: [Vec<Value>; 6] = Default::default();
    for day in today..today + DAYS {
        let rows: Vec<usize> = (0..points.len())
            .filter(|&at| day_of(points[at].0) == day)
            .collect();
        if rows.is_empty() {
            break;
        }
        let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
        let (mut sky, mut chance, mut rain) = (None::<i64>, None::<f64>, None::<f64>);
        for &row in &rows {
            let data = points[row].1;
            let six = &data["next_6_hours"]["details"];
            for degrees in [
                &data["instant"]["details"]["air_temperature"],
                &six["air_temperature_min"],
                &six["air_temperature_max"],
            ]
            .into_iter()
            .filter_map(Value::as_f64)
            {
                low = low.min(degrees);
                high = high.max(degrees);
            }
            if let Some(code) = sky_of(data) {
                sky = Some(sky.map_or(code, |sky| sky.max(code)));
            }
            for percent in WINDOWS.iter().filter_map(|window| {
                data[*window]["details"]["probability_of_precipitation"].as_f64()
            }) {
                chance = Some(chance.map_or(percent, |chance| chance.max(percent)));
            }
            let gap = points.get(row + 1).map(|(next, _)| next - points[row].0);
            if let Some(amount) = rain_until_next(data, gap) {
                rain = Some(rain.unwrap_or(0.0) + amount);
            }
        }
        let (year, month, date) = crate::local_time::civil(day);
        let degrees = |value: f64| value.is_finite().then_some(value);
        for (column, value) in daily.iter_mut().zip([
            json!(format!("{year:04}-{month:02}-{date:02}")),
            json!(sky),
            json!(degrees(high)),
            json!(degrees(low)),
            json!(chance.map(|chance| chance.round() as i64)),
            json!(rain),
        ]) {
            column.push(value);
        }
    }
    let [time, weather_code, high, low, chance, rain] = daily;
    Some(json!({
        "utc_offset_seconds": offset,
        "current": current,
        "daily": {
            "time": time,
            "weather_code": weather_code,
            "temperature_2m_max": high,
            "temperature_2m_min": low,
            "precipitation_probability_max": chance,
            "precipitation_sum": rain
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::super::{Day, Weather, said_for, sky};
    use super::*;
    use std::sync::Mutex;
    use tokio::time::Instant;

    /// Seven hours east of UTC, as Hanoi.
    const OFFSET: i64 = 25_200;

    /// Seconds since 1970 (UTC) at `hour` UTC on `date`.
    fn utc(date: &str, hour: i64) -> i64 {
        day_number(date).unwrap() * 86_400 + hour * 3_600
    }

    /// Noon on Tuesday 20 Oct 2026 in Hanoi: 05:00 UTC.
    fn noon() -> i64 {
        utc("2026-10-20", 5)
    }

    /// One of MET's periods ahead of a point: its symbol and details.
    fn period(symbol: &str, details: Value) -> Value {
        json!({"summary": {"symbol_code": symbol}, "details": details})
    }

    /// MET's compact answer for Hanoi from noon on Tuesday 20 Oct 2026, as
    /// it gives one: hourly, then six-hourly. Today (Hanoi's Tuesday) 26 to
    /// 33°C with light showers and 2 mm of rain; tomorrow 25 to 31°C with
    /// thunder and 5.6 mm. With `chances`, the probabilities of rain the
    /// `complete` answer adds: 40% and 60%.
    fn met_hanoi(chances: bool) -> Value {
        let chance = |percent: f64| {
            if chances { json!(percent) } else { Value::Null }
        };
        let point = |time: &str, instant: Value, data: Value| {
            let mut data = data;
            data["instant"] = json!({"details": instant});
            json!({"time": time, "data": data})
        };
        json!({
            "type": "Feature",
            "geometry": {"type": "Point", "coordinates": [105.8412, 21.0245, 15]},
            "properties": {
                "meta": {"updated_at": "2026-10-20T04:31:12Z",
                         "units": {"air_temperature": "celsius", "precipitation_amount": "mm"}},
                "timeseries": [
                    point("2026-10-20T05:00:00Z",
                          json!({"air_temperature": 31.2, "relative_humidity": 70.4}),
                          json!({"next_1_hours": period("partlycloudy_day", json!({"precipitation_amount": 0.0})),
                                 "next_6_hours": period("partlycloudy_day", json!({
                                     "air_temperature_max": 33.1, "air_temperature_min": 30.5,
                                     "precipitation_amount": 0.4}))})),
                    point("2026-10-20T06:00:00Z",
                          json!({"air_temperature": 33.0, "relative_humidity": 64.0}),
                          json!({"next_1_hours": period("lightrainshowers_day", json!({"precipitation_amount": 0.4})),
                                 "next_6_hours": period("rainshowers_day", json!({
                                     "air_temperature_max": 33.4, "air_temperature_min": 27.0,
                                     "precipitation_amount": 2.0,
                                     "probability_of_precipitation": chance(40.0)}))})),
                    point("2026-10-20T12:00:00Z",
                          json!({"air_temperature": 28.0}),
                          json!({"next_6_hours": period("cloudy", json!({
                                     "air_temperature_max": 28.0, "air_temperature_min": 26.0,
                                     "precipitation_amount": 0.0}))})),
                    point("2026-10-20T18:00:00Z",
                          json!({"air_temperature": 25.6}),
                          json!({"next_6_hours": period("clearsky_night", json!({
                                     "air_temperature_max": 26.0, "air_temperature_min": 25.0,
                                     "precipitation_amount": 0.0}))})),
                    point("2026-10-21T00:00:00Z",
                          json!({"air_temperature": 25.1}),
                          json!({"next_6_hours": period("fair_day", json!({"precipitation_amount": 0.0}))})),
                    point("2026-10-21T06:00:00Z",
                          json!({"air_temperature": 30.2}),
                          json!({"next_6_hours": period("rainandthunder", json!({
                                     "air_temperature_max": 31.4, "air_temperature_min": 25.1,
                                     "precipitation_amount": 5.6,
                                     "probability_of_precipitation": chance(60.0)}))})),
                    point("2026-10-21T12:00:00Z",
                          json!({"air_temperature": 27.0}),
                          json!({"next_6_hours": period("cloudy", json!({"precipitation_amount": 0.0}))}))
                ]
            }
        })
    }

    /// Open-Meteo's answer for the same weather in Hanoi: no feeling, as
    /// MET gives none.
    fn open_meteo_hanoi() -> Value {
        json!({
            "utc_offset_seconds": OFFSET, "timezone": "Asia/Bangkok",
            "current": {"temperature_2m": 31.2, "relative_humidity_2m": 70, "weather_code": 2},
            "daily": {
                "time": ["2026-10-20", "2026-10-21"],
                "weather_code": [80, 95],
                "temperature_2m_max": [33.4, 31.4],
                "temperature_2m_min": [26.0, 25.0],
                "precipitation_probability_max": [40, 60]
            }
        })
    }

    fn today() -> Option<i64> {
        day_number("2026-10-20")
    }

    /// MET's answer is said now, today and tomorrow as Open-Meteo's is:
    /// the hour's temperature, sky and humidity; each day's low and high,
    /// its most severe sky and its rain, from the lamp's own days.
    #[test]
    fn mets_answer_is_said_now_today_and_tomorrow() {
        let forecast = as_open_meteo(&met_hanoi(false), OFFSET, noon()).expect("read");
        assert_eq!(forecast["utc_offset_seconds"], OFFSET);
        assert_eq!(
            said_for(&forecast, "Hanoi", Day::Ahead, today()).as_deref(),
            Some(
                "In Hanoi now: 31°C, partly cloudy, humidity 70%. \
                 Today, Tuesday, 26 to 33°C, light showers, 2 mm of rain. \
                 Tomorrow, Wednesday, 25 to 31°C, thunderstorms, 5.6 mm of rain."
            )
        );
        assert_eq!(
            said_for(&forecast, "Hanoi", Day::Tomorrow, today()).as_deref(),
            Some("In Hanoi: Tomorrow, Wednesday, 25 to 31°C, thunderstorms, 5.6 mm of rain.")
        );
        // Half an hour on, the hour's point is still the weather now.
        let later = as_open_meteo(&met_hanoi(false), OFFSET, noon() + 1_800).unwrap();
        assert_eq!(later["current"], forecast["current"]);
    }

    /// A field MET leaves out is left out of what is said: no humidity, no
    /// humidity said; no temperature now, nothing now but the days still;
    /// no symbol in a day, that day not said; no timeseries, no answer.
    #[test]
    fn a_field_met_leaves_out_is_left_out() {
        let mut answer = met_hanoi(false);
        let points = &mut answer["properties"]["timeseries"];
        points[0]["data"]["instant"]["details"]
            .as_object_mut()
            .unwrap()
            .remove("relative_humidity");
        let forecast = as_open_meteo(&answer, OFFSET, noon()).unwrap();
        let said = said_for(&forecast, "Hanoi", Day::Today, today()).unwrap();
        assert!(
            said.starts_with("In Hanoi now: 31°C, partly cloudy. Today"),
            "{said}"
        );
        let mut answer = met_hanoi(false);
        answer["properties"]["timeseries"][0]["data"]["instant"]["details"]
            .as_object_mut()
            .unwrap()
            .remove("air_temperature");
        let forecast = as_open_meteo(&answer, OFFSET, noon()).unwrap();
        assert_eq!(said_for(&forecast, "Hanoi", Day::Ahead, today()), None);
        assert!(said_for(&forecast, "Hanoi", Day::Tomorrow, today()).is_some());
        let mut answer = met_hanoi(false);
        for at in 3..7 {
            for window in ["next_1_hours", "next_6_hours"] {
                if let Some(window) =
                    answer["properties"]["timeseries"][at]["data"][window].as_object_mut()
                {
                    window.remove("summary");
                }
            }
        }
        let forecast = as_open_meteo(&answer, OFFSET, noon()).unwrap();
        assert_eq!(said_for(&forecast, "Hanoi", Day::Tomorrow, today()), None);
        assert_eq!(
            as_open_meteo(&json!({"properties": {}}), OFFSET, noon()),
            None
        );
        assert_eq!(as_open_meteo(&json!("nonsense"), OFFSET, noon()), None);
    }

    /// The same weather from either service is said in the same sentence.
    #[test]
    fn the_same_weather_from_both_services_is_said_the_same() {
        let met = as_open_meteo(&met_hanoi(true), OFFSET, noon()).unwrap();
        for day in [
            Day::Ahead,
            Day::Today,
            Day::Tomorrow,
            Day::Weekday("Wednesday"),
        ] {
            let from_met = said_for(&met, "Hanoi, Vietnam", day, today());
            assert!(from_met.is_some(), "{day:?}");
            assert_eq!(
                from_met,
                said_for(&open_meteo_hanoi(), "Hanoi, Vietnam", day, today()),
                "{day:?}"
            );
        }
    }

    /// MET's symbols in the words Open-Meteo's codes are said in.
    #[test]
    fn mets_symbols_are_said_in_open_meteos_words() {
        for (symbol, words) in [
            ("clearsky_day", "clear"),
            ("clearsky_night", "clear"),
            ("fair_polartwilight", "mainly clear"),
            ("partlycloudy_night", "partly cloudy"),
            ("cloudy", "overcast"),
            ("fog", "foggy"),
            ("lightrain", "light rain"),
            ("rain", "rain"),
            ("heavyrain", "heavy rain"),
            ("lightrainshowers_day", "light showers"),
            ("rainshowers_night", "showers"),
            ("heavyrainshowers_day", "heavy showers"),
            ("lightsleet", "snow"),
            ("heavysleetshowers_day", "snow"),
            ("snow", "snow"),
            ("lightsnowshowers_night", "snow"),
            ("rainandthunder", "thunderstorms"),
            ("lightssnowshowersandthunder_day", "thunderstorms"),
            ("heavysleetandthunder", "thunderstorms"),
            ("something_new", "mixed weather"),
        ] {
            assert_eq!(sky(code(symbol)), words, "{symbol}");
        }
    }

    /// The request carries the coordinates alone, to 4 decimals, and names
    /// the lamp as MET's terms ask.
    #[tokio::test]
    async fn the_request_carries_coordinates_alone_and_names_the_lamp() {
        assert_eq!(
            url(21.024_512_3, 105.841_17).as_deref(),
            Some(
                "https://api.met.no/weatherapi/locationforecast/2.0/compact?lat=21.0245&lon=105.8412"
            )
        );
        assert_eq!(
            url(-33.868_82, -151.209_3).as_deref(),
            Some(
                "https://api.met.no/weatherapi/locationforecast/2.0/compact?lat=-33.8688&lon=-151.2093"
            )
        );
        assert!(
            USER_AGENT.starts_with("lampOS/") && USER_AGENT.contains(env!("CARGO_PKG_VERSION"))
        );
        let (base, heads) = met_stub(met_hanoi(false).to_string()).await;
        let answered = (met_norway())(
            format!("{base}/weatherapi/locationforecast/2.0/compact?lat=21.0245&lon=105.8412"),
            super::super::super::public::OPEN_WITHIN,
        )
        .await;
        assert!(answered.is_ok(), "{answered:?}");
        let heads = heads.lock().unwrap();
        let agent = heads[0].lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("user-agent")
                .then(|| value.trim().to_owned())
        });
        assert_eq!(agent.as_deref(), Some(USER_AGENT), "{}", heads[0]);
    }

    /// A local server answering every request with `body`, keeping each
    /// request's head.
    async fn met_stub(body: String) -> (String, Arc<Mutex<Vec<String>>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let heads = Arc::new(Mutex::new(Vec::new()));
        let kept = heads.clone();
        let body: Arc<str> = body.into();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let (body, kept) = (body.clone(), kept.clone());
                tokio::spawn(async move {
                    let mut read = Vec::new();
                    let mut buffer = [0u8; 1024];
                    while !read.windows(4).any(|end| end == b"\r\n\r\n") {
                        match socket.read(&mut buffer).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => read.extend_from_slice(&buffer[..n]),
                        }
                    }
                    kept.lock()
                        .unwrap()
                        .push(String::from_utf8_lossy(&read).into_owned());
                    let answer = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(answer.as_bytes()).await;
                });
            }
        });
        (format!("http://{address}"), heads)
    }

    /// Open-Meteo's geocoding for Hanoi, as it answers.
    fn hanoi_found() -> Value {
        json!({"results": [{"id": 1, "name": "Hanoi", "latitude": 21.0245,
                            "longitude": 105.84117, "country": "Vietnam"}]})
    }

    /// Open-Meteo for the whole call: Hanoi found after 1.2 s, as on
    /// lamp-9adf on 18 Nov (#7593), then its forecast host's one address
    /// never opening, with no other way to it.
    fn open_meteo_dead() -> Fetch {
        Arc::new(|url: String, open_within| {
            Box::pin(async move {
                if url.contains("geocoding-api") {
                    tokio::time::sleep(Duration::from_millis(1200)).await;
                    return Ok(hanoi_found());
                }
                super::super::super::public::tests::one_address_dead(
                    open_meteo_hanoi().to_string(),
                    open_within,
                    false,
                )
                .await
            })
        })
    }

    fn at_noon() -> super::super::Clock {
        Arc::new(noon)
    }

    fn hanoi_offset() -> Offset {
        Arc::new(|_| OFFSET)
    }

    /// The whole call with Open-Meteo's forecast unreachable: with MET up
    /// its forecast comes within the 6 s, said as Open-Meteo's own would be
    /// had it come; with MET dead too the weather is unreached, still
    /// within them.
    #[tokio::test]
    async fn open_meteo_unreached_is_answered_from_met_within_six_seconds() {
        let (base, heads) = met_stub(met_hanoi(true).to_string()).await;
        let met_up: Fetch = Arc::new(move |url: String, open_within| {
            let url = url.replace("https://api.met.no", &base);
            Box::pin(async move { (met_norway())(url, open_within).await })
        });
        let met_dead: Fetch = Arc::new(|_, open_within| {
            Box::pin(super::super::super::public::tests::one_address_dead(
                met_hanoi(true).to_string(),
                open_within,
                false,
            ))
        });
        let backed = Weather::new(None, open_meteo_dead(), at_noon()).with_backup(
            Some("on"),
            met_up,
            hanoi_offset(),
        );
        let both_dead = Weather::new(None, open_meteo_dead(), at_noon()).with_backup(
            Some("on"),
            met_dead,
            hanoi_offset(),
        );
        let asked = Instant::now();
        let ((reached, reached_in), (unreached, unreached_in)) = tokio::join!(
            async {
                let answered = backed.answer(&json!({"place": "Hanoi"})).await;
                (answered, asked.elapsed())
            },
            async {
                let answered = both_dead.answer(&json!({"place": "Hanoi"})).await;
                (answered, asked.elapsed())
            },
        );
        assert!(reached_in < super::super::WITHIN, "{reached_in:?}");
        assert!(
            reached
                .how
                .starts_with("the forecast came from the backup service after ")
                && reached.how.ends_with(" ms"),
            "{}",
            reached.how
        );
        assert!(!reached.how.contains("Hanoi"), "{}", reached.how);
        let said = said_for(&open_meteo_hanoi(), "Hanoi, Vietnam", Day::Ahead, today()).unwrap();
        assert_eq!(
            reached.told,
            json!({"place": "Hanoi, Vietnam", "weather": said})
        );
        let heads = heads.lock().unwrap();
        assert_eq!(heads.len(), 1);
        assert!(
            heads[0].starts_with(
                "GET /weatherapi/locationforecast/2.0/compact?lat=21.0245&lon=105.8412 "
            ),
            "{}",
            heads[0]
        );
        assert_eq!(unreached.told, json!({"error": super::super::UNREACHED}));
        assert_eq!(
            unreached.how,
            "the weather was not fetched: connection timed out; nor from the backup service: connection timed out"
        );
        assert!(unreached_in <= super::super::WITHIN, "{unreached_in:?}");
    }

    /// Open-Meteo's forecast failing as `failing` says, its place found at
    /// once; MET answering Hanoi's week, each URL it is asked kept.
    fn failing_with(failing: Error) -> (Fetch, Arc<Mutex<Vec<String>>>, Fetch) {
        let open_meteo: Fetch = Arc::new(move |url: String, _| {
            let failing = failing.clone();
            Box::pin(async move {
                if url.contains("geocoding-api") {
                    Ok(hanoi_found())
                } else {
                    Err(failing)
                }
            })
        });
        let asked = Arc::new(Mutex::new(Vec::new()));
        let kept = asked.clone();
        let met: Fetch = Arc::new(move |url: String, _| {
            kept.lock().unwrap().push(url);
            Box::pin(async { Ok(met_hanoi(false)) })
        });
        (open_meteo, asked, met)
    }

    /// The backup is asked only when Open-Meteo can't be reached, or
    /// answers only with a server error (5xx, Thirteen's P3 on #7732), and
    /// the switch is `on`: never for another answer it gave (a 4xx, 429
    /// among them, or JSON not read), and with the switch off the call
    /// goes straight to "couldn't be reached".
    #[tokio::test]
    async fn the_backup_is_asked_only_when_open_meteo_is_unreached_or_errs_and_it_is_on() {
        let unreached_by = |switch: Option<&'static str>, failing: Error| async move {
            let (open_meteo, asked, met) = failing_with(failing);
            let weather =
                Weather::new(None, open_meteo, at_noon()).with_backup(switch, met, hanoi_offset());
            let answered = weather.answer(&json!({"place": "Hanoi"})).await;
            let asked = asked.lock().unwrap().clone();
            (answered, asked)
        };
        let (answered, asked) = unreached_by(Some("off"), Error::Timeout(OPENING)).await;
        assert_eq!(answered.told, json!({"error": super::super::UNREACHED}));
        assert_eq!(
            answered.how,
            "the weather was not fetched: connection timed out"
        );
        assert!(asked.is_empty(), "{asked:?}");
        for failing in [
            Error::Status(400),
            Error::Status(404),
            Error::Status(429),
            Error::UsageLimit(None),
            Error::Status(499),
            Error::Status(600),
            Error::Protocol("JSON"),
            Error::Transport("request"),
        ] {
            let (answered, asked) = unreached_by(Some("on"), failing.clone()).await;
            assert_eq!(
                answered.told,
                json!({"error": super::super::UNREACHED}),
                "{failing}"
            );
            assert_eq!(
                answered.how,
                format!("the weather was not fetched: {failing}")
            );
            assert!(asked.is_empty(), "{failing}: {asked:?}");
        }
        for failing in [
            Error::Transport(OPENING),
            Error::Timeout(OPENING),
            Error::Transport("address"),
            Error::Timeout("request"),
            Error::Status(500),
            Error::Status(502),
            Error::Status(503),
            Error::Status(599),
        ] {
            {
                let (answered, asked) = unreached_by(Some("on"), failing.clone()).await;
                assert_eq!(
                    answered.told["place"], "Hanoi, Vietnam",
                    "{failing}: {}",
                    answered.told
                );
                assert!(
                    answered
                        .how
                        .starts_with("the forecast came from the backup service")
                );
                assert_eq!(asked.len(), 1, "{failing}");
                assert!(
                    asked[0].ends_with("compact?lat=21.0245&lon=105.8412"),
                    "{asked:?}"
                );
                assert!(!asked[0].contains("Hanoi"), "{asked:?}");
            }
        }
        assert!(on(Some("on")) && !on(None) && !on(Some("off")));
    }

    /// The switch is off unless exactly `on` (El's ruling on #7700, 18
    /// Nov): unset, `off` or any other word, no backup is asked and
    /// Open-Meteo's forecast opens within the whole 6 s less its answer
    /// (5.2 s), with nothing left to a backup; `on`, it leaves the backup
    /// [`LEFT_FOR_IT`] (2.9 s) and the backup is asked.
    #[tokio::test(start_paused = true)]
    async fn the_backup_is_off_unless_on_and_off_leaves_open_meteo_the_whole_six_seconds() {
        for switch in [
            None,
            Some("off"),
            Some(""),
            Some("yes"),
            Some("ON"),
            Some("on"),
        ] {
            let bounds = Arc::new(Mutex::new(Vec::new()));
            let kept = bounds.clone();
            let open_meteo: Fetch = Arc::new(move |url: String, open_within| {
                kept.lock().unwrap().push(open_within);
                Box::pin(async move {
                    if url.contains("geocoding-api") {
                        Ok(hanoi_found())
                    } else {
                        Err(Error::Timeout(OPENING))
                    }
                })
            });
            let (_, asked, met) = failing_with(Error::Timeout(OPENING));
            let weather =
                Weather::new(None, open_meteo, at_noon()).with_backup(switch, met, hanoi_offset());
            let answered = weather.answer(&json!({"place": "Hanoi"})).await;
            let forecast_bound = bounds.lock().unwrap()[1];
            let asked = asked.lock().unwrap().len();
            if switch == Some("on") {
                assert!(on(switch));
                assert_eq!(answered.told["place"], "Hanoi, Vietnam");
                assert_eq!(asked, 1);
                assert_eq!(
                    forecast_bound,
                    super::super::WITHIN - LEFT_FOR_IT - super::super::FORECAST_ANSWERED_IN
                );
            } else {
                assert!(!on(switch), "{switch:?}");
                assert_eq!(answered.told, json!({"error": super::super::UNREACHED}));
                assert_eq!(
                    answered.how,
                    "the weather was not fetched: connection timed out"
                );
                assert_eq!(asked, 0, "{switch:?}");
                assert_eq!(
                    forecast_bound,
                    super::super::WITHIN - super::super::FORECAST_ANSWERED_IN,
                    "{switch:?}"
                );
            }
        }
    }

    /// The backup's forecast is kept as Open-Meteo's: a follow-up is
    /// answered from it, with nothing fetched.
    #[tokio::test]
    async fn the_backups_forecast_is_kept_as_open_meteos() {
        let (open_meteo, asked, met) = failing_with(Error::Timeout(OPENING));
        let weather =
            Weather::new(None, open_meteo, at_noon()).with_backup(Some("on"), met, hanoi_offset());
        weather.answer(&json!({"place": "Hanoi"})).await;
        let tomorrow = weather
            .answer(&json!({"place": "Hanoi", "day": "tomorrow"}))
            .await;
        assert_eq!(tomorrow.how, "from what was kept");
        assert_eq!(
            tomorrow.told["weather"],
            "In Hanoi, Vietnam: Tomorrow, Wednesday, 25 to 31°C, thunderstorms, 5.6 mm of rain."
        );
        assert_eq!(asked.lock().unwrap().len(), 1);
    }
}
