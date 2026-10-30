//! "Lamp, go to sleep" (#3748): a system function the model calls when the
//! person asks the lamp to sleep. The lamp says one fixed line, and once it
//! has played the coordinator asks the same sleep a hand held on the head
//! asks (`HandControls::ask_sleep`): the microphones close, the ring shows
//! its asleep face, and a tap on the head wakes it.
//!
//! Never while onboarding is under way (#3767): the model is told so, and
//! nothing sleeps. Declared only with `LAMP_SLEEP_ASKED=on`; never a skill.
use lamp_gemini::Tool;
use serde_json::{Value, json};

/// `on` for the function to be declared. Anything else: never.
pub const ENVIRONMENT: &str = "LAMP_SLEEP_ASKED";
/// The provider worker's flag for it.
pub const FLAG: &str = "--sleep-asked";
/// The function the model calls.
pub const NAME: &str = "go_to_sleep";
/// What the lamp says as it goes to sleep, before the microphones close.
pub const LINE: &str = lamp_msgs::SLEEP_LINE;
/// The latest a sleep is asked after the model was told its line, if the
/// line's turn never ends: the line takes about 3 s to say.
pub const LINE_PLAYED_WITHIN_US: u64 = 8_000_000;
/// What the model is told while onboarding is under way.
pub const NOT_NOW: &str = "Not now: we are in the middle of getting to know each other. Say so in a few words, and that they can ask again once we are done.";

/// The request that began while onboarding was under way, as the
/// coordinator marked it (`ProviderInput::Onboarding`): one at a time, as
/// requests are, kept for the worker's two routes alike.
static DURING_ONBOARDING: std::sync::Mutex<Option<u64>> = std::sync::Mutex::new(None);

/// `request` began while onboarding was under way: its call is not now.
pub fn mark_onboarding(request: u64) {
    if let Ok(mut marked) = DURING_ONBOARDING.lock() {
        *marked = Some(request);
    }
}

/// Whether `request` began while onboarding was under way.
pub fn during_onboarding(request: u64) -> bool {
    DURING_ONBOARDING
        .lock()
        .is_ok_and(|marked| *marked == Some(request))
}

/// Whether the switch is on in this process's environment.
pub fn switched_on() -> bool {
    std::env::var(ENVIRONMENT).as_deref() == Ok("on")
}

/// The provider worker's flag, with the switch `on`.
pub fn arguments(value: Option<&str>) -> Vec<String> {
    if value == Some("on") {
        vec![FLAG.to_owned()]
    } else {
        Vec::new()
    }
}

/// The worker's flag kept only with head sleep (`HandControls::head_sleep`):
/// without it the lamp would say "Tap my head to wake me" and stay awake
/// (#3835). Said once in the journal when the switch is on and it is dropped.
pub fn only_with_head_sleep(arguments: &mut Vec<String>, head_sleep: bool) {
    if !head_sleep && arguments.iter().any(|argument| argument == FLAG) {
        arguments.retain(|argument| argument != FLAG);
        eprintln!(
            "lamp-voice: {ENVIRONMENT}=on, but head sleep is off (LAMP_HEAD_TOUCH and the durable mute): go_to_sleep is not declared"
        );
    }
}

pub fn is_tool(name: &str) -> bool {
    name == NAME
}

pub fn tool() -> Tool {
    Tool {
        name: NAME.into(),
        description: "Call this only when the person asks you to go to sleep, or to sleep now. \
                      You will stop listening until they tap your head. Then say exactly what it \
                      gives you as \"say\", and nothing else."
            .into(),
        parameters: None,
        silent: false,
    }
}

/// What the model is told of its call: the line to say, or, while onboarding
/// is under way, that it is not now.
pub fn told(onboarding: bool) -> Value {
    if onboarding {
        json!({ "say": NOT_NOW })
    } else {
        json!({ "say": format!("Say only this, nothing before or after it: \"{LINE}\"") })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_on_declares_it() {
        assert_eq!(arguments(Some("on")), [FLAG]);
        for value in [None, Some("off"), Some("ON"), Some("1"), Some("")] {
            assert!(arguments(value).is_empty(), "{value:?}");
        }
    }

    #[test]
    fn it_is_declared_only_where_a_tap_can_wake_it() {
        let given = || vec!["--other".to_owned(), FLAG.to_owned()];
        let mut with = given();
        only_with_head_sleep(&mut with, true);
        assert_eq!(with, given());
        let mut without = given();
        only_with_head_sleep(&mut without, false);
        assert_eq!(without, ["--other"]);
    }

    #[test]
    fn the_model_is_told_the_line_or_not_now() {
        let asleep = told(false);
        assert!(asleep["say"].as_str().unwrap().contains(LINE));
        let walking = told(true);
        assert_eq!(walking["say"], NOT_NOW);
        assert!(!walking["say"].as_str().unwrap().contains(LINE));
    }

    #[test]
    fn only_the_marked_request_is_during_onboarding() {
        mark_onboarding(41);
        assert!(during_onboarding(41));
        assert!(!during_onboarding(42));
        mark_onboarding(43);
        assert!(!during_onboarding(41), "one at a time");
        assert!(during_onboarding(43));
    }

    #[test]
    fn the_tool_is_the_voices_own() {
        let tool = tool();
        assert_eq!(tool.name, NAME);
        assert!(tool.parameters.is_none());
        assert!(!tool.silent);
        assert!(is_tool(NAME) && !is_tool("sleep"));
    }
}
