//! #8022 case 2: on lamp-52e6 on 20 Nov (15024) a spoken make ask, "make
//! me a skill that when I say apple file has an agent write three apple
//! facts into a file", went to `start_agent_task` twice of two, and the
//! journal could not say whether round 1 was forced to `make_skill`
//! (`LAMP_MAKE_SKILL_ASKED`) or whether the line was read as a make ask.
//! Round 1's line says what it was offered and forced, whether the line
//! asks to make a skill and how many sentences it has: names, `y`/`n` and a
//! count, never a word of the line.
use super::*;
use crate::second_provider::net::{Chat, Declare};

/// The words of the one-sentence make ask and of the same ask with a stop
/// set inside it, the shape speech to text may write.
const ONE: &str = "Lamp, make me a skill that when I say apple file, has an agent write three apple facts into a file for me.";
const TWO: &str = "Lamp, make me a skill that when I say apple file. Has an agent write three apple facts into a file for me.";

/// A chat that declares `start_agent_task`, and `make_skill` with `make`,
/// forced as `LAMP_MAKE_SKILL_ASKED` forces it.
fn chat(make: bool) -> Chat {
    let declare: Declare = Arc::new(move || {
        let mut tools = crate::remember::tools_with(false);
        tools.push(lamp_gemini::Tool {
            name: crate::agent_tasks::START.into(),
            description: "Hand work to an agent.".into(),
            parameters: None,
            silent: false,
        });
        if make {
            tools.push(crate::make_skill_local::tool());
        }
        tools
    });
    let chat = Chat::deepseek("deepseek-chat", "k", declare, false).unwrap();
    chat.forcing(crate::tools::make_skill_asked::force());
    chat
}

/// Round 1's line for `said` on `chat`, as the engine asks it.
fn line(chat: &Chat, said: &str) -> String {
    let messages = [Message::system("You are a lamp."), Message::user(said)];
    crate::second_provider::round_one_line(
        7,
        &messages,
        |name| Some(chat.declares(&messages, &[], name)),
        Some(chat.forced(&messages, &[])),
    )
}

#[test]
fn round_one_says_its_force_and_the_make_ask_never_its_words() {
    let made = chat(true);
    assert_eq!(
        line(&made, ONE),
        "lamp-voice second: request 7: round 1: task_offered=y make_offered=y forced=make_skill make_ask=y sentences=1"
    );
    assert_eq!(
        line(&made, TWO),
        "lamp-voice second: request 7: round 1: task_offered=y make_offered=y forced=none make_ask=n sentences=2"
    );
    // Without make_skill declared (`LAMP_MAKE_SKILL` off) nothing is
    // forced, though the line asks to make one.
    assert_eq!(
        line(&chat(false), ONE),
        "lamp-voice second: request 7: round 1: task_offered=y make_offered=n forced=none make_ask=y sentences=1"
    );
    // Services that cannot say what was offered or forced say `?`.
    let messages = [Message::user(ONE)];
    assert_eq!(
        crate::second_provider::round_one_line(7, &messages, |_| None, None),
        "lamp-voice second: request 7: round 1: task_offered=? make_offered=? forced=? make_ask=y sentences=1"
    );
    // A round whose last message is no person's line has no ask to read.
    let noted = [Message::user(ONE), Message::system("(A note.)")];
    assert!(
        crate::second_provider::round_one_line(7, &noted, |_| Some(true), Some(None))
            .ends_with("forced=none make_ask=? sentences=?")
    );
    for said in [ONE, TWO] {
        let journalled = line(&made, said);
        for word in ["Lamp,", "apple", "file.", "facts", "three", "agent"] {
            assert!(!journalled.contains(word), "{word:?} in {journalled}");
        }
    }
}

/// #8022 case 2's third cause: kit-8101's ROW (lamp-52e6, 20 Nov 14:38)
/// played the make ask, and its first word, set off, was the lamp's name
/// heard as another word (`wake_heard_as`): round 1 said `forced=none
/// make_ask=n sentences=1`. On a line that names the lamp, the force reads
/// the make past that word ([`past_the_name_heard_as`]), as the made-skill matcher
/// does; said to a person, the line names nothing and is not forced.
const LYNN: &str = "Hey, Lynn, make me a skill that, when I say apple file, has an agent write three apple facts into a file for me.";
const ANNA: &str = "Hey, Anna, make me a skill that, when I say apple file, has an agent write three apple facts into a file for me.";

#[test]
fn a_make_past_the_lamps_name_heard_as_another_word_is_forced_on_a_named_line_alone() {
    let made = chat(true);
    crate::tools::make_skill_asked::heard_named(LYNN, true);
    assert_eq!(
        line(&made, LYNN),
        "lamp-voice second: request 7: round 1: task_offered=y make_offered=y forced=make_skill make_ask=y sentences=1"
    );
    crate::tools::make_skill_asked::heard_named(ANNA, false);
    assert_eq!(
        line(&made, ANNA),
        "lamp-voice second: request 7: round 1: task_offered=y make_offered=y forced=none make_ask=n sentences=1"
    );
    // The same words heard again with no name heard are not forced.
    crate::tools::make_skill_asked::heard_named(LYNN, false);
    let again = line(&made, LYNN);
    assert!(again.ends_with("forced=none make_ask=n sentences=1"), "{again}");
}
