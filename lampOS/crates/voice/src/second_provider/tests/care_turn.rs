//! `LAMP_CARE_LINE` on the second route (#7689): a line with words of
//! self-harm is told so beside it, and its answer is followed by the fixed
//! care line for the lamp's region and language, never the model's words.
//! An ordinary line, and every line with the switch off, go on as before.
use super::*;

/// One spoken turn heard as `heard`, answered `reply`: what was said, and
/// whether the model was told the care note.
fn care_turn(
    care: Option<crate::care_line::Care>,
    heard: &str,
    reply: &str,
) -> (Vec<String>, bool) {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(heard.to_owned())].into()),
        chats: Mutex::new([vec![text(reply), Ok(ChatEvent::Done)]].into()),
        ..Script::default()
    });
    let (mut engine, _, _) = scripted(script.clone());
    engine = engine.announcing(false).caring(care.map(Arc::new));
    let labels = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            engine.submit(start(1));
            engine.submit(audio(1, 1));
            engine.submit(end(1));
            turn(&mut engine).await
        });
    let told = script
        .asked
        .lock()
        .unwrap()
        .iter()
        .flatten()
        .filter_map(|message| message.content.as_ref().and_then(Value::as_str))
        .any(|content| content.contains(crate::care_line::NOTE));
    let said = labels
        .into_iter()
        .filter(|label| label.starts_with("said:"))
        .collect();
    (said, told)
}

fn care(region: Option<&str>) -> Option<crate::care_line::Care> {
    Some(crate::care_line::Care::new(
        crate::care_line::CareLines::shipped(),
        region.map(str::to_owned),
        "en",
    ))
}

const REPLY: &str = "I'm here with you.";

#[test]
fn words_of_self_harm_are_noted_and_the_regions_line_follows_the_answer() {
    let (said, told) = care_turn(care(Some("US")), "honestly I want to kill myself", REPLY);
    assert!(told, "the model is told");
    let line = crate::care_line::CareLines::shipped()
        .line(Some("US"), "en")
        .to_owned();
    assert_eq!(said, [format!("said:{REPLY}"), format!("said:{line}")]);
}

/// A lamp whose region isn't known says the default line, never another
/// country's number.
#[test]
fn a_lamp_in_no_known_region_says_the_default_line() {
    let (said, _) = care_turn(care(None), "I don't want to live anymore", REPLY);
    assert_eq!(
        said.last().map(String::as_str),
        Some(
            "said:Please reach out to someone you trust, or your local emergency number, right now."
        )
    );
    assert!(!said.iter().any(|line| line.contains("988")));
}

#[test]
fn an_ordinary_line_and_the_switch_off_go_on_as_before() {
    let (said, told) = care_turn(care(Some("US")), "what's the weather like", "Sunny.");
    assert_eq!((said, told), (vec!["said:Sunny.".to_owned()], false));
    let (said, told) = care_turn(None, "honestly I want to kill myself", REPLY);
    assert_eq!((said, told), (vec![format!("said:{REPLY}")], false));
}
