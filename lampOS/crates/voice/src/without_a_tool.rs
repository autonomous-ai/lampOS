//! What the model is told it cannot do when the function for it is not
//! declared: it never agrees to an action it has no function for, and says
//! how instead (#4595).
//!
//! On launch lamps "You can sleep until I come back to the desk" was
//! answered "Rest well, … I'll keep the light warm till you're back", with
//! no `go_to_sleep` declared (`LAMP_SLEEP_ASKED` off): the lamp stayed awake
//! and then answered the lunch chatter. "Lamp, I'm unplugging you" was
//! answered OK four times, with no `prepare_to_unplug` offered
//! (`LAMP_UNPLUG` off, unplug.md). So while a session has no `go_to_sleep`
//! it is told a hand rested 3 s on its head puts it to sleep, and while it
//! has no `prepare_to_unplug`, that it cannot get itself ready: with the
//! app's off button on (`LAMP_SHUTDOWN=on`), to be turned off from the app
//! first; without it, that pulling the plug is the only way for now, never
//! naming the app (the first builder's option (b) on #4595). With the
//! function declared nothing is added: the function is the answer.
//!
//! Gated on what the session is given, the same check the functions are
//! declared by: `go_to_sleep` only with the voice's switch and head sleep
//! ([`crate::sleep_asked`]), `prepare_to_unplug` only in a catalog of a
//! Skills run with its switch on (`lamp-skills` leaves the skill out
//! otherwise). No switch of its own: `LAMP_SHUTDOWN` is the voice's own
//! (it hands it to its ring worker too), passed to the provider worker as
//! [`SHUTDOWN_FLAG`], as `LAMP_SLEEP_ASKED` is.
use lamp_gemini::Tool;

/// The core skill `prepare-unplug`'s function
/// (`skills/prepare-unplug/skill.toml`): offered only by a Skills run with
/// `LAMP_UNPLUG=on` (unplug.md).
pub const UNPLUG_TOOL: &str = "prepare_to_unplug";

/// The provider worker's flag for `LAMP_SHUTDOWN=on`
/// ([`lamp_msgs::SHUTDOWN_SWITCH`]): the app's off button turns the lamp
/// off ([`crate::tools::Settings::shutdown_from_app`]).
pub const SHUTDOWN_FLAG: &str = "--shutdown-from-app";

/// The section's heading and its rule, told when something below is.
pub const NEVER_AGREE: &str = "\n\nWhat you have no function for\nNever agree to do something you have no function for, and never say you did it or will do it.";

/// Told without `go_to_sleep`: a hand held 3 s on the head is the sleep
/// (controls.md, `LAMP_HEAD_TOUCH`), and a tap wakes it.
pub const NO_SLEEP: &str = " You cannot put yourself to sleep because someone asks you to. If someone asks you to sleep, or to rest until they come back, do not agree and do not say good night as if you will: say in a few words of your own that they can put you to sleep by resting a hand on your head for three seconds, and wake you with a tap on your head.";

/// Told without `prepare_to_unplug`, with the app's off button on (cloud.md,
/// "The lamp turned off from the app"): it rests the lamp before it powers
/// off.
pub const NO_UNPLUG_APP: &str = " You cannot get ready to be unplugged. If someone says they are unplugging you, do not say OK as if you were ready: say in a few words of your own to turn you off from the app first, and unplug you once you are off.";

/// Told without `prepare_to_unplug` and without the app's off button: there
/// is no way to get it ready, so it says so, and names no app.
pub const NO_UNPLUG: &str = " You cannot get yourself ready to be unplugged. If someone says they are unplugging you, do not say OK as if you were ready: say plainly, in a few words of your own, that you cannot get yourself ready for it, so pulling the plug is the only way for now.";

/// The provider worker's flag, with the switch `on`.
pub fn arguments(value: Option<&str>) -> Vec<String> {
    if value == Some("on") {
        vec![SHUTDOWN_FLAG.to_owned()]
    } else {
        Vec::new()
    }
}

/// What a session is told, with `sleep` whether it has `go_to_sleep`,
/// `unplug` whether it has `prepare_to_unplug`, and `app` whether the app's
/// off button turns it off. Empty with both functions.
pub fn told(sleep: bool, unplug: bool, app: bool) -> String {
    if sleep && unplug {
        return String::new();
    }
    let mut told = NEVER_AGREE.to_owned();
    if !sleep {
        told.push_str(NO_SLEEP);
    }
    if !unplug {
        told.push_str(if app { NO_UNPLUG_APP } else { NO_UNPLUG });
    }
    told
}

/// [`told`] for a Live session given `tools`: a function counts whether
/// declared on its own or reached through `use_skill`
/// ([`crate::session_tools::names`]).
pub fn for_declared(tools: &[Tool], app: bool) -> String {
    let names = crate::session_tools::names(tools);
    let has = |name: &str| {
        names.contains(name) || names.contains(&format!("{}:{name}", crate::use_skill::NAME))
    };
    told(has(crate::sleep_asked::NAME), has(UNPLUG_TOOL), app)
}

/// [`told`] for the second route's turn: `go_to_sleep` with its flag
/// ([`crate::tools::Settings::sleep_asked`]), `prepare_to_unplug` when the
/// skills' catalogs offer it (`skills`, [`crate::tools::skill_tools`]).
pub fn for_turn(settings: &crate::tools::Settings, skills: &[Tool]) -> String {
    told(
        settings.sleep_asked,
        skills.iter().any(|tool| tool.name == UNPLUG_TOOL),
        settings.shutdown_from_app,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(name: &str) -> Tool {
        Tool {
            name: name.into(),
            description: "d".into(),
            parameters: None,
            silent: false,
        }
    }

    /// #4595: only `on` passes the switch, and the worker takes its flag.
    #[test]
    fn only_on_passes_the_app_off_button() {
        assert_eq!(arguments(Some("on")), [SHUTDOWN_FLAG]);
        for value in [None, Some("off"), Some("ON"), Some("1"), Some("")] {
            assert!(arguments(value).is_empty(), "{value:?}");
        }
        let flags = arguments(Some("on"));
        let (settings, rest) = crate::tools::take(&flags).unwrap();
        assert!(settings.shutdown_from_app);
        assert!(rest.is_empty());
        let (off, _) = crate::tools::take(&[]).unwrap();
        assert!(!off.shutdown_from_app);
    }

    /// #4595: no `prepare_to_unplug`, the app's off button on: turn it off
    /// from the app first.
    #[test]
    fn no_unplug_tool_with_the_app_off_button_says_the_app() {
        let told = for_declared(&[tool(crate::sleep_asked::NAME)], true);
        assert_eq!(told, format!("{NEVER_AGREE}{NO_UNPLUG_APP}"));
        assert!(told.contains("turn you off from the app first"));
    }

    /// #4595, option (b): no `prepare_to_unplug` and no app off button: it
    /// says pulling the plug is the only way, and never names the app.
    #[test]
    fn no_unplug_tool_without_the_app_off_button_says_the_plug_and_no_app() {
        let told = for_declared(&[tool(crate::sleep_asked::NAME)], false);
        assert_eq!(told, format!("{NEVER_AGREE}{NO_UNPLUG}"));
        assert!(told.contains("pulling the plug is the only way for now"));
        assert!(told.contains("do not say OK as if you were ready"));
        assert!(!told.contains("app"), "{told}");
    }

    /// #4595: with `prepare_to_unplug` (and `go_to_sleep`) nothing is told,
    /// the app's off button on or off.
    #[test]
    fn with_the_unplug_tool_nothing_of_it_is_told_either_way() {
        for app in [false, true] {
            let both = [tool(crate::sleep_asked::NAME), tool(UNPLUG_TOOL)];
            assert_eq!(for_declared(&both, app), "", "app {app}");
            let unplug_only = for_declared(&[tool(UNPLUG_TOOL)], app);
            assert_eq!(unplug_only, format!("{NEVER_AGREE}{NO_SLEEP}"), "app {app}");
        }
    }

    /// The sleep half is as before, whatever the app's off button: told
    /// without `go_to_sleep`, not with it.
    #[test]
    fn the_sleep_half_is_told_without_go_to_sleep_only() {
        for app in [false, true] {
            let neither = for_declared(&[], app);
            assert!(neither.starts_with(NEVER_AGREE), "{neither}");
            assert!(neither.contains(NO_SLEEP));
            assert!(neither.contains("resting a hand on your head for three seconds"));
            let sleeps = for_declared(&[tool(crate::sleep_asked::NAME)], app);
            assert!(!sleeps.contains(NO_SLEEP), "{sleeps}");
        }
    }

    /// A skill reached through `use_skill` is declared as much as one of
    /// its own.
    #[test]
    fn the_unplug_reached_through_use_skill_counts() {
        let reach = Tool {
            parameters: Some(serde_json::json!({
                "type": "object",
                "properties": {"skill": {"type": "string", "enum": [UNPLUG_TOOL]}}
            })),
            ..tool(crate::use_skill::NAME)
        };
        let told = for_declared(&[reach], false);
        assert!(!told.contains(NO_UNPLUG), "{told}");
        assert!(told.contains(NO_SLEEP));
    }

    /// The second route: `go_to_sleep` by its flag, the unplug by the
    /// skills' catalogs, the app line by the switch's flag.
    #[test]
    fn the_second_routes_turn_is_told_by_its_flags_and_catalog() {
        let off = crate::tools::Settings::default();
        let on = crate::tools::Settings {
            sleep_asked: true,
            ..Default::default()
        };
        let app = crate::tools::Settings {
            shutdown_from_app: true,
            ..on.clone()
        };
        assert_eq!(for_turn(&off, &[]), told(false, false, false));
        assert_eq!(for_turn(&on, &[tool("other"), tool(UNPLUG_TOOL)]), "");
        assert_eq!(
            for_turn(&on, &[tool("other")]),
            format!("{NEVER_AGREE}{NO_UNPLUG}")
        );
        assert_eq!(
            for_turn(&app, &[tool("other")]),
            format!("{NEVER_AGREE}{NO_UNPLUG_APP}")
        );
    }
}
