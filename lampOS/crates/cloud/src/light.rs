//! The app's Light row (`led.resting.*`, Fourth on #1396) and its faces row
//! (`led.faces.*`) on this lamp's light. V1's resting light is any
//! `[r, g, b]`; this lamp's is the person's own light (`lamp_msgs::Light`):
//! on or off, a colour, a brightness of ten steps, what the ring rests in
//! between turns. A colour the app sends is kept as it is: its hue is the
//! light's own `tint` (one of the named colours only when it is exactly
//! that), at the step its brightest channel comes to. The app is told the
//! light back in V1's shape (`docs/mqtt.md`, `led.resting.*`), with what
//! this lamp takes: any colour, ten levels. The faces of listening,
//! thinking and speaking are the person's too, in colour and brightness
//! (`lamp_msgs::LightFaces`).
use lamp_msgs::{FaceLook, Light, LightFaces};
use serde_json::{Map, Value, json};

/// How the ring moves while a light is on: a slow breath, never a still
/// disc (`ring_face`'s levels).
const ON_EFFECT: &str = "breathing";
/// V1's look for a light that is off.
const OFF_EFFECT: &str = "solid";

/// What the lamp takes for a colour: any `[r, g, b]`, kept as sent. Said in
/// `led.resting.get` and `led.faces.get`, so an app that reads it knows the
/// lamp keeps the colour chosen; a lamp that says nothing is the one with
/// ten named colours.
const COLOURS: &str = "any";

/// The faces the app may set, resting first: resting is the person's light.
pub const FACES: [&str; 4] = ["resting", "listening", "thinking", "speaking"];

/// The light before anyone has asked for one: on, warm, at its lowest, as
/// the ring starts (`ring_light::first`). The app's "default".
pub fn first() -> Light {
    Light {
        on: true,
        colour: "warm".into(),
        level: 1,
        tint: None,
        step: None,
    }
}

/// A light's colour as `[r, g, b]` with the brightest channel at 255: its
/// own, or its name's.
fn hue(light: &Light) -> Option<[u8; 3]> {
    light.tint.or_else(|| {
        Light::tint_of(&light.colour).map(|tint| tint.map(|share| (share * 255.0).round() as u8))
    })
}

/// The light as the app's colour: its hue at its step's share of 255. One
/// that is off is black, as V1 shows it.
pub fn rgb(light: &Light) -> [u8; 3] {
    let Some(hue) = hue(light) else {
        return [0; 3];
    };
    if !light.on {
        return [0; 3];
    }
    // In whole numbers, rounding half up, so that a step and its colour are
    // each other's inverse (`nearest`).
    let step = u32::from(light.step_now().min(Light::STEPS));
    hue.map(|channel| ((u32::from(channel) * step + 5) / u32::from(Light::STEPS)) as u8)
}

/// The light a hue and a step make, on: named when the hue is exactly a
/// named colour's, with its own `tint` and the nearest name otherwise.
fn light_of(hue: [u8; 3], step: u8) -> Option<Light> {
    let (name, exact) = Light::nearest_name(hue)?;
    let tint = Light::scaled(hue)?;
    Some(
        Light {
            on: true,
            colour: name.to_owned(),
            level: 1,
            tint: (!exact).then_some(tint),
            step: None,
        }
        .at_step(step.clamp(1, Light::STEPS)),
    )
}

/// The light the app's colour is: its hue kept, at the step its brightest
/// channel is nearest to, a faint colour being the lowest step and not off.
/// `None` for black, which V1 keeps as off.
pub fn nearest(rgb: [u8; 3]) -> Option<Light> {
    let brightest = rgb.into_iter().max().unwrap_or(0);
    if brightest == 0 {
        return None;
    }
    // The nearest step: step 3 is 77 of 255, and 77 is step 3. A faint
    // colour is the lowest step, not off.
    let step = ((u32::from(brightest) * u32::from(Light::STEPS) + 127) / 255) as u8;
    light_of(rgb, step)
}

/// The name of the light's colour when it is one: its own when the light
/// has no colour of its own, or when that colour is exactly a named one's.
fn named(light: &Light) -> Option<&str> {
    match light.tint {
        None => Some(&light.colour),
        Some(tint) => Light::nearest_name(tint)
            .filter(|(_, exact)| *exact)
            .map(|(name, _)| name),
    }
}

/// V1's snapshot of the resting light (`GET /led/resting`) for `shown`:
/// the choice (`off`, `default` while it is the light the lamp starts with,
/// else `custom` with its colour), the default look and the look in
/// effect. `light` is the light by its own names, which V1 has not, `name`
/// the colour's name when it is one there is, and `colours` and `levels`
/// what this lamp takes.
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
        "name": named(shown),
        "colours": COLOURS,
        "levels": Light::STEPS,
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
/// that light, `off` keeps the colour and the brightness, `custom` is its
/// colour as sent, and black is off, as V1 keeps it.
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

/// The light `led.resting.preview` shows for a while: its colour as sent,
/// or the light off for black.
pub fn previewed(data: &Value, now: Option<&Light>) -> Result<Light, String> {
    let rgb = colour(data)?;
    Ok(nearest(rgb).unwrap_or_else(|| Light {
        on: false,
        ..now.cloned().unwrap_or_else(first)
    }))
}

/// A face's own colour as the app sees it: the lamp's default for it.
fn default_hue(face: &str) -> [u8; 3] {
    let at = LightFaces::NAMES
        .iter()
        .position(|name| *name == face)
        .unwrap_or(0);
    LightFaces::DEFAULT_TINTS[at].map(|share| (share * 255.0).round() as u8)
}

/// What `led.faces.get` and `led.faces.set` answer: each face as its colour
/// (`[r, g, b]`, the colour alone, brightest channel at 255), its `level`
/// from 1 to `levels`, and whether it is the lamp's own (`default`); the
/// resting face is the person's light, with `on` too. And `colours` and
/// `levels`, what this lamp takes.
pub fn faces_snapshot(light: &Light, faces: &LightFaces) -> Value {
    let mut out = Map::new();
    out.insert(
        "resting".into(),
        json!({
            "color": hue(light).unwrap_or([255, 140, 0]),
            "level": light.step_now(),
            "on": light.on,
            "default": *light == first(),
        }),
    );
    for name in LightFaces::NAMES {
        let look = faces.look(name).flatten();
        out.insert(
            name.into(),
            json!({
                "color": look.map_or_else(|| default_hue(name), |look| look.tint),
                "level": look.map_or(Light::STEPS, |look| look.level),
                "default": look.is_none(),
            }),
        );
    }
    json!({"colours": COLOURS, "levels": Light::STEPS, "faces": out})
}

/// One face's look from what the app sent: `{"color": [r, g, b], "level":
/// N}`, the colour kept as its hue (its brightest channel scaled to 255).
fn look(face: &str, data: &Value) -> Result<FaceLook, String> {
    let tint = Light::scaled(colour(data)?).ok_or_else(|| {
        format!("{face}: a colour that is not black, its brightness is the level")
    })?;
    let level = data
        .get("level")
        .and_then(Value::as_u64)
        .and_then(|level| u8::try_from(level).ok())
        .filter(|level| (1..=Light::STEPS).contains(level))
        .ok_or_else(|| {
            format!(
                "{face}: level must be a whole number from 1 to {}",
                Light::STEPS
            )
        })?;
    Ok(FaceLook { tint, level })
}

/// What `led.faces.set` asks for, from the faces and the light now: the
/// faces named, each `"default"` (the lamp's own, and for resting the light
/// the lamp starts with) or `{"color", "level"}`; the faces not named stay.
/// The light to ask for is none when resting is not named.
pub fn faces_chosen(
    data: &Value,
    faces: LightFaces,
) -> Result<(Option<Light>, LightFaces), String> {
    let given = data.as_object().filter(|given| !given.is_empty()).ok_or(
        "led.faces.set takes {<face>: {\"color\": [r, g, b], \"level\": N} or \"default\"} for resting, listening, thinking or speaking",
    )?;
    let (mut light, mut faces) = (None, faces);
    for (face, value) in given {
        if !FACES.contains(&face.as_str()) {
            return Err(format!("{face:?} is not a face: {}", FACES.join(", ")));
        }
        let own = match value {
            Value::String(word) if word == "default" => None,
            Value::Object(_) => Some(look(face, value)?),
            _ => {
                return Err(format!(
                    "{face}: {{\"color\": [r, g, b], \"level\": N}} or \"default\""
                ));
            }
        };
        if face == "resting" {
            light = Some(match own {
                None => first(),
                Some(own) => light_of(own.tint, own.level).ok_or("resting: not black")?,
            });
        } else {
            faces = faces.with(face, own).unwrap_or(faces);
        }
    }
    Ok((light, faces))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn light(on: bool, colour: &str, level: u8) -> Light {
        Light {
            on,
            colour: colour.into(),
            level,
            tint: None,
            step: None,
        }
    }

    fn stepped(colour: &str, step: u8) -> Light {
        light(true, colour, 1).at_step(step)
    }

    /// Every light there is, told to the app as a colour and sent back as
    /// that colour, is the same light: a page that shows what it read and
    /// saves it unchanged never moves the lamp's light. Seventeen named
    /// colours at ten steps, and the lights of the old five levels.
    #[test]
    fn each_named_light_comes_back_from_its_own_colour() {
        for colour in Light::COLOURS.into_iter().chain(Light::SOFT_COLOURS) {
            for step in 1..=Light::STEPS {
                let lit = stepped(colour, step);
                assert_eq!(nearest(rgb(&lit)), Some(lit.clone()), "{colour} {step}");
            }
        }
        // A light in the old words comes back at its step.
        for level in 1..=Light::BRIGHTEST {
            let old = light(true, "blue", level);
            let back = nearest(rgb(&old)).unwrap();
            assert_eq!(back.step, Some(old.step_now()), "{level}");
            assert_eq!(back.colour, "blue");
            assert_eq!(back.tint, None);
        }
    }

    /// The colour chosen is kept: an app's colour that is no named one is
    /// the light's own tint, scaled to its brightest channel, never snapped
    /// to the nearest of the ten.
    #[test]
    fn a_colour_the_app_sends_is_kept_as_it_is_not_snapped() {
        let own = nearest([30, 200, 90]).unwrap();
        assert_eq!(own.tint, Some([38, 255, 115]));
        assert_eq!(own.step, Some(8));
        assert!(own.check().is_ok());
        // Told back as the same colour, and saved again unchanged.
        assert_eq!(rgb(&own), [30, 204, 92]);
        assert_eq!(nearest(rgb(&own)), Some(own.clone()));
        // Two colours the old ten would both call green are two lights.
        let other = nearest([60, 200, 20]).unwrap();
        assert_ne!(other.tint, own.tint);
        // A dim amber of the app's own is its own too: V1's example.
        let dim = nearest([8, 4, 1]).unwrap();
        assert_eq!(dim.tint, Some([255, 128, 32]));
        assert_eq!(dim.step, Some(1));
        // A named colour at any brightness is that name, with no tint.
        assert_eq!(nearest([51, 28, 0]), Some(stepped("warm", 2)));
        assert_eq!(nearest([255, 0, 0]), Some(stepped("red", 10)));
        assert_eq!(
            nearest([255, 255, 255]).map(|light| light.colour),
            Some("white".into())
        );
        assert_eq!(nearest([255, 173, 148]), Some(stepped("peach", 10)));
        // The faintest colour is the lowest step, not off; black is off.
        assert_eq!(nearest([1, 0, 0]), Some(stepped("red", 1)));
        assert_eq!(nearest([0, 0, 0]), None);
        assert_eq!(rgb(&light(false, "red", 5)), [0, 0, 0]);
        assert_eq!(rgb(&light(true, "red", 5)), [255, 0, 0]);
    }

    #[test]
    fn the_snapshot_is_v1s_shape_with_the_light_by_its_own_names_and_what_it_takes() {
        assert_eq!(
            snapshot(&first()),
            json!({
                "mode": "default",
                "color": null,
                "default": {"effect": "breathing", "color": [51, 28, 0]},
                "effective": {"effect": "breathing", "color": [51, 28, 0]},
                "light": {"on": true, "colour": "warm", "level": 1},
                "name": "warm",
                "colours": "any",
                "levels": 10,
            })
        );
        let blue = snapshot(&light(true, "blue", 3));
        assert_eq!(blue["mode"], "custom");
        assert_eq!(blue["color"], json!([0, 39, 179]));
        assert_eq!(blue["effective"]["color"], json!([0, 39, 179]));
        assert_eq!(blue["name"], "blue");
        let off = snapshot(&light(false, "blue", 3));
        assert_eq!(off["mode"], "off");
        assert_eq!(off["color"], Value::Null);
        assert_eq!(
            off["effective"],
            json!({"effect": "solid", "color": [0, 0, 0]})
        );
        assert_eq!(off["light"]["colour"], "blue");
        // A colour of its own is told as it is, with its nearest name in
        // the light and none as its own.
        let own = snapshot(&nearest([30, 200, 90]).unwrap());
        assert_eq!(own["color"], json!([30, 204, 92]));
        assert_eq!(own["light"]["tint"], json!([38, 255, 115]));
        assert_eq!(own["light"]["step"], 8);
        assert_eq!(own["name"], Value::Null);
        // One that is exactly a named colour is named, and a soft one too.
        assert_eq!(
            snapshot(&nearest([255, 173, 148]).unwrap())["name"],
            "peach"
        );
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
            Ok(stepped("red", 10))
        );
        assert_eq!(
            chosen(&json!({"mode": "custom", "color": [0, 0, 0]}), Some(&now)),
            Ok(light(false, "blue", 3))
        );
        let own = chosen(&json!({"mode": "custom", "color": [30, 200, 90]}), None).unwrap();
        assert_eq!(own.tint, Some([38, 255, 115]));
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
        let pink = previewed(&json!({"color": [20, 0, 10]}), Some(&now)).unwrap();
        assert_eq!(pink.tint, Some([255, 0, 128]));
        assert_eq!(pink.step, Some(1));
        assert_eq!(
            previewed(&json!({"color": [0, 0, 0]}), Some(&now)),
            Ok(light(false, "blue", 3))
        );
        assert!(previewed(&json!({"color": "red"}), Some(&now)).is_err());
    }

    /// `led.faces.get`: every face as a colour and a level, the lamp's own
    /// ones flagged `default`, with what the lamp takes.
    #[test]
    fn the_faces_snapshot_tells_each_face_and_what_the_lamp_takes() {
        let got = faces_snapshot(&first(), &LightFaces::default());
        assert_eq!(got["colours"], "any");
        assert_eq!(got["levels"], 10);
        let faces = &got["faces"];
        assert_eq!(
            faces["resting"],
            json!({"color": [255, 140, 0], "level": 2, "on": true, "default": true})
        );
        assert_eq!(
            faces["listening"],
            json!({"color": [128, 217, 255], "level": 10, "default": true})
        );
        assert_eq!(faces["thinking"]["color"], json!([255, 133, 10]));
        assert_eq!(faces["speaking"]["color"], json!([255, 184, 77]));
        // A face the person set is told as set; the others stay the lamp's.
        let set = LightFaces {
            speaking: Some(FaceLook {
                tint: [0, 255, 0],
                level: 4,
            }),
            ..LightFaces::default()
        };
        let got = faces_snapshot(&stepped("mint", 6), &set);
        assert_eq!(
            got["faces"]["speaking"],
            json!({"color": [0, 255, 0], "level": 4, "default": false})
        );
        assert_eq!(got["faces"]["listening"]["default"], true);
        assert_eq!(
            got["faces"]["resting"],
            json!({"color": [153, 255, 199], "level": 6, "on": true, "default": false})
        );
        assert_eq!(
            faces_snapshot(&light(false, "warm", 1), &set)["faces"]["resting"]["on"],
            false
        );
    }

    /// `led.faces.set`: the faces named change, the others stay, `default`
    /// gives a face back, resting is the person's light; bad input is refused.
    #[test]
    fn a_faces_choice_changes_only_the_faces_named_and_is_checked() {
        let before = LightFaces {
            thinking: Some(FaceLook {
                tint: [255, 0, 0],
                level: 3,
            }),
            ..LightFaces::default()
        };
        let (light, faces) = faces_chosen(
            &json!({"listening": {"color": [10, 20, 40], "level": 7}, "thinking": "default"}),
            before,
        )
        .unwrap();
        assert_eq!(light, None);
        assert_eq!(
            faces,
            LightFaces {
                listening: Some(FaceLook {
                    tint: [64, 128, 255],
                    level: 7
                }),
                ..LightFaces::default()
            }
        );
        let (light, faces) = faces_chosen(
            &json!({"resting": {"color": [255, 140, 0], "level": 4}}),
            before,
        )
        .unwrap();
        assert_eq!(light, Some(stepped("warm", 4)));
        assert_eq!(faces, before);
        let (light, _) = faces_chosen(&json!({"resting": "default"}), before).unwrap();
        assert_eq!(light, Some(first()));
        for (data, why) in [
            (json!({}), "led.faces.set takes"),
            (json!(null), "led.faces.set takes"),
            (json!({"offline": "default"}), "is not a face"),
            (json!({"speaking": "dim"}), "speaking:"),
            (json!({"speaking": {"color": [1, 2, 3]}}), "level must be"),
            (
                json!({"speaking": {"color": [1, 2, 3], "level": 0}}),
                "level must be",
            ),
            (
                json!({"speaking": {"color": [1, 2, 3], "level": 11}}),
                "level must be",
            ),
            (
                json!({"speaking": {"color": [1, 2], "level": 3}}),
                "color must be",
            ),
            (
                json!({"speaking": {"color": [0, 0, 0], "level": 3}}),
                "not black",
            ),
        ] {
            let why_not = faces_chosen(&data, before).unwrap_err();
            assert!(why_not.contains(why), "{data}: {why_not}");
        }
    }
}
