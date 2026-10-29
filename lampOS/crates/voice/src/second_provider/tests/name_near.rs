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

/// [`asked_with`], nobody turned to the lamp.
async fn asked_as(heard: &str, near: Option<Option<f32>>) -> (Option<String>, Vec<u64>) {
    asked_with(heard, near, false).await
}

/// One line heard as `heard`, its first sound read at [`READ_US`], with
/// lamp-name's near hearing `near` (none: the switch off, no hook) and a
/// face `turned` to the lamp or not; what it was asked as, if it was, and
/// the line start each near question was asked of. The rule holds a line
/// whose words don't begin with the name.
async fn asked_with(
    heard: &str,
    near: Option<Option<f32>>,
    turned: bool,
) -> (Option<String>, Vec<u64>) {
    let script = Arc::new(Script {
        heard: Mutex::new([Ok(heard.to_owned())].into()),
        chats: Mutex::new([vec![text("Sure."), Ok(ChatEvent::Done)]].into()),
        ..Script::default()
    });
    let (engine, _, _) = scripted(script.clone());
    let mut engine = engine.heeding(Arc::new(|_, _, words, _, _| {
        (!crate::attention::begins_with_the_name(words)).then_some(crate::attention::NOT_SPOKEN_TO)
    }));
    let asked_of: Arc<Mutex<Vec<u64>>> = Arc::default();
    if let Some(near) = near {
        let seen = asked_of.clone();
        engine = engine.hearing_the_name_near(Arc::new(move |(began_us, _): (u64, u64)| {
            seen.lock().unwrap().push(began_us);
            (near, turned)
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
        asked_as(line, Some(Some(0.6))).await.0.as_deref(),
        Some("Lamp, I want to start reading more."),
        "a word written for the name needs no face"
    );
    // The shape alone is a call only with a face turned to the lamp
    // (reviewer-three's P2 on 194accd24): people's names have it too.
    assert_eq!(
        asked_with("Lump.", Some(Some(0.5)), true)
            .await
            .0
            .as_deref(),
        Some("Lamp.")
    );
    assert_eq!(
        asked_with("Leon, pass me that", Some(Some(0.6)), true)
            .await
            .0
            .as_deref(),
        Some("Lamp, pass me that")
    );
    for name in [
        "Leon", "Lynne", "Leann", "Laine", "Lyon", "Lon", "Lum", "Lem",
    ] {
        let heard = format!("{name}, pass me that");
        assert_eq!(
            asked_as(&heard, Some(Some(0.6))).await.0,
            None,
            "{heard}: no face turned to the lamp"
        );
    }
    assert_eq!(
        asked_with("Lynn, pass me that", Some(Some(0.6)), true)
            .await
            .0,
        None,
        "a name on the list, even turned to the lamp"
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
        "Louvre", "Month", "Life", "Light", "Length", "LUMP", "loom", "Lean", "Learn", "lawn",
        "lip",
    ] {
        assert!(sounds_like_the_name(like), "{like}");
    }
    for not in [
        "Thanks", "Right", "Like", "Look", "Land", "Listen", "Lemon", "Lamps", "Lane's", "Mike",
        "Lynn", "Lane", "Lan", "Lin", "Liam", "Lam", "Loan", "Leanne", "Lorne",
        // Compared after its silent "e", "line" is "Lin", a name; "Lamb",
        // its "-mb" an M, is "Lam" (the rule reads "Lamb" as the lamp's
        // name already).
        "line", "Lamb", "What", "No", "", "1",
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

/// Reviewer-three's P3 on 194accd24: a line that begins with an earlier
/// piece's words ("Louvre," given up, then "I need a gift for my sister.")
/// is never rewritten by a near hearing asked of its own start, which is
/// not that word's.
#[tokio::test]
async fn a_line_going_on_from_an_earlier_piece_is_not_rewritten() {
    let script = Arc::new(Script {
        heard: Mutex::new(
            [
                Ok("Louvre,".to_owned()),
                Ok("I need a gift for my sister.".to_owned()),
            ]
            .into(),
        ),
        chats: Mutex::new(
            [
                vec![text("Yes?"), Ok(ChatEvent::Done)],
                vec![text("Sure."), Ok(ChatEvent::Done)],
            ]
            .into(),
        ),
        ..Script::default()
    });
    let (engine, _, now) = scripted(script.clone());
    let mut engine = engine
        .heeding(Arc::new(|_, _, words, _, _| {
            (!crate::attention::begins_with_the_name(words))
                .then_some(crate::attention::NOT_SPOKEN_TO)
        }))
        .hearing_the_name_near(Arc::new(|_: (u64, u64)| (Some(0.6), true)));
    engine.submit(start(1));
    engine.submit(audio(1, 1));
    engine.submit(end(1));
    tokio::time::sleep(Duration::from_millis(20)).await;
    *now.lock().unwrap() += Duration::from_secs(2);
    engine.submit(ProviderInput::Start {
        request: 2,
        session: 1,
        privacy_generation: 1,
        continues: true,
        continues_cut_off: true,
        over_the_lamp: false,
        after_stop: false,
        follows_on: false,
        over_the_lamps_voice: false,
        heard_of_cut: None,
        soft_lead_us: 0,
    });
    engine.submit(audio(2, 1));
    engine.submit(end(2));
    turn(&mut engine).await;
    let asked: Vec<String> = script
        .asked
        .lock()
        .unwrap()
        .iter()
        .filter_map(|messages| {
            messages
                .last()?
                .content
                .as_ref()?
                .as_str()
                .map(str::to_owned)
        })
        .collect();
    assert!(
        !asked.iter().any(|words| words.contains("gift")),
        "the two pieces are judged as they came, not named: {asked:?}"
    );
}

/// #3945: no word written for the lamp's name on lamp-4ace, nor a
/// person's name of its shape, nor a word that isn't alphabetic, is one
/// said to a person (`LAMP_NAME_SAID_TO_A_PERSON`): a hearing at the
/// line's start on any of them still counts.
#[test]
fn no_word_heard_for_the_name_is_said_to_a_person() {
    use super::super::{HEARD_FOR_THE_NAME, NAMES_OF_ITS_SHAPE};
    use crate::attention::said_to_a_person;
    for word in HEARD_FOR_THE_NAME
        .iter()
        .chain(NAMES_OF_ITS_SHAPE.iter())
        .chain(["1", "2nd", "ma'am1", "--"].iter())
    {
        let line = format!("{word}, what time is it in London?");
        assert_eq!(said_to_a_person(&line), None, "{line}");
    }
    assert_eq!(said_to_a_person("Ma'am, the salt?"), Some("ma'am"));
}
