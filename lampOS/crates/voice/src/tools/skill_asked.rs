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
//!
//! A line that is only a plain question about the lamp itself ("who made
//! you?", "where can I buy you?", "how much do you cost?") calls the lamp's
//! help skill (faq's `lamp_help`) the same way. On build 11756 on 11 Nov
//! (#6216's row) both were answered with no call, though faq was offered
//! through `use_skill` with a line naming those very questions: "who made
//! you?" was the model's own words, and "where can I buy you?" said
//! "autonomous.ai" without the page faq's `about` names. The topic is still
//! the model's. A question with any other word ("who made you sad", "where
//! can I buy milk") is the model's, as before. The same questions asked in
//! Vietnamese ("Ai làm ra bạn?", "Mua bạn ở đâu?", "Bạn giá bao nhiêu?")
//! are forced too, with diacritics as speech-to-text writes them; "ai làm
//! bạn buồn" (who made you sad) and "mua sữa ở đâu" (where to buy milk)
//! are the model's.
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
/// The lamp's help skill's function (faq's `tool`): the name `use_skill`
/// gives it, as it gives light-scenes' as `light_scene`.
pub const HELP: &str = "lamp_help";

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

/// The plain questions about the lamp itself, each whole, and the help
/// skill each calls: who made it, where it is sold, what it costs (faq's
/// `about`, and its SKILL.md's "who made you?", "where can I buy you?").
/// Only [`AROUND`] words may stand beside one, and no request: "do you know
/// who made you" or "who made you sad" is the model's.
// ponytail: questions named here, not read from the skill, as [`ASKS`].
const QUESTIONS: &[(&[&str], &str)] = &[
    (&["who", "made", "you"], HELP),
    (&["who", "makes", "you"], HELP),
    (&["who", "built", "you"], HELP),
    (&["who", "created", "you"], HELP),
    (&["what", "company", "made", "you"], HELP),
    (&["which", "company", "made", "you"], HELP),
    (&["where", "can", "i", "buy", "you"], HELP),
    (&["where", "can", "i", "get", "you"], HELP),
    (&["where", "do", "i", "buy", "you"], HELP),
    (&["where", "can", "i", "buy", "one"], HELP),
    (
        &["where", "can", "i", "buy", "a", "lamp", "like", "you"],
        HELP,
    ),
    (&["how", "much", "do", "you", "cost"], HELP),
    (&["how", "much", "are", "you"], HELP),
    (&["what", "do", "you", "cost"], HELP),
    (&["what", "is", "your", "price"], HELP),
    (&["what's", "your", "price"], HELP),
    (&["whats", "your", "price"], HELP),
    // The same in Vietnamese: "ra" whole, since "ai làm bạn …" is "who
    // makes you …" (sad, a friend); a longer question before its start.
    (&["ai", "làm", "ra", "bạn"], HELP),
    (&["ai", "tạo", "ra", "bạn"], HELP),
    (&["ai", "đã", "làm", "ra", "bạn"], HELP),
    (&["ai", "đã", "tạo", "ra", "bạn"], HELP),
    (&["ai", "sản", "xuất", "ra", "bạn"], HELP),
    (&["công", "ty", "nào", "làm", "ra", "bạn"], HELP),
    (&["công", "ty", "nào", "tạo", "ra", "bạn"], HELP),
    (&["công", "ty", "nào", "sản", "xuất", "ra", "bạn"], HELP),
    (&["hãng", "nào", "làm", "ra", "bạn"], HELP),
    (&["mua", "bạn", "ở", "đâu"], HELP),
    (&["có", "thể", "mua", "bạn", "ở", "đâu"], HELP),
    (&["tôi", "có", "thể", "mua", "bạn", "ở", "đâu"], HELP),
    (&["mình", "có", "thể", "mua", "bạn", "ở", "đâu"], HELP),
    (&["tôi", "mua", "bạn", "ở", "đâu"], HELP),
    (&["mình", "mua", "bạn", "ở", "đâu"], HELP),
    (&["bạn", "giá", "bao", "nhiêu", "tiền"], HELP),
    (&["bạn", "giá", "bao", "nhiêu"], HELP),
    (&["bạn", "bao", "nhiêu", "tiền"], HELP),
    (&["giá", "của", "bạn", "là", "bao", "nhiêu"], HELP),
    (&["giá", "bạn", "bao", "nhiêu"], HELP),
];

/// Words a Vietnamese question about the lamp may hold anywhere besides
/// [`AROUND`]: a call ("ơi", "đèn ơi"), a greeting, a thanks, and the
/// endings a plain question takes ("vậy", "thế", "ạ", "nhỉ"). Only for
/// [`QUESTIONS`]: the scenes' and shows' asks are as before.
const AROUND_VI: &[&[&str]] = &[
    &["ơi"],
    &["đèn"],
    &["chào"],
    &["xin", "chào"],
    &["vậy"],
    &["thế"],
    &["ạ"],
    &["nhỉ"],
    &["cảm", "ơn"],
    &["cám", "ơn"],
];

/// Words the line may hold anywhere: a greeting, the lamp's name, a
/// "please", a "thanks", an article. Any word not here, in a request
/// ([`REQUESTS`]) or in the ask ("off", "don't", "no", "what", "later", a
/// story) leaves the line to the model: a missed force only means the model
/// answers as without the switch, while a wrong one changes the light nobody
/// asked to change.
pub(crate) const AROUND: &[&[&str]] = &[
    &["hey"],
    &["hi"],
    &["hello"],
    &["ok"],
    &["okay"],
    &["lamp"],
    &["so"],
    &["oh"],
    &["wait"],
    &["please"],
    &["now"],
    &["thanks"],
    &["thank", "you"],
    &["a"],
    &["an"],
    &["the"],
    &["my"],
    &["some"],
];

/// The request constructions, whole, and only before the ask: "can you",
/// "give me", "turn on", "show me", "do", "I'd like", "I want". "I", "like",
/// "you" and "do" alone are not one, so a preference ("I like movie light")
/// or a question ("do you like movie light?", "would you like a light
/// show?") is the model's.
pub(crate) const REQUESTS: &[&[&str]] = &[
    &["can", "you"],
    &["could", "you"],
    &["would", "you"],
    &["will", "you"],
    &["give", "me"],
    &["give", "us"],
    &["show", "me"],
    &["show", "us"],
    &["set"],
    &["put", "on"],
    &["turn", "on"],
    &["switch", "on"],
    &["do"],
    &["start"],
    &["play"],
    &["make", "it"],
    &["let's"],
    &["lets"],
    &["i'd", "like"],
    &["id", "like"],
    &["i", "would", "like"],
    &["i", "want"],
    &["i", "need"],
];

/// How many words of `words` from `at` the first of `phrases` takes, if one.
pub(crate) fn phrase_at(words: &[String], at: usize, phrases: &[&[&str]]) -> Option<usize> {
    phrases
        .iter()
        .find(|phrase| {
            phrase
                .iter()
                .enumerate()
                .all(|(next, word)| words.get(at + next).is_some_and(|said| said == word))
        })
        .map(|phrase| phrase.len())
}

/// The skill's function a line plainly asks for, if any: the line holds
/// exactly one ask ([`ASKS`]), requests ([`REQUESTS`]) only before it, and
/// no word but those [`AROUND`].
pub fn asks_for(said: &str) -> Option<&'static str> {
    plainly(said, ASKS, REQUESTS, &[])
}

/// The help skill's function a line plainly asks a question about the lamp
/// itself for, if any: the line holds exactly one whole question
/// ([`QUESTIONS`]) and no word but those [`AROUND`] or [`AROUND_VI`].
pub fn asks_about(said: &str) -> Option<&'static str> {
    plainly(said, QUESTIONS, &[], AROUND_VI)
}

/// The function of the one entry of `table` that `said` holds, with
/// `requests` only before it and no word but those [`AROUND`] or `also`.
fn plainly(
    said: &str,
    table: &[(&[&str], &'static str)],
    requests: &[&[&str]],
    also: &[&[&str]],
) -> Option<&'static str> {
    let words = lower_words(said);
    let mut asked = None;
    let mut at = 0;
    while at < words.len() {
        let found = table
            .iter()
            .find(|(ask, _)| phrase_at(&words, at, &[ask]).is_some());
        if let Some((ask, name)) = found {
            if asked.is_some() {
                return None;
            }
            asked = Some(*name);
            at += ask.len();
        } else if let Some(taken) = phrase_at(&words, at, AROUND)
            .or_else(|| phrase_at(&words, at, also))
            .or_else(|| {
                asked
                    .is_none()
                    .then(|| phrase_at(&words, at, requests))
                    .flatten()
            })
        {
            at += taken;
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
/// a person's line that plainly asks for a scene or a show, or plainly
/// asks a question about the lamp itself, must call its skill. `offered`
/// says, at each request, whether `use_skill` offers a skill: then
/// `use_skill` is forced with its `skill` pinned to it, and nothing when it
/// is not offered (a skill turned off is never forced); without `use_skill`
/// (`None`), the skill's own function, which the chat forces only when it
/// is declared.
pub fn force(offered: Option<Offered>) -> Force {
    std::sync::Arc::new(move |messages: &[Message]| {
        let last = messages.last().filter(|message| message.role == "user")?;
        let said = last.words()?;
        let skill = asks_for(said).or_else(|| asks_about(said))?;
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

    #[test]
    fn reviewer_three_preferences_and_questions_do_not_force_skills() {
        let forced = force(Some(Arc::new(|_| true)));
        let mut wrongly_forced = Vec::new();
        for said in [
            "Lamp, do you like movie light?",
            "Lamp, I like movie light.",
            "Lamp, I like the light show.",
        ] {
            if let Some(call) = forced(&[Message::user(said)]) {
                wrongly_forced.push((said, call));
            }
        }
        assert!(
            wrongly_forced.is_empty(),
            "non-command lines forced calls: {wrongly_forced:?}"
        );
    }

    /// A preference or a question about a scene or a show is the model's;
    /// only a request construction before the ask forces.
    #[test]
    fn a_preference_or_a_question_is_not_an_ask() {
        for said in [
            "Do you like the night light?",
            "Do you want a light show?",
            "Would you like a movie light?",
            "Can you like a light show?",
            "I like movie light.",
            "I love the light show.",
            "I want to like movie light.",
            "Is that a light show?",
            "Movie light, do you like it?",
            "Movie light, I like.",
            "Lamp, you movie light.",
        ] {
            assert_eq!(asks_for(said), None, "{said}");
        }
        for (said, name) in [
            ("I want a movie light, please.", SCENE),
            ("I would like a light show.", SHOW),
            ("Can you do a light show?", SHOW),
            ("Could you please put on the reading light?", SCENE),
            ("Let's do a light show.", SHOW),
        ] {
            assert_eq!(asks_for(said), Some(name), "{said}");
        }
    }

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

    /// Each plain question about the lamp itself (build 11756, 11 Nov:
    /// "who made you?" and "where can I buy you?" called nothing) asks the
    /// help skill; a light ask never does.
    #[test]
    fn a_plain_question_about_the_lamp_asks_its_help() {
        for said in [
            "Hey Lamp, who made you?",
            "Who makes you?",
            "Lamp, who built you?",
            "Who created you, lamp?",
            "What company made you?",
            "Which company made you?",
            "Hey Lamp, where can I buy you?",
            "Where can I get you?",
            "Where do I buy you?",
            "Where can I buy one, please?",
            "Where can I buy a lamp like you?",
            "How much do you cost?",
            "Lamp, how much are you?",
            "What do you cost?",
            "What is your price?",
            "What's your price?",
            "What\u{2019}s your price?",
            "Whats your price",
            "Okay lamp, who made you? Thanks.",
        ] {
            assert_eq!(asks_about(said), Some(HELP), "{said}");
            assert_eq!(asks_for(said), None, "{said}");
        }
        for said in [
            "Where can I buy milk?",
            "Who made you cry?",
            "Do you know who made you sad?",
            "Who made you sad?",
            "Can you tell me who made you?",
            "Who made you, and where can I buy you?",
            "Where can I buy a lamp?",
            "How much do you cost in Vietnam?",
            "Who made this?",
            "Lamp, movie light.",
            "Lamp.",
            "",
        ] {
            assert_eq!(asks_about(said), None, "{said}");
        }
        for (said, name) in [
            ("Lamp, movie light.", SCENE),
            ("Lamp, show me a light show.", SHOW),
        ] {
            assert_eq!(asks_for(said), Some(name), "{said}");
        }
    }

    /// The same questions asked in Vietnamese (the testers' lines) ask the
    /// help skill; one with any other word, or two, is the model's, and a
    /// Vietnamese call or ending does not make a light ask.
    #[test]
    fn a_plain_question_about_the_lamp_in_vietnamese_asks_its_help() {
        for said in [
            "Ai làm ra bạn?",
            "Ai tạo ra bạn?",
            "Công ty nào làm ra bạn?",
            "Mua bạn ở đâu?",
            "Bạn giá bao nhiêu?",
            "Lamp ơi, ai làm ra bạn vậy?",
            "Đèn ơi, ai đã tạo ra bạn thế?",
            "Ai sản xuất ra bạn?",
            "Công ty nào sản xuất ra bạn?",
            "Hãng nào làm ra bạn?",
            "Tôi có thể mua bạn ở đâu?",
            "Mình mua bạn ở đâu nhỉ?",
            "Có thể mua bạn ở đâu ạ?",
            "Bạn giá bao nhiêu tiền?",
            "Bạn bao nhiêu tiền?",
            "Giá của bạn là bao nhiêu?",
            "Xin chào, giá bạn bao nhiêu? Cảm ơn.",
            "AI LÀM RA BẠN",
        ] {
            assert_eq!(asks_about(said), Some(HELP), "{said}");
            assert_eq!(asks_for(said), None, "{said}");
        }
        for said in [
            "Ai làm bạn buồn?",
            "Ai làm ra bạn buồn vậy?",
            "Ai làm bạn với tôi?",
            "Mua sữa ở đâu?",
            "Bạn mua sữa ở đâu?",
            "Bạn mua ở đâu?",
            "Bạn có biết ai làm ra bạn không?",
            "Ai làm ra bạn? Mua bạn ở đâu?",
            "Ai làm ra bạn và mua bạn ở đâu?",
            "Bạn giá bao nhiêu ở Việt Nam?",
            "Bạn bao nhiêu tuổi?",
            "Cái này giá bao nhiêu?",
            "Ai làm ra cái này?",
            "Mua bạn ở đâu rẻ nhất?",
            "Đèn ơi.",
            "Lamp ơi, movie light.",
        ] {
            assert_eq!(asks_about(said), None, "{said}");
        }
        assert_eq!(asks_for("Lamp ơi, movie light."), None);
    }

    /// The help skill is forced through `use_skill`, pinned to `lamp_help`,
    /// only when offered; without `use_skill`, its own function.
    #[test]
    fn a_plain_question_about_the_lamp_forces_its_help_only_when_offered() {
        let who = [Message::user("Hey Lamp, who made you?")];
        let buy = [Message::user("Hey Lamp, where can I buy you?")];
        let every: Offered = Arc::new(|_| true);
        let wrapped = force(Some(every));
        for line in [&who, &buy] {
            assert_eq!(
                wrapped(line),
                Some(Forced {
                    name: crate::use_skill::NAME,
                    only: Some(("skill", HELP))
                })
            );
        }
        let only_scenes: Offered = Arc::new(|skill| skill == SCENE);
        assert_eq!(force(Some(only_scenes))(&who), None, "faq not offered");
        assert_eq!(
            force(None)(&buy),
            Some(Forced {
                name: HELP,
                only: None
            })
        );
        let mut after = who.to_vec();
        after.push(Message::tool("1", "{\"next\":\"Say it.\"}"));
        assert_eq!(wrapped(&after), None);
        assert_eq!(wrapped(&[Message::user("Where can I buy milk?")]), None);
        for said in ["Ai làm ra bạn?", "Mua bạn ở đâu?", "Bạn giá bao nhiêu?"] {
            let line = [Message::user(said)];
            assert_eq!(
                wrapped(&line),
                Some(Forced {
                    name: crate::use_skill::NAME,
                    only: Some(("skill", HELP))
                }),
                "{said}"
            );
            let only_scenes: Offered = Arc::new(|skill| skill == SCENE);
            assert_eq!(force(Some(only_scenes))(&line), None, "{said}");
        }
        assert_eq!(wrapped(&[Message::user("Mua sữa ở đâu?")]), None);
        assert_eq!(wrapped(&[Message::user("Ai làm bạn buồn?")]), None);
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
