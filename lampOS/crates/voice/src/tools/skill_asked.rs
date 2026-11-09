//! A line that is only a plain ask for a light scene or a light show calls
//! its skill (`LAMP_SKILL_ASKED`). On lamp-52e6 on 9 Nov (build 10883, the
//! product unit, played lines), "Lamp, movie light." and "Wait, movie light
//! please." were answered in words with no call, so the light never
//! changed, and "Lamp, show me a light show." and two more asks for a show
//! the same, three of three, though both skills were offered through
//! `use_skill` with lines that name those very asks ("movie light", "do a
//! light show"); "reading light" and "light my book" did call. The skills'
//! lines were right: the chat model answered the ask in words, as it did
//! the reminders before `LAMP_SCHEDULE_ASKED`. With the switch, such a
//! line's first chat round must call the skill (`tool_choice`): through
//! `use_skill`, its `skill` told only that one, when the skill is offered
//! there, or the skill's own function. The choice (which scene, which show)
//! is still the model's. A line with any other word, a round after a call,
//! and a skill not offered are asked as before. Off, nothing is forced.
use crate::known_faces::lower_words;
use crate::second_provider::net::{Force, Forced, Message};

/// Only the exact value `on`; off unless set.
pub const ENVIRONMENT: &str = "LAMP_SKILL_ASKED";

/// Whether the switch is on.
pub fn on() -> bool {
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// Light scenes' function.
pub const SCENE: &str = "light_scene";
/// Light show's function.
pub const SHOW: &str = "light_show";

/// The asks, as words in a row, and the skill's function each calls: the
/// scenes' names before "light" (light-scenes' `one_of`, and "relaxing" as
/// its SKILL.md says it) and a light show (light-show's SKILL.md).
// ponytail: two skills' asks named here, not read from the skills; a
// skill-declared list of asks when a third skill needs one.
const ASKS: &[(&[&str], &str)] = &[
    (&["reading", "light"], SCENE),
    (&["focus", "light"], SCENE),
    (&["relax", "light"], SCENE),
    (&["relaxing", "light"], SCENE),
    (&["movie", "light"], SCENE),
    (&["night", "light"], SCENE),
    (&["light", "show"], SHOW),
    (&["party", "lights"], SHOW),
];

/// Every other word the line may hold: a greeting, the lamp's name, a
/// "please", "can you", "give me a", "turn on the", "show me a", "do a",
/// "I'd like a". Any word not here or in the ask ("off", "don't", "no",
/// "what", "later", a story) leaves the line to the model: a missed force
/// only means the model answers as without the switch, while a wrong one
/// changes the light nobody asked to change.
const AROUND: &[&str] = &[
    "hey", "hi", "hello", "ok", "okay", "lamp", "so", "oh", "wait", "please", "now", "thanks",
    "thank", "you", "can", "could", "would", "will", "give", "me", "us", "set", "put", "turn",
    "switch", "on", "a", "an", "the", "my", "some", "i", "i'd", "id", "like", "want", "need", "do",
    "show", "start", "play", "make", "it", "let's", "lets",
];

/// The skill's function a line plainly asks for, if any: the line holds
/// exactly one ask ([`ASKS`]) and no word but those [`AROUND`].
pub fn asks_for(said: &str) -> Option<&'static str> {
    let words = lower_words(said);
    let mut asked = None;
    let mut at = 0;
    while at < words.len() {
        let found = ASKS.iter().find(|(ask, _)| {
            ask.iter()
                .enumerate()
                .all(|(next, word)| words.get(at + next).is_some_and(|said| said == word))
        });
        if let Some((ask, name)) = found {
            if asked.is_some() {
                return None;
            }
            asked = Some(*name);
            at += ask.len();
        } else if AROUND.contains(&words[at].as_str()) {
            at += 1;
        } else {
            return None;
        }
    }
    asked
}

/// Whether `use_skill` offers a skill now ([`offered_now`]).
pub type Offered = std::sync::Arc<dyn Fn(&str) -> bool + Send + Sync>;

/// The force the second route's chat is given with the switch on
/// (`second_provider::net::Chat::forcing`): a request whose last message is
/// a person's line that plainly asks for a scene or a show must call its
/// skill. `offered` says, at each request, whether `use_skill` offers a
/// skill: then `use_skill` is forced with its `skill` pinned to it, and
/// nothing when it is not offered (a skill turned off is never forced);
/// without `use_skill` (`None`), the skill's own function, which the chat
/// forces only when it is declared.
pub fn force(offered: Option<Offered>) -> Force {
    std::sync::Arc::new(move |messages: &[Message]| {
        let last = messages.last().filter(|message| message.role == "user")?;
        let skill = asks_for(last.words()?)?;
        match &offered {
            Some(offered) => offered(skill).then_some(Forced {
                name: crate::use_skill::NAME,
                only: Some(("skill", skill)),
            }),
            None => Some(Forced {
                name: skill,
                only: None,
            }),
        }
    })
}

/// Whether `use_skill` offers `skill` now, as the tools `settings` read
/// give it (`use_skill::offered`).
pub fn offered_now(settings: crate::tools::Settings) -> impl Fn(&str) -> bool + Send + Sync {
    move |skill| {
        crate::use_skill::offered(&crate::tools::skill_tools(&settings))
            .iter()
            .any(|tool| tool.name == skill)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// The played lines on lamp-52e6 (9 Nov) and the skills' own examples
    /// are forced to their skill; a line with any other word is not.
    #[test]
    fn a_plain_ask_for_a_scene_or_a_show_is_told_apart() {
        for (said, name) in [
            ("Lamp, movie light.", SCENE),
            ("Wait, movie light please.", SCENE),
            ("Movie light", SCENE),
            ("Lamp, reading light please.", SCENE),
            ("Hey lamp, can you give me a night light?", SCENE),
            ("Turn on the relaxing light.", SCENE),
            ("Lamp, show me a light show.", SHOW),
            ("Do a light show, please.", SHOW),
            ("Lamp, start a light show.", SHOW),
            ("Party lights!", SHOW),
            ("I'd like a light show.", SHOW),
        ] {
            assert_eq!(asks_for(said), Some(name), "{said}");
        }
        for said in [
            "Lamp, turn off the movie light.",
            "Don't do a light show.",
            "No movie light, thanks.",
            "What's a movie light?",
            "The movie light was too dim yesterday.",
            "I watched a light show last night.",
            "Movie light, and a light show.",
            "Lamp, light my book.",
            "Lamp, disco light.",
            "Lamp, set a scene.",
            "Lamp.",
            "",
        ] {
            assert_eq!(asks_for(said), None, "{said}");
        }
    }

    /// Through `use_skill`, the skill is pinned only when it is offered;
    /// without it, the skill's own function; a round after a call, or a
    /// lamp's message last, is asked as before.
    #[test]
    fn a_plain_ask_forces_its_skill_only_when_offered() {
        let movie = [Message::user("Lamp, movie light.")];
        let show = [Message::user("Lamp, show me a light show.")];
        let only_scenes: Offered = Arc::new(|skill| skill == SCENE);
        let wrapped = force(Some(only_scenes));
        assert_eq!(
            wrapped(&movie),
            Some(Forced {
                name: crate::use_skill::NAME,
                only: Some(("skill", SCENE))
            })
        );
        assert_eq!(wrapped(&show), None, "light show not offered");
        let own = force(None);
        assert_eq!(
            own(&show),
            Some(Forced {
                name: SHOW,
                only: None
            })
        );
        let mut after = movie.to_vec();
        after.push(Message::tool("1", "{\"next\":\"Say it.\"}"));
        assert_eq!(wrapped(&after), None);
        assert_eq!(own(&[Message::user("Lamp, light my book.")]), None);
    }
}
