//! `LAMP_NAME_NEAR`: a line's first word, set off and sounding like the
//! lamp's name, is the name only when lamp-name heard it near (0.5 or
//! more) within 1.3 s of the line's start. In 22 Oct's played rounds on
//! lamp-4ace, 24 of 132 lines that began with the name had it written as
//! another word ("Louvre", "Month", "Life", "Light", "Length"), and lamp-name
//! said nothing at its bar of 0.9 for some whose clean files scored 0.97
//! and more.
use super::*;

/// When the line's first sound was read, by this process's clock.
const READ_US: u64 = 5_000_000;

/// One line heard as `heard`, its first sound read at [`READ_US`], with
/// lamp-name's near hearing `near` (none: the switch off, no hook); what it
/// was asked as, if it was, and the line start each near question was
/// asked of. The rule holds a line whose words don't begin with the name.
async fn asked_as(heard: &str, near: Option<Option<f32>>) -> (Option<String>, Vec<u64>) {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(heard.to_owned())].into()),
        chats: Mutex::new([vec![text("Sure."), Ok(ChatEvent::Done)]].into()),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script.clone());
    let mut engine = engine.heeding(Arc::new(|_, _, words, _| {
        (!crate::attention::begins_with_the_name(words)).then_some(crate::attention::NOT_SPOKEN_TO)
    }));
    let asked_of: Arc<Mutex<Vec<u64>>> = Arc::default();
    if let Some(near) = near {
        let seen = asked_of.clone();
        engine = engine.hearing_the_name_near(Arc::new(move |began_us| {
            seen.lock().unwrap().push(began_us);
            near
        }));
    }
    engine.submit(start(1));
    engine.submit(ProviderInput::Audio {
        request: 1,
        privacy_generation: 1,
        sequence: 1,
        read_completed_at_us: READ_US,
        samples: vec![0; lamp_audio::CAPTURE_SAMPLES],
    });
    engine.submit(end(1));
    assert_eq!(turn(&mut engine).await.last().unwrap(), "complete");
    let asked = script
        .asked
        .lock()
        .unwrap()
        .first()
        .and_then(|messages| messages.last().cloned())
        .and_then(|message| message.content)
        .and_then(|content| content.as_str().map(str::to_owned));
    let asked_of = asked_of.lock().unwrap().clone();
    (asked, asked_of)
}

/// Both signs call: the word set off and like the name, and a near
/// hearing of 0.5 or more; it is asked as the name, the near hearing asked
/// of the line's first sound. Either alone calls nothing: no near hearing,
/// one under 0.5, "Thanks," with 0.6, a desk line "Right, …" with none or
/// with one, a word like the name not set off, or the switch off.
#[tokio::test]
async fn a_first_word_like_the_name_is_it_only_with_lamp_names_near_hearing() {
    let line = "Louvre, I want to start reading more.";
    assert_eq!(
        asked_as(line, Some(Some(0.62))).await,
        (
            Some("Lamp, I want to start reading more.".to_owned()),
            vec![READ_US]
        )
    );
    assert_eq!(
        asked_as("Hey Month, what time is it?", Some(Some(0.71)))
            .await
            .0
            .as_deref(),
        Some("Hey Lamp, what time is it?")
    );
    assert_eq!(
        asked_as("Lump.", Some(Some(0.5))).await.0.as_deref(),
        Some("Lamp.")
    );
    for (heard, near) in [
        (line, Some(None)),
        (line, Some(Some(0.49))),
        (line, None),
        ("Thanks, I want to start reading more.", Some(Some(0.6))),
        ("Lynn, pass me that", Some(Some(0.6))),
        ("Right, I'll be there at four.", Some(None)),
        ("Right, I'll be there at four.", Some(Some(0.6))),
        ("Louvre is a museum in Paris.", Some(Some(0.9))),
    ] {
        assert_eq!(asked_as(heard, near).await.0, None, "{heard} {near:?}");
    }
}

/// The word's shape: one of the words written for "Lamp" on lamp-4ace,
/// or one syllable from an L to an M, P or N. Never a person's name of
/// that shape ("Lynn", "Lan", "Liam"), a word with an apostrophe, two
/// syllables, or another ending.
#[test]
fn a_word_sounds_like_the_name_by_its_shape_or_as_heard_for_it() {
    use super::super::sounds_like_the_name;
    for like in [
        "Louvre", "Month", "Life", "Light", "Length", "line", "LUMP", "loom", "Lean", "Learn",
        "lawn", "Lamb", "lip",
    ] {
        assert!(sounds_like_the_name(like), "{like}");
    }
    for not in [
        "Thanks", "Right", "Like", "Look", "Land", "Listen", "Lemon", "Lamps", "Lane's", "Mike",
        "Lynn", "Lane", "Lan", "Lin", "Liam", "Lam", "Loan", "What", "No", "", "1",
    ] {
        assert!(!sounds_like_the_name(not), "{not}");
    }
}

/// Set off as a name is: first, past a greeting at most, then a comma or
/// a stop, or the whole line.
#[test]
fn a_first_word_is_set_off_by_a_comma_a_stop_or_the_lines_end() {
    use super::super::first_word_set_off;
    fn word(text: &str) -> Option<&str> {
        first_word_set_off(text).map(|(from, to)| &text[from..to])
    }
    assert_eq!(word("Louvre, I want"), Some("Louvre"));
    assert_eq!(word("  Life. I need a gift"), Some("Life"));
    assert_eq!(word("Hey Month, what"), Some("Month"));
    assert_eq!(word("Hey, Life."), Some("Life"));
    assert_eq!(word("Lynn"), Some("Lynn"));
    assert_eq!(word("Louvre is a museum"), None);
    assert_eq!(word("Hey"), None);
    assert_eq!(word(""), None);
}
