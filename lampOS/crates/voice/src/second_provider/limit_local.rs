//! Asks that need no model, done at the usage limit (#7346, done-when 4,
//! `LAMP_LIMIT_LOCAL_ASKS`): while the relay refuses the chat with its
//! usage limit, a heard English line that is plainly a light or volume
//! ask runs the same function the model would have called, and the lamp
//! says that function's own short line. Anything else goes on as the limit
//! does without the switch.
//!
//! The words are matched whole: once the lamp's name, a greeting and
//! politeness are set aside, every word left must belong to the ask, so
//! "it's too bright outside" or "is the light on?" is no ask. A question
//! is never one.

use serde_json::{Value, json};

/// The switch, exactly `on`. Off unless set.
pub const ENVIRONMENT: &str = "LAMP_LIMIT_LOCAL_ASKS";

/// Whether the switch is on.
pub fn on() -> bool {
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// Words an ask may hold that change nothing about which ask it is.
const FILLER: [&str; 26] = [
    "hey", "hi", "ok", "okay", "lamp", "lamb", "please", "can", "could", "would", "will", "you",
    "the", "a", "an", "it", "just", "now", "thanks", "thank", "so", "your", "for", "me", "be",
    "to",
];
/// Words that ask for a change, any of them.
const VERBS: [&str; 6] = ["make", "turn", "switch", "set", "go", "change"];
/// Words that make "warm" or "cool" a step, not the colour.
const A_LITTLE: [&str; 6] = ["bit", "little", "touch", "slightly", "tad", "more"];
/// Words that name the light.
const LIGHT: [&str; 4] = ["light", "lights", "colour", "color"];
/// Words that name the lamp's own voice.
const VOICE: [&str; 4] = ["volume", "voice", "yourself", "sound"];

/// The function a heard line asks for, with its arguments, if it is
/// plainly one of the light's or the volume's (`lamp_actions::catalog`).
pub(crate) fn matched(said: &str) -> Option<(&'static str, Value)> {
    if said.contains('?') {
        return None;
    }
    let lower = said.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .filter(|word| !word.is_empty())
        .collect();
    let (mut light, mut voice, mut verb, mut little) = (false, false, false, false);
    let mut rest: Vec<&str> = Vec::new();
    for word in words {
        if LIGHT.contains(&word) {
            light = true;
        } else if VOICE.contains(&word) {
            voice = true;
        } else if VERBS.contains(&word) {
            verb = true;
        } else if A_LITTLE.contains(&word) {
            little = true;
        } else if !FILLER.contains(&word) {
            rest.push(word);
        }
    }
    if light && voice {
        return None;
    }
    let plain = |name: &'static str| Some((name, json!({})));
    let ask = match rest.as_slice() {
        ["on"] if light => plain("light_on"),
        ["off"] | ["out"] if light => plain("light_off"),
        ["brighter"] | ["too", "dark"] => plain("light_brighter"),
        ["up"] if light => plain("light_brighter"),
        ["dimmer"] | ["dim"] | ["darker"] | ["softer"] | ["too", "bright"] => plain("light_dimmer"),
        ["down"] if light => plain("light_dimmer"),
        ["warmer"] => plain("light_warmer"),
        ["warm"] if little => plain("light_warmer"),
        ["cooler"] | ["whiter"] => plain("light_cooler"),
        ["cool"] if little => plain("light_cooler"),
        ["louder"] | ["speak", "up"] => plain("volume_up"),
        ["up"] if voice => plain("volume_up"),
        ["quieter"] | ["too", "loud"] => plain("volume_down"),
        ["down"] if voice => plain("volume_down"),
        [colour] if (light || verb) && !little => {
            let colour = if *colour == "violet" {
                "purple"
            } else {
                colour
            };
            lamp_msgs::Light::COLOURS
                .contains(&colour)
                .then(|| ("light_colour", json!({ "colour": colour })))
        }
        _ => None,
    }?;
    // A step of the light said of the voice, or of the voice said of the
    // light, is no plain ask.
    let of_voice = ask.0.starts_with("volume_");
    (!((of_voice && light) || (!of_voice && voice))).then_some(ask)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each line and the function it asks for; `None` for no plain ask.
    #[test]
    fn matches_only_plain_light_and_volume_asks() {
        let cases: &[(&str, Option<(&str, Value)>)] = &[
            ("Turn the light on.", Some(("light_on", json!({})))),
            (
                "Hey Lamp, lights off please.",
                Some(("light_off", json!({}))),
            ),
            ("Switch off the light", Some(("light_off", json!({})))),
            ("Lights out.", Some(("light_off", json!({})))),
            ("Make it brighter.", Some(("light_brighter", json!({})))),
            ("Brighter?", None),
            ("Lamp, it's too dark", None),
            ("Too dark.", Some(("light_brighter", json!({})))),
            ("Turn the light up.", Some(("light_brighter", json!({})))),
            ("Dim the light.", Some(("light_dimmer", json!({})))),
            (
                "Turn the light down a bit.",
                Some(("light_dimmer", json!({}))),
            ),
            ("Too bright.", Some(("light_dimmer", json!({})))),
            (
                "Make it a little warmer.",
                Some(("light_warmer", json!({}))),
            ),
            (
                "Make the light a bit warm.",
                Some(("light_warmer", json!({}))),
            ),
            (
                "Make it warm.",
                Some(("light_colour", json!({"colour": "warm"}))),
            ),
            ("Cooler please.", Some(("light_cooler", json!({})))),
            (
                "Make the light red.",
                Some(("light_colour", json!({"colour": "red"}))),
            ),
            (
                "Turn it blue.",
                Some(("light_colour", json!({"colour": "blue"}))),
            ),
            (
                "Set the light to violet.",
                Some(("light_colour", json!({"colour": "purple"}))),
            ),
            ("Louder.", Some(("volume_up", json!({})))),
            ("Speak up.", Some(("volume_up", json!({})))),
            ("Turn yourself up.", Some(("volume_up", json!({})))),
            ("Turn the volume down.", Some(("volume_down", json!({})))),
            ("Quieter, please.", Some(("volume_down", json!({})))),
            ("Too loud.", Some(("volume_down", json!({})))),
            // Not plain: questions, talk, the ambiguous and the mixed.
            ("Is the light on?", None),
            ("What time is it?", None),
            ("Set a timer for five minutes.", None),
            ("It's too bright outside today.", None),
            ("Turn it up.", None),
            ("Turn it off.", None),
            ("Red.", None),
            ("Cool, thanks.", None),
            ("Turn the light louder.", None),
            ("Make your voice brighter.", None),
            ("Make it brighter and tell me a joke.", None),
            ("Turn the light on and the volume up.", None),
            ("I said the lights were brighter yesterday.", None),
            ("", None),
        ];
        for (said, want) in cases {
            assert_eq!(&matched(said), want, "{said:?}");
        }
    }
}
