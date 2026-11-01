//! `LAMP_SECOND_CHECKING_LINE` (#4684): a round with no words of its own
//! that begins a search is begun with the lamp's own "Let me check.", and
//! the next round is told it was said; never beside the model's own words,
//! for a call that isn't a search, or with the switch off.
use super::*;

/// One turn as it went.
#[derive(Debug, PartialEq)]
struct Went {
    /// What was said, as `said:` labels.
    said: Vec<String>,
    /// Whether the next round was told the line was said.
    noted: bool,
}

const ANSWER: &str = "About 1,800 dollars, which may have changed.";

/// A round of `names`' calls and nothing else.
fn calls(names: &[&str]) -> Vec<net::Result<ChatEvent>> {
    let calls = names
        .iter()
        .enumerate()
        .map(|(at, name)| {
            call(
                &format!("c{at}"),
                name,
                json!({"query": "Aeron chair price"}),
            )
        })
        .collect();
    vec![Ok(ChatEvent::ToolCalls(calls)), Ok(ChatEvent::Done)]
}

/// "How much is an Aeron chair?" with round 1's `first` events, then
/// round 2's [`ANSWER`], the switch `on`, the owner's `language` (`None`:
/// English).
fn checking_turn(
    on: bool,
    language: Option<crate::language::Language>,
    first: Vec<net::Result<ChatEvent>>,
) -> Went {
    let second = vec![text(ANSWER), Ok(ChatEvent::Done)];
    let script = Arc::new(Script {
        heard: Mutex::new([Ok("How much is an Aeron chair?".to_owned())].into()),
        chats: Mutex::new(std::iter::once(first).chain([second]).collect()),
        ..Script::default()
    });
    let tools: ToolRunner = Arc::new(|_, _, _, _| Box::pin(async { json!({"results": []}) }));
    let mut engine = Engine::with_clock(
        script.clone(),
        Arc::new(|| "SYSTEM".to_owned().into()),
        tools,
        Arc::new(Instant::now),
    )
    .announcing(false)
    .saying_checking(on)
    .in_language(language);
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
    let said = labels
        .into_iter()
        .filter(|label| label.starts_with("said:"))
        .collect();
    let told = format!(
        "you already said aloud \"{}\"",
        crate::language::fixed_in(language, CHECKING)
    );
    let noted = script
        .asked
        .lock()
        .unwrap()
        .iter()
        .flatten()
        .any(|message| {
            message
                .content
                .as_ref()
                .and_then(|content| content.as_str())
                .is_some_and(|content| content.contains(&told))
        });
    Went { said, noted }
}

/// A search begun with nothing said: the lamp's own line first, then the
/// answer, and the next round told it was said. `fetch_page` too.
#[test]
fn a_search_begun_with_nothing_said_is_begun_with_let_me_check() {
    for name in ["web_search", "fetch_page"] {
        assert_eq!(
            checking_turn(true, None, calls(&[name])),
            Went {
                said: vec![format!("said:{CHECKING}"), format!("said: {ANSWER}")],
                noted: true
            },
            "{name}"
        );
    }
}

/// Off, the model's own words before the search, or a call that isn't a
/// search: nothing of the lamp's own, and no note.
#[test]
fn no_checking_line_when_off_beside_its_own_words_or_without_a_search() {
    let worded = || {
        let mut first = vec![text("Let me look that up.")];
        first.extend(calls(&["web_search"]));
        first
    };
    for (why, on, first) in [
        ("off", false, calls(&["web_search"])),
        ("its own words", true, worded()),
        ("not a search", true, calls(&["get_weather"])),
    ] {
        let went = checking_turn(on, None, first);
        assert!(!went.noted, "{why}: {went:?}");
        assert!(
            !went
                .said
                .iter()
                .any(|said| said == &format!("said:{CHECKING}")),
            "{why}: {went:?}"
        );
        assert!(
            went.said.iter().any(|said| said.ends_with(ANSWER)),
            "{why}: {went:?}"
        );
    }
}

/// With an owner's language the lamp's own line is said in it, and the next
/// round is told that line, never the English (#4473; `language::FIXED_LINES`
/// since #4713), as the product turns the switch on (#4707).
#[test]
fn a_search_begun_with_nothing_said_is_begun_in_the_owners_language() {
    use crate::language::{Language, fixed_in};
    for language in [
        Language::Vietnamese,
        Language::Japanese,
        Language::Chinese,
        Language::Korean,
    ] {
        let line = fixed_in(Some(language), CHECKING);
        assert_ne!(line, CHECKING, "{language:?} has its own words");
        assert_eq!(
            checking_turn(true, Some(language), calls(&["web_search"])),
            Went {
                said: vec![format!("said:{line}"), format!("said: {ANSWER}")],
                noted: true
            },
            "{language:?}"
        );
    }
}
