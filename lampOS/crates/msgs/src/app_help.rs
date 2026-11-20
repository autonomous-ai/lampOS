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
/// about 300 bytes").
pub const MOST_BYTES: usize = 300;

/// The languages an app's help may be in: the lamp's own (`lamp.language`,
/// `lamp_voice::language`). English is required.
pub const LANGUAGES: [&str; 5] = ["en", "vi", "ja", "zh", "ko"];

/// The language every app's help has.
pub const ENGLISH: &str = "en";

/// An app's name as a manifest gives it (`^[a-z][a-z0-9-]{0,23}$`).
fn name_ok(name: &str) -> bool {
    let mut chars = name.chars();
    name.len() <= 24
        && chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Whether `c` could close a quote, start markup or reorder what is shown:
/// double quotes (`"` `“` `”`), markup (`<>` `` ` `` `*` `[]` `{}`), a
/// backslash, and the zero-width and bidi format characters.
fn left_out(c: char) -> bool {
    matches!(
        c,
        '<' | '>' | '`' | '*' | '[' | ']' | '{' | '}' | '"' | '“' | '”' | '\\'
    ) || matches!(
        c,
        '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}'
    )
}

/// An app's help made safe to tell, as #8068 tidies an app's words: control
/// characters and runs of spaces become one space, a link (a word with
/// `://`, or starting `www.`) is left out, and markup characters, double
/// quotes, backslashes and bidi characters go ([`left_out`]), so the help
/// can never close the quote it is told in. `None` when nothing is left.
/// Never longer than what it was given.
pub fn tidy(text: &str) -> Option<String> {
    let words: Vec<String> = text
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|word| !word.contains("://") && !word.to_lowercase().starts_with("www."))
        .map(|word| word.chars().filter(|c| !left_out(*c)).collect::<String>())
        .filter(|word| !word.is_empty())
        .collect();
    let line = words.join(" ");
    (!line.is_empty()).then_some(line)
}

/// A manifest's `help` as the lamp takes it: English there, only the lamp's
/// languages, each 1 to [`MOST_BYTES`] bytes with no control character, and
/// something left once tidied. Empty is no help, and fine.
pub fn help_ok(help: &BTreeMap<String, String>) -> Result<(), String> {
    if help.is_empty() {
        return Ok(());
    }
    if !help.contains_key(ENGLISH) {
        return Err("help needs English (en)".into());
    }
    for (language, text) in help {
        if !LANGUAGES.contains(&language.as_str()) {
            return Err(format!(
                "help in {language:?} is not one of the lamp's languages: {}",
                LANGUAGES.join(", ")
            ));
        }
        if text.is_empty() || text.len() > MOST_BYTES || text.chars().any(char::is_control) {
            return Err(format!(
                "help in {language} is 1 to {MOST_BYTES} bytes on one line"
            ));
        }
        if tidy(text).is_none() {
            return Err(format!("help in {language} has no words once tidied"));
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
    fn tidy_leaves_out_quotes_markup_links_and_bidi() {
        assert_eq!(
            tidy("Say \"stop\"  <b>now</b> see https://x.io or www.x.io \u{202E}ok\\").as_deref(),
            Some("Say stop bnow/b see or ok")
        );
        assert_eq!(tidy("  \n https://x.io "), None);
        let long = "a".repeat(MOST_BYTES);
        assert!(tidy(&long).unwrap().len() <= long.len());
    }

    #[test]
    fn help_needs_english_and_the_lamps_languages_within_the_bound() {
        assert!(help_ok(&help(&[])).is_ok());
        assert!(help_ok(&help(&[("en", "Stand side-on.")])).is_ok());
        assert!(help_ok(&help(&[("en", "Stand side-on."), ("vi", "Đứng nghiêng.")])).is_ok());
        assert!(
            help_ok(&help(&[("vi", "Đứng nghiêng.")])).is_err(),
            "no English"
        );
        assert!(help_ok(&help(&[("en", "x y z"), ("fr", "x y z")])).is_err());
        assert!(help_ok(&help(&[("en", &"a".repeat(MOST_BYTES))])).is_ok());
        assert!(help_ok(&help(&[("en", &"a".repeat(MOST_BYTES + 1))])).is_err());
        // Vietnamese counts bytes, not characters: 101 of a 3-byte letter.
        assert!(help_ok(&help(&[("en", "ok"), ("vi", &"ệ".repeat(101))])).is_err());
        assert!(help_ok(&help(&[("en", "one\ntwo")])).is_err());
        assert!(help_ok(&help(&[("en", "\"<>\"")])).is_err(), "nothing left");
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
