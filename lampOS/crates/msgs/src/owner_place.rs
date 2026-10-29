//! The owner's approximate place, as the app gives it (#4333,
//! `docs/owner-place.md`). One copy of its rules, read by `lamp-cloud`,
//! which shapes the app's request, by root's helper (`lamp-setup change`),
//! which writes the file, and by the voice, which tells the model the place
//! for a question that needs one.
//!
//! The place is rounded on a grid no finer than about a kilometre each way,
//! here, whatever the phone sent: [`rounded`]. Latitude in steps of 0.01°
//! (1.11 km); longitude in steps of 0.01° times the least whole number that
//! keeps a step at least 1 km wide at the cell's edge nearest the pole, so
//! a cell is never narrower than that, at any latitude (one hundredth up to
//! about 25° from the equator, two to about 63°, more nearer the poles).
//! Every reader rounds again, so a file written by hand is never finer
//! either.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Where the place is kept: beside the person's name, root's, 0600.
pub const FILE: &str = "/etc/lampOS/person/place";

/// The longest area name kept, in characters: the person's name's bound.
pub const NAME_MOST: usize = 64;

/// The years a `set_at` may fall in.
const SET_AT_YEARS: std::ops::RangeInclusive<u32> = 2020..=2099;

/// Why a `set_at` is refused.
const SET_AT_UNFIT: &str =
    "set_at must be when it was set, in ISO 8601 UTC, such as 2026-10-29T08:30:00Z";

/// The most bytes read of the file: a place is well under it.
const FILE_MOST: u64 = 1_024;

/// Hundredths of a degree of longitude in a whole turn: the widest step.
const TURN: i64 = 36_000;

/// A kilometre in hundredths of a degree on the equator (a degree is
/// 111.195 km on a sphere of the Earth's mean radius, 6371 km).
const KILOMETRE: f64 = 100.0 / 111.195;

/// The owner's place, rounded: the area's name, its rounded position in
/// degrees, and when the owner set it, in UTC to the second
/// (`2026-10-29T08:30:00Z`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Place {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub set_at: String,
}

/// `text`, an ISO 8601 time in UTC (`2026-10-29T08:30:00Z`, a fraction of
/// a second and `+00:00` for `Z` taken), as kept: to the second, with `Z`;
/// `None` for anything else, another offset included.
pub fn utc_time(text: &str) -> Option<String> {
    let text = text.trim();
    let rest = text
        .strip_suffix('Z')
        .or_else(|| text.strip_suffix('z'))
        .or_else(|| text.strip_suffix("+00:00"))?;
    let (date, time) = rest.split_once(['T', 't'])?;
    let time = match time.split_once('.') {
        Some((whole, fraction))
            if !fraction.is_empty() && fraction.bytes().all(|b| b.is_ascii_digit()) =>
        {
            whole
        }
        Some(_) => return None,
        None => time,
    };
    // Three numbers of these widths, between these separators.
    let numbers = |text: &str, separator: char, widths: [usize; 3]| -> Option<[u32; 3]> {
        let mut parts = text.split(separator);
        let mut found = [0; 3];
        for (slot, width) in found.iter_mut().zip(widths) {
            let part = parts.next()?;
            if part.len() != width || !part.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            *slot = part.parse().ok()?;
        }
        parts.next().is_none().then_some(found)
    };
    let date = numbers(date, '-', [4, 2, 2])?;
    let time = numbers(time, ':', [2, 2, 2])?;
    let (year, month, day) = (date[0], date[1], date[2]);
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    (SET_AT_YEARS.contains(&year)
        && (1..=days).contains(&day)
        && time[0] < 24
        && time[1] < 60
        && time[2] < 60)
        .then(|| {
            format!(
                "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
                time[0], time[1], time[2]
            )
        })
}

/// The place's file beside the person's name file `name` (setup's
/// configuration names that one).
pub fn beside_name(name: &Path) -> PathBuf {
    name.with_file_name("place")
}

/// How many hundredths of a degree one longitude step is at the rounded
/// latitude `lat`: the least whole number that keeps a step at least a
/// kilometre wide at the cell's edge nearest the pole.
pub fn longitude_step(lat: f64) -> i64 {
    let edge = (lat.abs() + 0.005).min(90.0).to_radians().cos();
    if edge * TURN as f64 <= KILOMETRE {
        return TURN;
    }
    ((KILOMETRE / edge).ceil() as i64).clamp(1, TURN)
}

/// `lat` and `lon` on the grid ([`longitude_step`]), or `None` when either
/// is not a position on Earth. Rounding a rounded place gives it back.
pub fn rounded(lat: f64, lon: f64) -> Option<(f64, f64)> {
    if !(lat.is_finite() && lon.is_finite() && lat.abs() <= 90.0 && lon.abs() <= 180.0) {
        return None;
    }
    let lat_hundredths = (lat * 100.0).round();
    let lat = lat_hundredths / 100.0 + 0.0;
    let step = longitude_step(lat);
    let lon_hundredths = ((lon * 100.0 / step as f64).round() as i64 * step).clamp(-18_000, 18_000);
    Some((lat, lon_hundredths as f64 / 100.0 + 0.0))
}

/// A control character, a line or paragraph separator, or a mark that turns
/// the text's direction: none belongs in a name (the person's name's rule).
fn breaks_or_turns(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{2028}' | '\u{2029}' | '\u{200e}' | '\u{200f}')
        || ('\u{202a}'..='\u{202e}').contains(&c)
        || ('\u{2066}'..='\u{2069}').contains(&c)
}

/// The area's name trimmed, or why it can't be kept. Never echoes it.
pub fn fit_name(name: &str) -> Result<String, &'static str> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > NAME_MOST || name.chars().any(breaks_or_turns) {
        return Err("name must be 1 to 64 characters on one line");
    }
    Ok(name.to_owned())
}

/// The place in `data` (`{"name", "lat", "lon", "set_at"}`, other fields
/// ignored), checked and rounded, or why not. No error holds a value.
pub fn fit(data: &serde_json::Value) -> Result<Place, &'static str> {
    let number = |field: &str| data.get(field).and_then(serde_json::Value::as_f64);
    let lat = number("lat")
        .filter(|lat| lat.abs() <= 90.0)
        .ok_or("lat must be a number from -90 to 90")?;
    let lon = number("lon")
        .filter(|lon| lon.abs() <= 180.0)
        .ok_or("lon must be a number from -180 to 180")?;
    let name = fit_name(
        data.get("name")
            .and_then(serde_json::Value::as_str)
            .ok_or("name must be 1 to 64 characters on one line")?,
    )?;
    let set_at = data
        .get("set_at")
        .and_then(serde_json::Value::as_str)
        .and_then(utc_time)
        .ok_or(SET_AT_UNFIT)?;
    let (lat, lon) = rounded(lat, lon).ok_or("lat and lon must be a place on Earth")?;
    Ok(Place {
        name,
        lat,
        lon,
        set_at,
    })
}

/// The file's text for `place`: one JSON line.
pub fn text(place: &Place) -> String {
    let mut line = serde_json::to_string(place).unwrap_or_default();
    line.push('\n');
    line
}

/// The place kept at `path`, checked and rounded again; `None` when there
/// is none, it can't be read, or it doesn't fit.
pub fn read(path: &Path) -> Option<Place> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(FILE_MOST + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > FILE_MOST {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    fit(&value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The width of a longitude step at `lat`'s cell edge nearest the pole,
    /// in kilometres, on a sphere of the Earth's mean radius.
    fn step_km(lat: f64) -> f64 {
        let edge = (lat.abs() + 0.005).min(90.0).to_radians().cos();
        longitude_step(lat) as f64 / 100.0 * 111.195 * edge
    }

    #[test]
    fn the_grid_is_never_finer_than_a_kilometre_each_way() {
        let mut lat = -89.99;
        while lat <= 89.99 {
            let (lat_r, _) = rounded(lat, 0.0).unwrap();
            assert!(step_km(lat_r) >= 1.0, "{lat_r}: {} km", step_km(lat_r));
            lat += 0.37;
        }
        assert_eq!(longitude_step(0.0), 1);
        assert_eq!(longitude_step(21.03), 1);
        assert_eq!(longitude_step(33.87), 2);
        assert_eq!(longitude_step(60.17), 2);
        assert_eq!(longitude_step(75.12), 4);
        assert_eq!(longitude_step(90.0), TURN);
    }

    #[test]
    fn a_place_is_rounded_to_its_cell_and_rounding_it_again_keeps_it() {
        // A house in Hanoi's Ba Dinh: to a hundredth each way there.
        assert_eq!(rounded(21.035_123, 105.834_987), Some((21.04, 105.83)));
        assert_eq!(rounded(21.034_9, 105.829_9), Some((21.03, 105.83)));
        // Near the equator a hundredth each way.
        assert_eq!(rounded(1.283_3, 103.851_9), Some((1.28, 103.85)));
        // Helsinki: two hundredths in longitude.
        assert_eq!(rounded(60.169_9, 24.938_4), Some((60.17, 24.94)));
        // South and west, and no negative zero.
        assert_eq!(rounded(-33.868_8, -151.209_3), Some((-33.87, -151.2)));
        assert_eq!(rounded(-0.001, -0.001), Some((0.0, 0.0)));
        assert!(rounded(-0.001, -0.001).unwrap().0.is_sign_positive());
        // At a pole longitude says nothing.
        assert_eq!(rounded(89.999, 123.4), Some((90.0, 0.0)));
        for (lat, lon) in [
            (21.035_123, 105.834_987),
            (60.169_9, 24.938_4),
            (-33.868_8, -151.209_3),
            (45.0, 179.999),
            (-45.0, -179.999),
            (75.123, 33.33),
        ] {
            let once = rounded(lat, lon).unwrap();
            assert_eq!(rounded(once.0, once.1), Some(once), "{lat} {lon}");
            // Never more than about a cell's diagonal away.
            assert!((once.0 - lat).abs() <= 0.005 + 1e-9);
            assert!(once.1.abs() <= 180.0);
        }
        for (lat, lon) in [
            (90.01, 0.0),
            (0.0, -180.01),
            (f64::NAN, 0.0),
            (0.0, f64::INFINITY),
        ] {
            assert_eq!(rounded(lat, lon), None);
        }
    }

    #[test]
    fn set_at_is_iso_8601_utc_kept_to_the_second() {
        for (given, kept) in [
            ("2026-10-29T08:30:00Z", "2026-10-29T08:30:00Z"),
            ("2026-10-29T08:30:00.123456Z", "2026-10-29T08:30:00Z"),
            ("2026-10-29T08:30:00+00:00", "2026-10-29T08:30:00Z"),
            (" 2028-02-29t23:59:59z ", "2028-02-29T23:59:59Z"),
        ] {
            assert_eq!(utc_time(given).as_deref(), Some(kept), "{given}");
        }
        for given in [
            "2026-10-29T08:30:00",
            "2026-10-29T15:30:00+07:00",
            "2026-10-29",
            "2026-13-01T00:00:00Z",
            "2027-02-29T00:00:00Z",
            "2026-10-29T24:00:00Z",
            "2026-10-29T08:30:00.Z",
            "2019-12-31T23:59:59Z",
            "2100-01-01T00:00:00Z",
            "26-10-29T08:30:00Z",
            "2026-10-29T08:30:00:00Z",
            "",
        ] {
            assert_eq!(utc_time(given), None, "{given}");
        }
    }

    #[test]
    fn the_apps_place_is_checked_rounded_and_no_error_holds_a_value() {
        let place = fit(&json!({
            "name": "  Ba Đình, Hà Nội ", "lat": 21.035_123, "lon": 105.834_987,
            "set_at": "2026-10-29T08:30:15.250Z", "street": "kept nowhere"
        }))
        .unwrap();
        assert_eq!(
            place,
            Place {
                name: "Ba Đình, Hà Nội".into(),
                lat: 21.04,
                lon: 105.83,
                set_at: "2026-10-29T08:30:15Z".into(),
            }
        );
        let line = text(&place);
        assert_eq!(
            line,
            "{\"name\":\"Ba Đình, Hà Nội\",\"lat\":21.04,\"lon\":105.83,\"set_at\":\"2026-10-29T08:30:15Z\"}\n"
        );
        let good =
            json!({"name": "Ba Dinh", "lat": 21.0, "lon": 105.8, "set_at": "2026-10-29T08:30:00Z"});
        for (field, value, why) in [
            ("lat", json!(91), "lat must be a number from -90 to 90"),
            ("lat", json!("21.03"), "lat must be a number from -90 to 90"),
            ("lon", json!(-181), "lon must be a number from -180 to 180"),
            (
                "name",
                json!(""),
                "name must be 1 to 64 characters on one line",
            ),
            (
                "name",
                json!("a\nb"),
                "name must be 1 to 64 characters on one line",
            ),
            (
                "name",
                json!("x".repeat(65)),
                "name must be 1 to 64 characters on one line",
            ),
            (
                "name",
                json!(7),
                "name must be 1 to 64 characters on one line",
            ),
            ("set_at", json!(1_793_000_000_u64), SET_AT_UNFIT),
            ("set_at", json!("2026-10-29T15:30:00+07:00"), SET_AT_UNFIT),
        ] {
            let mut data = good.clone();
            data[field] = value;
            assert_eq!(fit(&data), Err(why), "{field}");
        }
        let mut missing = good.clone();
        missing.as_object_mut().unwrap().remove("set_at");
        assert!(fit(&missing).is_err());
    }

    #[test]
    fn the_file_is_read_rounded_again_and_nothing_is_none() {
        let dir = std::env::temp_dir().join(format!("lamp-msgs-place-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = beside_name(&dir.join("name"));
        assert_eq!(path, dir.join("place"));
        assert_eq!(read(&path), None);
        // A file written finer, by hand, is read on the grid.
        std::fs::write(
            &path,
            r#"{"name":"Ba Dinh","lat":21.035123,"lon":105.834987,"set_at":"2026-10-29T08:30:00Z"}"#,
        )
        .unwrap();
        let place = read(&path).unwrap();
        assert_eq!((place.lat, place.lon), (21.04, 105.83));
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(read(&path), None);
        std::fs::write(&path, " ".repeat(2_000)).unwrap();
        assert_eq!(read(&path), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
