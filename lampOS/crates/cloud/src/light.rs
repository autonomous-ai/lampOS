//! The app's Light row (`led.resting.*`, Fourth on #1396) on this lamp's
//! light. V1's resting light is any `[r, g, b]`; this lamp's is the
//! person's own light (`lamp_msgs::Light`): on or off, one of ten colours
//! by name, a level from 1 to 5, what the ring rests in between turns. So a
//! colour the app sends is taken as the nearest of the ten, at the level
//! its brightest channel comes to, and the app is told the light's own
//! colours back in V1's shape (`docs/mqtt.md`, `led.resting.*`).
use lamp_msgs::Light;
use serde_json::{Value, json};

/// How the ring moves while a light is on: a slow breath, never a still
/// disc (`ring_face`'s levels).
const ON_EFFECT: &str = "breathing";
/// V1's look for a light that is off.
const OFF_EFFECT: &str = "solid";

/// The light before anyone has asked for one: on, warm, at its lowest, as
/// the ring starts (`ring_light::first`). The app's "default".
pub fn first() -> Light {
    Light {
        on: true,
        colour: "warm".into(),
        level: 1,
    }
}

/// The light as the app's colour: its tint at its level's share of 255.
/// One that is off is black, as V1 shows it.
pub fn rgb(light: &Light) -> [u8; 3] {
    let Some((_, tint)) = Light::TINTS.iter().find(|(name, _)| *name == light.colour) else {
        return [0; 3];
    };
    if !light.on {
        return [0; 3];
    }
    let share = f32::from(light.level.min(Light::BRIGHTEST)) / f32::from(Light::BRIGHTEST);
    tint.map(|channel| (channel * share * 255.0).round() as u8)
}

/// The nearest light there is to the app's colour: the tint closest to it
/// once both are scaled to their brightest channel, at the level that
/// channel reaches, rounded up so a faint colour is the lowest level and
/// not off. `None` for black, which V1 keeps as off.
pub fn nearest(rgb: [u8; 3]) -> Option<Light> {
    let brightest = rgb.into_iter().max().unwrap_or(0);
    if brightest == 0 {
        return None;
    }
    let scaled = rgb.map(|channel| f32::from(channel) / f32::from(brightest));
    let distance = |tint: &[f32; 3]| {
        let top = tint.iter().copied().fold(0.0_f32, f32::max);
        (0..3)
            .map(|i| (scaled[i] - tint[i] / top).powi(2))
            .sum::<f32>()
    };
    let (colour, _) = Light::TINTS
        .iter()
        .min_by(|(_, a), (_, b)| distance(a).total_cmp(&distance(b)))?;
    let level = (u32::from(brightest) * u32::from(Light::BRIGHTEST)).div_ceil(255) as u8;
    Some(Light {
        on: true,
        colour: (*colour).to_owned(),
        level: level.clamp(1, Light::BRIGHTEST),
    })
}

/// V1's snapshot of the resting light (`GET /led/resting`) for `shown`:
/// the choice (`off`, `default` while it is the light the lamp starts with,
/// else `custom` with its colour), the default look and the look in
/// effect. `light` is the light by its own names, which V1 has not.
pub fn snapshot(shown: &Light) -> Value {
    let mode = if !shown.on {
        "off"
    } else if *shown == first() {
        "default"
    } else {
        "custom"
    };
    let effect = if shown.on { ON_EFFECT } else { OFF_EFFECT };
    json!({
        "mode": mode,
        "color": (mode == "custom").then(|| rgb(shown)),
        "default": {"effect": ON_EFFECT, "color": rgb(&first())},
        "effective": {"effect": effect, "color": rgb(shown)},
        "light": shown,
    })
}

/// An `[r, g, b]` with channels 0-255, as V1 checks it.
pub fn colour(data: &Value) -> Result<[u8; 3], String> {
    let channels = data
        .get("color")
        .and_then(Value::as_array)
        .filter(|channels| channels.len() == 3)
        .ok_or("color must be [r, g, b]")?;
    let mut rgb = [0u8; 3];
    for (slot, channel) in rgb.iter_mut().zip(channels) {
        *slot = channel
            .as_u64()
            .and_then(|channel| u8::try_from(channel).ok())
            .ok_or("color channels must be 0-255")?;
    }
    Ok(rgb)
}

/// The light `led.resting.set` asks for, from what is shown now (`now`,
/// the light the lamp starts with when that is not known): `default` is
/// that light, `off` keeps the colour and the level, `custom` is the
/// nearest light to its colour, and black is off, as V1 keeps it.
pub fn chosen(data: &Value, now: Option<&Light>) -> Result<Light, String> {
    let now = now.cloned().unwrap_or_else(first);
    let off = Light { on: false, ..now };
    match data.get("mode").and_then(Value::as_str) {
        Some("default") => Ok(first()),
        Some("off") => Ok(off),
        Some("custom") => Ok(nearest(colour(data)?).unwrap_or(off)),
        other => Err(format!(
            "mode must be default, off or custom (got {:?})",
            other.unwrap_or("")
        )),
    }
}

/// The light `led.resting.preview` shows for a while: the nearest to its
/// colour, or the light off for black.
pub fn previewed(data: &Value, now: Option<&Light>) -> Result<Light, String> {
    let rgb = colour(data)?;
    Ok(nearest(rgb).unwrap_or_else(|| Light {
        on: false,
        ..now.cloned().unwrap_or_else(first)
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn light(on: bool, colour: &str, level: u8) -> Light {
        Light {
            on,
            colour: colour.into(),
            level,
        }
    }

    /// Every light there is, told to the app as a colour and sent back as
    /// that colour, is the same light: a page that shows what it read and
    /// saves it unchanged never moves the lamp's light.
    #[test]
    fn each_of_the_fifty_lights_comes_back_from_its_own_colour() {
        for colour in Light::COLOURS {
            for level in 1..=Light::BRIGHTEST {
                let lit = light(true, colour, level);
                assert_eq!(nearest(rgb(&lit)), Some(lit.clone()), "{colour} {level}");
            }
        }
    }

    #[test]
    fn a_colour_is_the_nearest_tint_at_the_level_its_brightest_channel_reaches() {
        // V1's own examples (`docs/mqtt.md`): a dim amber, and a dim
        // red-violet, half as much blue as red, nearer pink than purple.
        assert_eq!(nearest([8, 4, 1]), Some(light(true, "warm", 1)));
        assert_eq!(nearest([20, 0, 10]), Some(light(true, "pink", 1)));
        assert_eq!(nearest([160, 0, 255]), Some(light(true, "purple", 5)));
        assert_eq!(nearest([255, 0, 0]), Some(light(true, "red", 5)));
        assert_eq!(nearest([0, 0, 200]), Some(light(true, "blue", 4)));
        assert_eq!(nearest([255, 255, 255]), Some(light(true, "white", 5)));
        assert_eq!(nearest([128, 70, 0]), Some(light(true, "warm", 3)));
        // The faintest colour is the lowest level, not off; black is off.
        assert_eq!(nearest([1, 0, 0]), Some(light(true, "red", 1)));
        assert_eq!(nearest([0, 0, 0]), None);
        assert_eq!(rgb(&light(false, "red", 5)), [0, 0, 0]);
        assert_eq!(rgb(&light(true, "red", 5)), [255, 0, 0]);
    }

    #[test]
    fn the_snapshot_is_v1s_shape_with_the_light_by_its_own_names() {
        assert_eq!(
            snapshot(&first()),
            json!({
                "mode": "default",
                "color": null,
                "default": {"effect": "breathing", "color": [51, 28, 0]},
                "effective": {"effect": "breathing", "color": [51, 28, 0]},
                "light": {"on": true, "colour": "warm", "level": 1},
            })
        );
        let blue = snapshot(&light(true, "blue", 3));
        assert_eq!(blue["mode"], "custom");
        assert_eq!(blue["color"], json!([0, 34, 153]));
        assert_eq!(blue["effective"]["color"], json!([0, 34, 153]));
        let off = snapshot(&light(false, "blue", 3));
        assert_eq!(off["mode"], "off");
        assert_eq!(off["color"], Value::Null);
        assert_eq!(
            off["effective"],
            json!({"effect": "solid", "color": [0, 0, 0]})
        );
        assert_eq!(off["light"]["colour"], "blue");
    }

    #[test]
    fn a_choice_is_checked_as_v1_checks_it_and_off_keeps_the_colour() {
        let now = light(true, "blue", 3);
        assert_eq!(chosen(&json!({"mode": "default"}), Some(&now)), Ok(first()));
        assert_eq!(
            chosen(&json!({"mode": "off"}), Some(&now)),
            Ok(light(false, "blue", 3))
        );
        assert_eq!(
            chosen(&json!({"mode": "off"}), None),
            Ok(light(false, "warm", 1))
        );
        assert_eq!(
            chosen(&json!({"mode": "custom", "color": [255, 0, 0]}), Some(&now)),
            Ok(light(true, "red", 5))
        );
        assert_eq!(
            chosen(&json!({"mode": "custom", "color": [0, 0, 0]}), Some(&now)),
            Ok(light(false, "blue", 3))
        );
        for (data, why) in [
            (json!({"mode": "custom"}), "color must be [r, g, b]"),
            (
                json!({"mode": "custom", "color": [1, 2]}),
                "color must be [r, g, b]",
            ),
            (
                json!({"mode": "custom", "color": [1, 2, 256]}),
                "color channels must be 0-255",
            ),
            (
                json!({"mode": "custom", "color": [1, -2, 3]}),
                "color channels must be 0-255",
            ),
            (
                json!({"mode": "bright"}),
                "mode must be default, off or custom (got \"bright\")",
            ),
            (json!({}), "mode must be default, off or custom (got \"\")"),
        ] {
            assert_eq!(chosen(&data, Some(&now)), Err(why.to_owned()), "{data}");
        }
        assert_eq!(
            previewed(&json!({"color": [20, 0, 10]}), Some(&now)),
            Ok(light(true, "pink", 1))
        );
        assert_eq!(
            previewed(&json!({"color": [0, 0, 0]}), Some(&now)),
            Ok(light(false, "blue", 3))
        );
        assert!(previewed(&json!({"color": "red"}), Some(&now)).is_err());
    }
}
