//! The weather where the lamp stands, asked of Open-Meteo ahead of any
//! question (`LAMP_SECOND_WEATHER=on`, #1712): its place looked up once by
//! name, then the weather now, today and tomorrow every [`EVERY`], told to
//! the model beside the time, so "what's the weather" needs no search. The
//! first builder on #1712 (15:18): Open-Meteo, free and with no key. Only the
//! place's name and its coordinates leave the lamp.
use super::{Result, get_public_json};
use serde_json::Value;
use std::time::Duration;

/// The switch, in the voice service's environment.
pub const ENVIRONMENT: &str = "LAMP_SECOND_WEATHER";
/// How often the weather is asked.
pub const EVERY: Duration = Duration::from_secs(15 * 60);
/// The weather is told while it is this fresh, never older.
pub const FRESH: Duration = Duration::from_secs(60 * 60);
const GEOCODING: &str = "https://geocoding-api.open-meteo.com/v1/search";
const FORECAST: &str = "https://api.open-meteo.com/v1/forecast";

/// Whether the switch is on: `on` exactly.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
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

/// The forecast request for a place: now, and today's and tomorrow's.
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
            ("forecast_days", "2"),
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

/// The forecast in a few words for `place`: now, today and tomorrow, in
/// degrees Celsius. `None` without the weather now.
pub fn summary(answer: &Value, place: &str) -> Option<String> {
    let now = &answer["current"];
    let degrees = |value: &Value| value.as_f64().map(|value| value.round() as i64);
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
    let daily = &answer["daily"];
    for (day, name) in [(0, "Today"), (1, "Tomorrow")] {
        let at = |field: &str| daily[field].get(day).cloned().unwrap_or(Value::Null);
        let (Some(low), Some(high), Some(code)) = (
            degrees(&at("temperature_2m_min")),
            degrees(&at("temperature_2m_max")),
            at("weather_code").as_i64(),
        ) else {
            continue;
        };
        said.push_str(&format!(" {name} {low} to {high}°C, {}", sky(code)));
        if let Some(rain) = at("precipitation_probability_max").as_i64() {
            said.push_str(&format!(", {rain}% chance of rain"));
        }
        said.push('.');
    }
    Some(said)
}

/// The place's coordinates, looked up by name.
pub async fn locate(place: &str) -> Result<Option<(f64, f64)>> {
    let Some(url) = place_query(place).and_then(|name| geocoding_url(&name)) else {
        return Ok(None);
    };
    Ok(coordinates(&get_public_json(&url).await?))
}

/// The weather at the coordinates, in words for `place`.
pub async fn fetch(latitude: f64, longitude: f64, place: &str) -> Result<Option<String>> {
    let Some(url) = forecast_url(latitude, longitude) else {
        return Ok(None);
    };
    Ok(summary(&get_public_json(&url).await?, place))
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
        assert!(forecast.contains("forecast_days=2"));
    }

    #[test]
    fn the_switch_is_on_with_on_alone() {
        assert!(on(Some("on")));
        assert!(!on(Some("yes")) && !on(Some("")) && !on(None));
    }
}
