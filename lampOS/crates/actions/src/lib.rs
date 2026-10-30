//! The executive: what the lamp can be asked to do to itself, and the doing.
//!
//! The voice's model asks for a function by name (docs/tools.md). It never
//! supplies anything else: a name, and for two functions one word out of a
//! closed list. Each name maps to steps this code owns. Before every step
//! the present state is read; after it the state is read again; and what
//! goes back to the model is what was established, in words it can say:
//! "The volume is now 60 out of 100", "The light is already at its
//! brightest", "I could not confirm it changed". A command that was tried
//! is finished: nothing is retried and nothing is handed on.
//!
//! The executive owns no conversation and no microphone. It writes the
//! speaker's volume control, which nothing else writes, and it asks the
//! ring's owner for a light over the bus (`light/command`), reading back
//! what that owner says it shows (`light/state`). A function whose
//! hardware is not there is not offered: the list handed to the voice
//! (`tools.json`) holds only what can be done now.
//!
//! What is kept of V1 and what is not is in docs/actions.md. In short:
//! the idea is kept (the model names a command, the code owns the step,
//! every change is read back, and the result is said as it is); the sound
//! steps are this crate's own; the light's arithmetic is still V1's and
//! is to be replaced by a colour and a level when the ring's owner is
//! built.

pub mod caps;
pub mod caps_service;
pub mod light;
pub mod schedule;
pub mod server;
pub mod sound;
pub mod speaker_state;
pub mod volume_out;

use serde_json::{Value, json};

/// A function as the voice's model is told of it (docs/tools.md).
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct Function {
    pub name: &'static str,
    pub description: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Value>,
}

/// What became of one call: said to the model either way.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// What was established, as a sentence.
    Done(String),
    /// Done, and nothing changed: it already was so ("My light is already
    /// orange."). Said to the model as a `Done` is, and never answered
    /// short, since there is no change to be the answer (the reviewer and
    /// Third on #1520).
    Already(String),
    /// Why it was not done, or could not be confirmed, as a sentence.
    Failed(String),
}
impl Outcome {
    /// The line that answers the call.
    pub fn line(&self) -> Value {
        match self {
            Self::Done(said) | Self::Already(said) => {
                json!({"ok": true, "result": {"result": said}})
            }
            Self::Failed(said) => json!({"ok": false, "error": said}),
        }
    }

    /// The line that answers `name`, short when `short` and the call is
    /// one of the light's or the volume's and was done: the model is told
    /// to say only a word or two after it ([`SHORT_NEXT`]). The change is
    /// the answer, seen or heard; the words after it only cost time
    /// (`LAMP_SHORT_DONE`, the first builder on #705, 13:4x). What was
    /// established is still told, and a call not done, or one that changed
    /// nothing ([`Outcome::Already`]), is said as before. The line carries
    /// the call's own words too ([`short_say`], `"say"`), which the voice's
    /// second route speaks in place of a further chat round when every call
    /// of a turn is so answered (#2877).
    pub fn line_for(&self, name: &str, short: bool) -> Value {
        match self {
            Self::Done(said) if short && is_short(name) => {
                json!({"ok": true, "result": {"result": said, "next": SHORT_NEXT, "say": short_say(name)}})
            }
            _ => self.line(),
        }
    }
}

/// What the model is told to say after a light or volume change it asked
/// for, with `LAMP_SHORT_DONE=on`.
pub const SHORT_NEXT: &str = "It is done. About this change, say only a word or two, in your own words and not the same each time. Answer anything else they asked as you would.";

/// The switch for [`SHORT_NEXT`]: `on` only.
pub const SHORT_ENVIRONMENT: &str = "LAMP_SHORT_DONE";

/// Whether `name` is answered short with the switch: the light's and the
/// volume's functions, whose change is the answer.
pub fn is_short(name: &str) -> bool {
    name.starts_with("light_") || (name.starts_with("volume_") && !is_app_call(name))
}

/// The call's own short line for a change answered short (#2877): a step
/// is named by its way ("Brighter.", "Quieter."), anything else is
/// "Done.". At most two words, as the battery's row asks of the reply
/// after "make it brighter" (#2877).
pub fn short_say(name: &str) -> &'static str {
    match name {
        "light_brighter" => "Brighter.",
        "light_dimmer" => "Dimmer.",
        "light_warmer" => "Warmer.",
        "light_cooler" => "Cooler.",
        "volume_up" => "Louder.",
        "volume_down" => "Quieter.",
        _ => SHORT_DONE_SAY,
    }
}

/// [`short_say`] for a change with no way to name: on, off, a colour, mute
/// and unmute.
pub const SHORT_DONE_SAY: &str = "Done.";

/// The functions that can be carried out now.
pub fn catalog(sound: bool, light: bool) -> Vec<Function> {
    let plain = |name, description| Function {
        name,
        description,
        parameters: None,
    };
    let mut functions = Vec::new();
    if sound {
        functions.extend([
            plain(
                "volume_up",
                "Make your own voice louder by one step, when someone asks you to speak up, be louder or turn yourself up, or says you are too quiet. \"Turn yourself up\" and \"louder\" are your voice; \"turn the light up\" is your light (light_brighter).",
            ),
            plain(
                "volume_down",
                "Make your own voice quieter by one step, when someone asks you to keep it down, be quieter, speak more quietly or turn yourself down, or says you are too loud. \"Turn yourself down\" and \"quieter\" are your voice; \"turn the light down\" is your light (light_dimmer). Not for \"softer\" alone: that is your light too, unless they say it of your voice or sound.",
            ),
            plain(
                "volume_mute",
                "Silence your own speaker until you are asked to unmute it.",
            ),
            plain("volume_unmute", "Bring your own speaker back after it was muted."),
        ]);
    }
    if light {
        let names = lamp_msgs::Light::COLOURS;
        functions.extend([
            plain(
                "light_on",
                "Switch your own light on, when someone asks for light and names no colour.",
            ),
            plain("light_off", "Switch your own light off."),
            Function {
                name: "light_colour",
                description: "Your light can change colour: set your own light to one of its colours by name: white, warm (a warm white), orange, yellow, red, pink, purple (violet), blue, cyan or green. White and warm are colours here, not brightness: \"make your light white\" is white, and \"make it warm\" is warm. For a step warmer or cooler than it is, use light_warmer or light_cooler.",
                parameters: Some(json!({
                    "type": "object",
                    "properties": {"colour": {"type": "string", "enum": names}},
                    "required": ["colour"]
                })),
            },
            plain(
                "light_brighter",
                "Make your own light a step brighter, keeping its colour, when someone asks for more light or to turn the light up. Not for white or whiter: white is a colour (light_colour, or light_cooler for a step). Not for \"turn yourself up\": that is your voice (volume_up).",
            ),
            plain(
                "light_dimmer",
                "Make your own light a step dimmer, keeping its colour, when someone says it is too bright or harsh, or asks for it softer, a touch softer, gentler or lower, or to turn the light down. \"Softer\" with nothing said of your voice or sound is your light. Not for \"turn yourself down\": that is your voice (volume_down).",
            ),
            plain(
                "light_warmer",
                "Make your own light a step warmer in tone (white, then warm, then orange), keeping its brightness, when someone asks for it warmer, cosier or more golden. For a colour by name, warm included, use light_colour.",
            ),
            plain(
                "light_cooler",
                "Make your own light a step cooler in tone (orange, then warm, then white), keeping its brightness, when someone asks for it cooler, whiter, a cooler white, crisper or more like daylight. For a colour by name, white included, use light_colour.",
            ),
        ]);
    }
    functions
}

/// The app's calls (#1396), never offered to the model. The voice keeps
/// the same list (`lamp_voice::tools::APP_CALLS`) and a test of its holds
/// the two equal.
pub const APP_CALLS: [&str; 4] = ["volume_level", "volume_to", "volume_step", "volume_muted"];

/// Whether `name` is one of the app's calls.
pub fn is_app_call(name: &str) -> bool {
    APP_CALLS.contains(&name)
}

/// The list as the voice reads it.
pub fn catalog_file(functions: &[Function]) -> String {
    json!({"tools": functions}).to_string()
}

/// The hardware the executive may use, each part there or not.
pub struct Executive<S, L> {
    pub sound: Option<sound::Volume<S>>,
    pub light: Option<L>,
    /// The light's and the volume's calls answered short
    /// ([`Outcome::line_for`]).
    pub short: bool,
}

impl<S: sound::Mixer, L: light::Light> Executive<S, L> {
    /// What can be done now.
    pub fn offered(&mut self) -> Vec<Function> {
        let sound = self.sound.is_some();
        let light = self.light.as_mut().is_some_and(|light| light.there());
        catalog(sound, light)
    }

    /// One of the app's calls (#1396), answered with numbers, not
    /// sentences: `volume_level`, `volume_to` with `{"volume": 0..100}`, and
    /// for lamp-control's `volume` op `volume_step` with `{"step": "up" |
    /// "down"}` and `volume_muted` with `{"muted": true | false}`. The step
    /// and the mute are the voice's own (`volume_up`, `volume_mute`, ...): the
    /// same steps, floor and ceiling, and an unmute back to where it was.
    /// They are never in the model's list, so only a caller on the socket
    /// that names them (`lamp-cloud`) asks them. `None` for any other name.
    pub fn app_call(&mut self, name: &str, args: &Value) -> Option<Value> {
        if !is_app_call(name) {
            return None;
        }
        let Some(volume) = self.sound.as_mut() else {
            return Some(json!({"ok": false, "error": "this lamp has no volume control"}));
        };
        // The voice's own change, then the level it left, or why not.
        let changed = |outcome: Outcome, volume: &mut sound::Volume<S>| match outcome {
            Outcome::Done(_) | Outcome::Already(_) => volume.level(),
            Outcome::Failed(why) => Err(why),
        };
        let level = match name {
            "volume_level" => volume.level(),
            "volume_step" => match args.get("step").and_then(Value::as_str) {
                Some("up") => {
                    let outcome = volume.up();
                    changed(outcome, volume)
                }
                Some("down") => {
                    let outcome = volume.down();
                    changed(outcome, volume)
                }
                _ => Err("step must be \"up\" or \"down\"".to_owned()),
            },
            "volume_muted" => match args.get("muted").and_then(Value::as_bool) {
                Some(true) => {
                    let outcome = volume.mute();
                    changed(outcome, volume)
                }
                Some(false) => {
                    let outcome = volume.unmute();
                    changed(outcome, volume)
                }
                None => Err("muted must be true or false".to_owned()),
            },
            _ => match args
                .get("volume")
                .and_then(Value::as_u64)
                .filter(|volume| *volume <= 100)
            {
                Some(share) => volume.to_share(share as u8),
                None => Err("volume must be a whole number from 0 to 100".to_owned()),
            },
        };
        Some(match level {
            Ok(level) => json!({"ok": true, "level": level}),
            Err(why) => json!({"ok": false, "error": why}),
        })
    }

    /// Carry out one call. A name that is not offered, and anything but the
    /// one word a function takes, is refused before any hardware is read.
    pub fn call(&mut self, name: &str, args: &Value) -> Outcome {
        let nothing = args.as_object().is_none_or(serde_json::Map::is_empty) || args.is_null();
        let unknown = || Outcome::Failed("I can't do that.".into());
        match name {
            "volume_up" | "volume_down" | "volume_mute" | "volume_unmute" if nothing => {
                let Some(volume) = self.sound.as_mut() else {
                    return unknown();
                };
                match name {
                    "volume_up" => volume.up(),
                    "volume_down" => volume.down(),
                    "volume_mute" => volume.mute(),
                    _ => volume.unmute(),
                }
            }
            "light_on" | "light_off" | "light_brighter" | "light_dimmer" | "light_warmer"
            | "light_cooler"
                if nothing =>
            {
                let Some(light) = self.light.as_mut() else {
                    return unknown();
                };
                if !light.there() {
                    return unknown();
                }
                match name {
                    "light_on" => light::on(light),
                    "light_off" => light::off(light),
                    "light_brighter" => light::brighter(light),
                    "light_warmer" => light::warmer(light),
                    "light_cooler" => light::cooler(light),
                    _ => light::dimmer(light),
                }
            }
            "light_colour" => {
                let Some(light) = self.light.as_mut() else {
                    return unknown();
                };
                if !light.there() {
                    return unknown();
                }
                // One word, and one of the ten.
                let asked = args
                    .as_object()
                    .filter(|object| object.len() == 1)
                    .and_then(|object| object.get("colour"))
                    .and_then(Value::as_str);
                match asked {
                    Some(name) => light::colour(light, name),
                    None => Outcome::Failed("That is not a colour my light can show.".into()),
                }
            }
            _ => unknown(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_what_can_be_done_now_is_offered_and_every_function_is_one_the_voice_accepts() {
        assert!(catalog(false, false).is_empty());
        let sound: Vec<&str> = catalog(true, false).iter().map(|f| f.name).collect();
        assert_eq!(
            sound,
            ["volume_up", "volume_down", "volume_mute", "volume_unmute"]
        );
        let all = catalog(true, true);
        assert_eq!(all.len(), 11);
        // The bounds the provider link puts on a function (docs/tools.md).
        for function in &all {
            let mut name = function.name.bytes();
            assert!(name.next().is_some_and(|first| first.is_ascii_lowercase()));
            assert!(name.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'));
            assert!(function.name.len() <= 48 && function.description.len() <= 1_024);
            assert!(
                function
                    .parameters
                    .as_ref()
                    .is_none_or(|p| p.is_object() && p.to_string().len() <= 4_096)
            );
        }
        // The file is the shape the voice reads, and only one function
        // takes anything: a colour out of the ten.
        let file: Value = serde_json::from_str(&catalog_file(&all)).unwrap();
        let listed = file["tools"].as_array().unwrap();
        assert_eq!(listed.len(), 11);
        let taking: Vec<&Value> = listed
            .iter()
            .filter(|f| f.get("parameters").is_some())
            .collect();
        assert_eq!(taking.len(), 1);
        assert_eq!(
            taking[0]["parameters"]["properties"]["colour"]["enum"]
                .as_array()
                .unwrap()
                .len(),
            10
        );
        assert!(catalog_file(&all).len() < 65_536);
    }

    #[test]
    fn only_a_done_light_or_volume_call_is_answered_short_and_only_with_the_switch() {
        let done = Outcome::Done("The light is warm now.".into());
        for (name, say) in [
            ("light_warmer", "Warmer."),
            ("light_colour", "Done."),
            ("light_off", "Done."),
            ("volume_up", "Louder."),
            ("volume_down", "Quieter."),
            ("volume_mute", "Done."),
        ] {
            let short = json!({"ok": true, "result": {"result": "The light is warm now.", "next": SHORT_NEXT, "say": say}});
            assert_eq!(done.line_for(name, true), short, "{name}");
            // At most two words (#2877).
            assert!(say.split_whitespace().count() <= 2, "{name}");
            assert_eq!(done.line_for(name, false), done.line(), "{name}");
        }
        // The app's numbers and anything else are not the light or the
        // volume changed for the model.
        for name in ["volume_level", "volume_to", "set_timer", "recall"] {
            assert!(!is_short(name), "{name}");
            assert_eq!(done.line_for(name, true), done.line(), "{name}");
        }
        let failed = Outcome::Failed("I could not confirm it changed.".into());
        assert_eq!(failed.line_for("light_warmer", true), failed.line());
        // Nothing changed: said as without the switch, for the model to say.
        let already = Outcome::Already("My light is already orange.".into());
        assert_eq!(already.line_for("light_colour", true), already.line());
        assert_eq!(
            already.line(),
            json!({"ok": true, "result": {"result": "My light is already orange."}})
        );
        assert!(SHORT_NEXT.contains("a word or two"));
        // No one word to repeat after every change (rule 8): no example,
        // quoted or not.
        assert!(!SHORT_NEXT.contains("such as") && !SHORT_NEXT.contains('"'));
    }

    #[test]
    fn white_and_warm_are_named_as_colours_not_brightness() {
        // lamp-4ace, 17 Oct 15:26 (the first builder on #1316): "make your
        // light white" and "make it whiter" each went to light_brighter.
        let all = catalog(true, true);
        let said = |name: &str| all.iter().find(|f| f.name == name).unwrap().description;
        for colour in lamp_msgs::Light::COLOURS {
            assert!(said("light_colour").contains(colour), "{colour}");
        }
        assert!(said("light_colour").contains("not brightness"));
        assert!(said("light_brighter").contains("Not for white or whiter"));
        assert!(said("light_cooler").contains("whiter, a cooler white"));
        assert!(said("light_warmer").contains("warm included, use light_colour"));
    }

    /// The first builder's light scene on lamp-4ace (18 Oct, #1367): "a
    /// touch softer, please" turned the volume down. "Softer" alone is the
    /// light's, the volume's only when said of the voice or sound.
    #[test]
    fn softer_alone_is_the_light_not_the_volume() {
        let all = catalog(true, true);
        let said = |name: &str| all.iter().find(|f| f.name == name).unwrap().description;
        assert!(said("light_dimmer").contains("a touch softer"));
        assert!(said("light_dimmer").contains("nothing said of your voice or sound is your light"));
        assert!(said("volume_down").contains("Not for \"softer\" alone: that is your light"));
    }

    /// #1712, lamp-4ace 19 Oct: "turn yourself down" and "turn yourself back
    /// up" were taken for the light on the second route, both times; Live
    /// took them for the voice. "Yourself" turned down or up is the voice.
    #[test]
    fn turning_yourself_down_or_up_is_the_voice_not_the_light() {
        let all = catalog(true, true);
        let said = |name: &str| all.iter().find(|f| f.name == name).unwrap().description;
        assert!(said("volume_down").contains("turn yourself down"));
        assert!(said("volume_up").contains("turn yourself up"));
        assert!(
            said("light_dimmer").contains("Not for \"turn yourself down\": that is your voice")
        );
        assert!(
            said("light_brighter").contains("Not for \"turn yourself up\": that is your voice")
        );
    }

    /// lamp-4ace, 19 Oct (the batteries at 11:10 and 13:03, #1712): the
    /// DeepSeek route took "turn yourself down a bit" and "turn yourself
    /// back up" for the light both times, where Gemini Live turned the
    /// voice. "Turn yourself", louder and quieter are the voice; the light
    /// turned up or down is the light. The names stay as every route knows
    /// them.
    #[test]
    fn turning_yourself_up_or_down_is_the_voice_and_the_light_turned_is_the_light() {
        let all = catalog(true, true);
        let said = |name: &str| all.iter().find(|f| f.name == name).unwrap().description;
        for (voice, words) in [
            ("volume_up", ["turn yourself up", "louder", "too quiet"]),
            ("volume_down", ["turn yourself down", "quieter", "too loud"]),
        ] {
            for word in words {
                assert!(said(voice).contains(word), "{voice}: {word}");
            }
        }
        assert!(said("volume_up").contains("\"turn the light up\" is your light (light_brighter)"));
        assert!(
            said("volume_down").contains("\"turn the light down\" is your light (light_dimmer)")
        );
        assert!(said("light_brighter").contains("turn the light up"));
        assert!(
            said("light_brighter")
                .contains("Not for \"turn yourself up\": that is your voice (volume_up)")
        );
        assert!(said("light_dimmer").contains("turn the light down"));
        assert!(
            said("light_dimmer")
                .contains("Not for \"turn yourself down\": that is your voice (volume_down)")
        );
        assert_eq!(
            all.iter().map(|f| f.name).collect::<Vec<_>>(),
            [
                "volume_up",
                "volume_down",
                "volume_mute",
                "volume_unmute",
                "light_on",
                "light_off",
                "light_colour",
                "light_brighter",
                "light_dimmer",
                "light_warmer",
                "light_cooler"
            ]
        );
    }

    #[test]
    fn an_outcome_is_one_line_the_voice_can_hand_to_the_model() {
        assert_eq!(
            Outcome::Done("The volume is now 60 out of 100.".into()).line(),
            json!({"ok":true,"result":{"result":"The volume is now 60 out of 100."}})
        );
        assert_eq!(
            Outcome::Failed("I could not confirm it changed.".into()).line(),
            json!({"ok":false,"error":"I could not confirm it changed."})
        );
    }
}
