//! `LAMP_CARE_LINE` on the second route (#7689): a line with words of
//! self-harm is told so beside it, and its answer is followed by the fixed
//! care line for the lamp's region and language, never the model's words.
//! An ordinary line, and every line with the switch off, go on as before.
use super::*;

thread_local! {
    /// Whether the last [`care_turn_with`] told its model
    /// [`crate::care_line::OWED_NOTE`].
    static OWED_TOLD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// One spoken turn heard as `heard`, answered `reply`: what was said, and
/// whether the model was told the care note.
fn care_turn(
    care: Option<crate::care_line::Care>,
    heard: &str,
    reply: &str,
) -> (Vec<String>, bool) {
    care_turn_with(care.map(Arc::new), heard, reply, None)
}

/// [`care_turn`] with the route's care shared, and the said-to-me rule
/// `heed` when given.
fn care_turn_with(
    care: Option<Arc<crate::care_line::Care>>,
    heard: &str,
    reply: &str,
    heed: Option<Heed>,
) -> (Vec<String>, bool) {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(heard.to_owned())].into()),
        chats: Mutex::new([vec![text(reply), Ok(ChatEvent::Done)]].into()),
        ..Script::default()
    });
    let (mut engine, _, _) = scripted(script.clone());
    engine = engine.announcing(false).caring(care);
    if let Some(heed) = heed {
        engine = engine.heeding(heed);
    }
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
    let asked: Vec<String> = script
        .asked
        .lock()
        .unwrap()
        .iter()
        .flatten()
        .filter_map(|message| message.content.as_ref().and_then(Value::as_str))
        .map(str::to_owned)
        .collect();
    let told = asked
        .iter()
        .any(|content| content.contains(crate::care_line::NOTE));
    OWED_TOLD.with(|owed| {
        owed.set(
            asked
                .iter()
                .any(|content| content.contains(crate::care_line::OWED_NOTE)),
        )
    });
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

/// A care turn cut, stopped or taken over by a newer line never reached
/// its end: its line is owed, and the next answer, whatever it is, is
/// followed by it; once said, not again (the P1s on #7752).
#[test]
fn a_line_owed_by_a_cut_care_turn_follows_the_next_answer() {
    let care = Arc::new(crate::care_line::Care::new(
        crate::care_line::CareLines::shipped(),
        Some("US".into()),
        "en",
    ));
    // As `spoken` marks a care turn whose answer is then cut.
    care.mark(Instant::now(), false);
    let (said, told) = care_turn_with(Some(care.clone()), "sorry, go on", "I'm listening.", None);
    let line = crate::care_line::CareLines::shipped()
        .line(Some("US"), "en")
        .to_owned();
    assert!(!told, "an ordinary line is not told it is a care turn");
    // The safety read's P2 on #7752: told the lamp says the line itself,
    // so the number isn't said twice.
    assert!(
        OWED_TOLD.with(std::cell::Cell::get),
        "told the line follows"
    );
    assert_eq!(
        said,
        ["said:I'm listening.".to_owned(), format!("said:{line}")]
    );
    let (said, _) = care_turn_with(Some(care), "thanks", "Of course.", None);
    assert_eq!(said, ["said:Of course.".to_owned()], "said once");
    assert!(!OWED_TOLD.with(std::cell::Cell::get), "nothing owed now");
}

/// A line of self-harm is never held by the said-to-me rule (the first
/// builder's P2 on #7752): answered, and the line follows; an ordinary line
/// the rule holds is still held.
#[test]
fn words_of_self_harm_are_never_held_by_the_said_to_me_rule() {
    let holds: Heed = Arc::new(|_, _, _, _, _| Some("not said to the lamp"));
    let (said, told) = care_turn_with(
        care(Some("US")).map(Arc::new),
        "honestly I want to kill myself",
        REPLY,
        Some(holds.clone()),
    );
    assert!(told);
    assert_eq!(
        said.first().map(String::as_str),
        Some("said:I'm here with you.")
    );
    assert!(said.iter().any(|line| line.contains("988")), "{said:?}");
    let (said, _) = care_turn_with(
        care(Some("US")).map(Arc::new),
        "what's the weather like",
        "Sunny.",
        Some(holds),
    );
    assert!(said.is_empty(), "{said:?}");
}

/// A care turn's exchange is held from the running summary (the safety
/// read's P1 on #7752): marked when the route caught it.
#[test]
fn a_care_turns_request_is_held_from_the_summary() {
    let _ = care_turn(care(Some("US")), "honestly I want to kill myself", REPLY);
    assert!(crate::care_line::held_from_summary(1));
}
