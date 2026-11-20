//! #8022: the first round's forces read a line past the lamp's name, as
//! `LAMP_MAKE_SKILL_ASKED`'s does ([`past_the_name_heard_as`]): its own
//! name set off by a stop ("Hey Lamp. Remind me …") and, on a line that
//! names the lamp, its name heard as another word ("Hey, Lynn, remind me
//! …"). On lamp-52e6 on 20 Nov the gates' timer and reminder asks said
//! `calls=none`. Said to a person ("Hey, Anna, …", no name heard), a line
//! is read as said, and not forced.
use super::*;
use crate::second_provider::net::Force;

/// The function `force` names for the person's one line `said`.
fn forced(force: &Force, said: &str) -> Option<&'static str> {
    force(&[Message::system("You are a lamp."), Message::user(said)])
        .map(|forced| forced.name)
}

/// Each word-read first-round force, its own natural ask as said after
/// "Hey Lamp, " and the function it is forced to.
fn forces() -> Vec<(&'static str, Force, &'static str, &'static str)> {
    vec![
        (
            "LAMP_MEMORY_KEEP_ASKED",
            crate::remember::asked::force(),
            "remember that I take my coffee black.",
            crate::remember::REMEMBER,
        ),
        (
            "LAMP_SCHEDULE_ASKED",
            crate::tools::schedule_asked::force(),
            "remind me in ten minutes to stretch.",
            crate::tools::schedule_asked::TIMER,
        ),
        (
            "LAMP_REMINDERS_ASKED",
            crate::tools::reminders_asked::force(),
            "what reminders do I have?",
            crate::tools::reminders_asked::LIST,
        ),
        (
            "LAMP_SKILL_ASKED",
            crate::tools::skill_asked::force(None),
            "movie light, please.",
            crate::tools::skill_asked::SCENE,
        ),
        (
            "LAMP_SECOND_REMEMBER_ASKED",
            crate::second_provider::worker::remember_asks(),
            "remember my face, I'm Kess.",
            crate::known_faces::REMEMBER,
        ),
    ]
}

/// The ask with its first letter in upper case, as it opens a sentence.
fn opening(ask: &str) -> String {
    let mut letters = ask.chars();
    letters
        .next()
        .map(|first| first.to_uppercase().chain(letters).collect())
        .unwrap_or_default()
}

#[test]
fn each_force_reads_its_ask_past_the_lamps_name_set_off_or_misheard() {
    for (switch, force, ask, name) in forces() {
        let plain = format!("Hey Lamp, {ask}");
        assert_eq!(forced(&force, &plain), Some(name), "{switch}: {plain}");
        let stopped = format!("Hey Lamp. {}", opening(ask));
        assert_eq!(forced(&force, &stopped), Some(name), "{switch}: {stopped}");
        let misheard = format!("Hey, Lynn, {ask}");
        crate::tools::make_skill_asked::heard_named(&misheard, true);
        assert_eq!(forced(&force, &misheard), Some(name), "{switch}: {misheard}");
        let to_a_person = format!("Hey, Anna, {ask}");
        crate::tools::make_skill_asked::heard_named(&to_a_person, false);
        assert_eq!(forced(&force, &to_a_person), None, "{switch}: {to_a_person}");
    }
}

/// The misheard name is read past only on a line that names the lamp: the
/// same words heard again with no name heard are read as said.
#[test]
fn a_misheard_name_is_read_past_only_while_the_line_names_the_lamp() {
    let force = crate::tools::schedule_asked::force();
    let said = "Hey, Lynn, set a timer for two minutes.";
    crate::tools::make_skill_asked::heard_named(said, true);
    assert_eq!(forced(&force, said), Some(crate::tools::schedule_asked::TIMER));
    crate::tools::make_skill_asked::heard_named(said, false);
    assert_eq!(forced(&force, said), None);
}
