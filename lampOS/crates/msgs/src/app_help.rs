//! An installed app's own help, for the voice (#7415; App Only's proposal,
//! issue comment 7032712984, and the first builder's yes, 7032733514).
//!
//! A format-2 manifest may carry `help`: one to three plain sentences on how
//! the app is used, by language (`{"en": "...", "vi": "..."}`), English
//! required, each at most [`MOST_BYTES`]. lamp-control tells the voice the
//! help of every installed app (`apps/help`, the newest kept), started or
//! not, so "how do I use the coach?" can be answered before it runs. The
//! voice reads it only with `LAMP_APP_HELP=on`, and only as a quoted note
//! that is data, never an instruction. Local to the lamp; no audio.
use crate::Invalid;
use lamp_bus::{Declaration, Kind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::Duration;

/// The most bytes of an app's help in one language (the ruling's "at most
/// about 300 bytes"; the Store's 1 to 300).
pub const MOST_BYTES: usize = 300;

/// The most languages an app's help may be in (the Store's 8).
pub const MOST_LANGUAGES: usize = 8;

/// The most lines of an app's help in one language (the Store's 3).
pub const MOST_LINES: usize = 3;

/// The language every app's help has.
pub const ENGLISH: &str = "en";

/// An app's name as a manifest gives it (`^[a-z][a-z0-9-]{0,23}$`).
fn name_ok(name: &str) -> bool {
    let mut chars = name.chars();
    name.len() <= 24
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// A language key as the Store takes it, BCP-47-ish and lower case: `en`,
/// `vi`, `pt-br`, `zh-hant` (2 or 3 letters, then up to two parts of 2 to 8
/// letters or digits). A key the lamp doesn't speak is carried, never read.
pub fn language_ok(key: &str) -> bool {
    let mut parts = key.split('-');
    let first_ok = parts.next().is_some_and(|part| {
        (2..=3).contains(&part.len()) && part.bytes().all(|b| b.is_ascii_lowercase())
    });
    let rest: Vec<&str> = parts.collect();
    first_ok
        && rest.len() <= 2
        && rest.iter().all(|part| {
            (2..=8).contains(&part.len())
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

/// The characters the Store refuses in help, the same set as #8068's
/// `tidy`: double quotes (`"` `“` `”`) and markup (`<>` `` ` `` `*` `[]`
/// `{}`). Apostrophes ("don't") pass.
fn refused(c: char) -> bool {
    matches!(
        c,
        '<' | '>' | '`' | '*' | '[' | ']' | '{' | '}' | '"' | '“' | '”'
    )
}

/// A link as the Store reads one: a word with `://`, or starting `www.`.
fn is_link(word: &str) -> bool {
    word.contains("://") || word.to_lowercase().starts_with("www.")
}

/// What the voice never tells, though the Store takes it: a backslash and
/// the zero-width and bidi format characters. Help holding one is left out
/// for the voice ([`tidy`] is `None`); the app still installs.
fn not_told(c: char) -> bool {
    c == '\\'
        || matches!(
            c,
            '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}'
        )
}

/// An app's help made safe to tell, as #8068 tidies an app's words: its
/// lines and runs of spaces become one line with single spaces, a link is
/// left out, and markup characters and double quotes go, so the help can
/// never close the quote it is told in. `None` when nothing is left, or
/// when it holds a backslash, a zero-width or a bidi character
/// ([`not_told`]): that help is not told at all. Never longer than what it
/// was given.
pub fn tidy(text: &str) -> Option<String> {
    if text.chars().any(not_told) {
        return None;
    }
    let words: Vec<String> = text
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|word| !is_link(word))
        .map(|word| word.chars().filter(|c| !refused(*c)).collect::<String>())
        .filter(|word| !word.is_empty())
        .collect();
    let line = words.join(" ");
    (!line.is_empty()).then_some(line)
}

/// A manifest's `help` as the lamp takes it, exactly the production Store's
/// rules (Backend3, #7415 comment 7033402561), so a Store-approved app
/// always installs: `en` required, at most [`MOST_LANGUAGES`] well-formed
/// keys ([`language_ok`]), each value 1 to [`MOST_BYTES`] bytes, at most
/// [`MOST_LINES`] lines, no other control character, and no quote, markup
/// or link. Empty is no help, and fine. The refusals are the Store's words.
pub fn help_ok(help: &BTreeMap<String, String>) -> Result<(), String> {
    if help.is_empty() {
        return Ok(());
    }
    if !help.contains_key(ENGLISH) {
        return Err("help: en is required when help is given".into());
    }
    if help.len() > MOST_LANGUAGES {
        return Err(format!("help: at most {MOST_LANGUAGES} languages"));
    }
    for (language, text) in help {
        if !language_ok(language) {
            return Err(format!(
                "help: {language:?} is not a language key (en, vi, pt-br)"
            ));
        }
        if text.is_empty() || text.len() > MOST_BYTES {
            return Err(format!("help.{language}: 1 to {MOST_BYTES} bytes"));
        }
        if text.split('\n').count() > MOST_LINES {
            return Err(format!("help.{language}: at most {MOST_LINES} lines"));
        }
        if text.chars().any(|c| c.is_control() && c != '\n') {
            return Err(format!("help.{language}: no control characters"));
        }
        if let Some(found) = text.chars().find(|c| refused(*c)) {
            return Err(format!(
                "help.{language}: no quotes, markup or links (found \"{found}\")"
            ));
        }
        if let Some(link) = text.split_whitespace().find(|word| is_link(word)) {
            let found: String = link.chars().take(32).collect();
            return Err(format!(
                "help.{language}: no quotes, markup or links (found \"{found}\")"
            ));
        }
    }
    Ok(())
}

/// The help of the apps installed now, the newest kept (`apps/help`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppHelps {
    pub apps: Vec<AppsHelp>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppsHelp {
    pub app: String,
    /// By language, English always there ([`help_ok`]), each tidied.
    pub help: BTreeMap<String, String>,
}

impl AppsHelp {
    /// The help in `language` when the app has it, else English.
    pub fn in_language(&self, language: &str) -> Option<(&str, &str)> {
        self.help
            .get_key_value(language)
            .or_else(|| self.help.get_key_value(ENGLISH))
            .map(|(language, text)| (language.as_str(), text.as_str()))
    }
}

impl AppHelps {
    pub const TYPE: &str = "lamp_msgs/AppHelps";
    pub const VERSION: u32 = 1;
    pub const TOPIC: &str = "apps/help";
    pub const MAX_APPS: usize = 32;
    /// The most bytes of the whole list as JSON, well within the bus's
    /// 60,000 (`lamp_bus::MOST_BYTES`): lamp-control leaves out the apps past
    /// it.
    pub const MOST_LIST_BYTES: usize = 48_000;

    /// Said by lamp-control every 5 s; older than 30 s is a dead
    /// publisher's, and the voice reads none.
    pub fn declaration() -> Declaration {
        Declaration::new(Self::TYPE, Self::VERSION, Kind::Latest)
            .every(Duration::from_secs(10))
            .lasting(Duration::from_secs(30))
    }

    pub fn check(&self) -> Result<(), Invalid> {
        if self.apps.len() > Self::MAX_APPS {
            return Err(Invalid("too many apps with help".into()));
        }
        for (index, app) in self.apps.iter().enumerate() {
            if !name_ok(&app.app) || self.apps[..index].iter().any(|seen| seen.app == app.app) {
                return Err(Invalid(
                    "an app's name is not a manifest name, or repeats".into(),
                ));
            }
            if app.help.is_empty() {
                return Err(Invalid("an app listed with no help".into()));
            }
            help_ok(&app.help).map_err(Invalid)?;
            // As told: tidy already, so the voice's quote holds.
            if app
                .help
                .values()
                .any(|text| tidy(text).as_deref() != Some(text.as_str()))
            {
                return Err(Invalid("an app's help is not tidied".into()));
            }
        }
        Ok(())
    }
}

pub fn app_help_topics() -> Vec<(&'static str, Declaration)> {
    vec![(AppHelps::TOPIC, AppHelps::declaration())]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn help(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(language, text)| ((*language).into(), (*text).into()))
            .collect()
    }

    #[test]
    fn tidy_leaves_out_quotes_markup_links_and_tells_no_bidi() {
        assert_eq!(
            tidy("Say \"stop\"  <b>now</b>\nsee https://x.io or www.x.io ok").as_deref(),
            Some("Say stop bnow/b see or ok")
        );
        assert_eq!(tidy("  \n https://x.io "), None);
        // A backslash, a zero-width or a bidi character: not told at all.
        assert_eq!(tidy("Stand side-on \u{202E}ok"), None);
        assert_eq!(tidy("Stand\u{200B} side-on"), None);
        assert_eq!(tidy("Stand side-on \\ ok"), None);
        let long = "a".repeat(MOST_BYTES);
        assert!(tidy(&long).unwrap().len() <= long.len());
    }

    /// Exactly the production Store's rules (#7415 comment 7033402561): what
    /// the Store takes, the lamp installs.
    #[test]
    fn help_takes_what_the_store_takes_and_refuses_what_it_refuses() {
        assert!(help_ok(&help(&[])).is_ok());
        assert!(help_ok(&help(&[("en", "Stand side-on.")])).is_ok());
        assert!(help_ok(&help(&[("en", "Stand side-on."), ("vi", "Đứng nghiêng.")])).is_ok());
        assert!(
            help_ok(&help(&[("en", "Don't stand too close.")])).is_ok(),
            "apostrophes pass"
        );
        assert_eq!(
            help_ok(&help(&[("vi", "Đứng nghiêng.")])),
            Err("help: en is required when help is given".into())
        );
        // Any well-formed key, a language the lamp doesn't speak included.
        assert!(help_ok(&help(&[("en", "ok"), ("pt-br", "Fique de lado.")])).is_ok());
        assert!(help_ok(&help(&[("en", "ok"), ("fr", "d'accord")])).is_ok());
        for bad in ["PT-BR", "e", "english", "pt_br", "pt-", "en-a"] {
            assert!(
                help_ok(&help(&[("en", "ok"), (bad, "x")])).is_err(),
                "{bad}"
            );
        }
        let eight = ["en", "vi", "ja", "zh", "ko", "pt-br", "fr", "de"];
        let pairs: Vec<(&str, &str)> = eight.iter().map(|key| (*key, "ok")).collect();
        assert!(help_ok(&help(&pairs)).is_ok(), "8 languages");
        let mut nine = pairs.clone();
        nine.push(("es", "ok"));
        assert!(help_ok(&help(&nine)).is_err(), "9 languages");
        // Three lines pass, four do not; no other control character.
        assert!(help_ok(&help(&[("en", "one\ntwo\nthree")])).is_ok());
        assert!(help_ok(&help(&[("en", "one\ntwo\nthree\nfour")])).is_err());
        assert!(help_ok(&help(&[("en", "one\ttwo")])).is_err());
        assert!(help_ok(&help(&[("en", "one\r\ntwo")])).is_err());
        assert!(help_ok(&help(&[("en", &"a".repeat(MOST_BYTES))])).is_ok());
        assert!(help_ok(&help(&[("en", &"a".repeat(MOST_BYTES + 1))])).is_err());
        assert!(help_ok(&help(&[("en", "")])).is_err());
        // Vietnamese counts bytes, not characters: 101 of a 3-byte letter.
        assert!(help_ok(&help(&[("en", "ok"), ("vi", &"ệ".repeat(101))])).is_err());
        assert_eq!(
            help_ok(&help(&[("en", "Say <b>stop</b>")])),
            Err("help.en: no quotes, markup or links (found \"<\")".into())
        );
        for bad in [
            "say \"stop\"",
            "say “stop”",
            "see https://x.io",
            "see www.x.io",
            "a `b`",
            "{x}",
            "*x*",
            "[x]",
        ] {
            assert!(help_ok(&help(&[("en", bad)])).is_err(), "{bad}");
        }
        // The Store takes these; the voice just does not tell them.
        assert!(help_ok(&help(&[("en", "Stand side-on \u{202E}ok")])).is_ok());
        assert!(help_ok(&help(&[("en", "a \\ b")])).is_ok());
    }

    #[test]
    fn the_list_is_checked_tidied_and_picks_a_language_else_english() {
        let ok = AppHelps {
            apps: vec![AppsHelp {
                app: "gym-coach".into(),
                help: help(&[("en", "Stand side-on."), ("vi", "Đứng nghiêng.")]),
            }],
        };
        assert!(ok.check().is_ok());
        let json = serde_json::to_string(&ok).unwrap();
        assert_eq!(serde_json::from_str::<AppHelps>(&json).unwrap(), ok);
        assert_eq!(ok.apps[0].in_language("vi"), Some(("vi", "Đứng nghiêng.")));
        assert_eq!(ok.apps[0].in_language("ja"), Some(("en", "Stand side-on.")));
        assert_eq!(ok.apps[0].in_language("en"), Some(("en", "Stand side-on.")));
        for bad in [
            vec![AppsHelp {
                app: "Bad".into(),
                help: help(&[("en", "ok")]),
            }],
            vec![AppsHelp {
                app: "a".into(),
                help: help(&[]),
            }],
            vec![AppsHelp {
                app: "a".into(),
                help: help(&[("en", "say \"x\"")]),
            }],
            vec![
                AppsHelp {
                    app: "a".into(),
                    help: help(&[("en", "ok")]),
                },
                AppsHelp {
                    app: "a".into(),
                    help: help(&[("en", "ok")]),
                },
            ],
        ] {
            assert!(AppHelps { apps: bad.clone() }.check().is_err(), "{bad:?}");
        }
        assert!(
            serde_json::from_str::<AppHelps>(r#"{"apps":[],"extra":1}"#).is_err(),
            "unknown fields are refused"
        );
    }
}
