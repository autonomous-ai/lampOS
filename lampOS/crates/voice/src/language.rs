//! The owner's language (#4473 (a), the voice side): English, Vietnamese,
//! Japanese, Chinese or Korean, from one line in the file [`FILE_ENVIRONMENT`] names
//! (the product's `/etc/lampOS/setup/language`). A missing file, an empty
//! one, `en` or a value this build does not know (said in the journal)
//! is English, and nothing below changes: the speech-to-text's language, the
//! character, the voice and its model stay as they are today.
//!
//! For `vi`, `ja`, `zh` or `ko`:
//! - speech to text is told the language (over `stt_language`), and Flux,
//!   English only, is left for Nova-3 by its own rule;
//! - the character's "Always speak English" becomes the chosen language's
//!   ([`instruction`]); the call name stays "Lamp";
//! - ElevenLabs speaks with [`SPEECH_MODEL`], which speaks all four, in the
//!   language's voice from V1's table ([`Language::voice`]);
//! - the English-only gates follow it: a cut-in with no English word is no
//!   fragment, a turn heard in another script is asked again (#926) only in
//!   a Latin-script language (Vietnamese), and the stop stream listens only
//!   in a language it has words for (Vietnamese, #2939), in that language;
//! - at start, a language other than the last one announced is said once in
//!   its own words ([`announce`]), and recorded in [`SAID_PATH`].
//!
//! Read at every voice start: the processes read it as they start, so a
//! change applies at the voice's next start.
use std::{ffi::OsString, fs, io::Read, path::Path};

/// The absolute path of the language file, and nothing else.
pub const FILE_ENVIRONMENT: &str = "LAMP_LANGUAGE_FILE";

/// The language last announced, one code a line; absent is English.
pub const SAID_PATH: &str = "/var/lib/lampOS/voice/language-said";

/// The ElevenLabs model for every language but English: it speaks ja, zh
/// and ko (`docs/second-voice.md`, "The owner's language").
pub const SPEECH_MODEL: &str = "eleven_flash_v2_5";

/// The longest file taken: a code and a newline, with room to spare.
const FILE_MOST: u64 = 64;

/// The character's own rule this replaces, in both built-in characters and
/// `deploy/character/lamp.txt`.
const ENGLISH_RULE: &str = "Always speak English";

/// The words said when the lamp goes back to English.
pub const ENGLISH_CHANGED: &str = "Okay, I'll speak English from now on.";

/// A language other than English.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Vietnamese,
    Japanese,
    Chinese,
    Korean,
}

impl Language {
    /// The setting's text: `None` for English (`en`, empty), or why not.
    pub fn parse(text: &str) -> Result<Option<Self>, String> {
        match text.trim() {
            "" | "en" => Ok(None),
            "vi" => Ok(Some(Self::Vietnamese)),
            "ja" => Ok(Some(Self::Japanese)),
            "zh" => Ok(Some(Self::Chinese)),
            "ko" => Ok(Some(Self::Korean)),
            other => Err(format!(
                "the language {:?} is not one this lamp speaks (en, vi, ja, zh, ko): English",
                other.chars().take(16).collect::<String>()
            )),
        }
    }

    /// The setting's own code.
    pub fn code(self) -> &'static str {
        match self {
            Self::Vietnamese => "vi",
            Self::Japanese => "ja",
            Self::Chinese => "zh",
            Self::Korean => "ko",
        }
    }

    /// Deepgram's `language` for it.
    pub fn stt(self) -> &'static str {
        self.code()
    }

    /// The character's rule, as [`ENGLISH_RULE`] is worded.
    fn rule(self) -> &'static str {
        match self {
            Self::Vietnamese => "Always speak Vietnamese",
            Self::Japanese => "Always speak Japanese",
            Self::Chinese => "Always speak Mandarin Chinese, in simplified characters",
            Self::Korean => "Always speak Korean",
        }
    }

    /// V1's first voice for it (`system/domain/device.go`'s
    /// `elevenLabsVoicesByLang`), by its name in `net::tts`'s table. V1 had
    /// no Korean voice: Korean keeps the lamp's own.
    pub fn voice(self) -> Option<&'static str> {
        match self {
            Self::Vietnamese => Some("Ngan"),
            Self::Japanese => Some("Shizuka"),
            Self::Chinese => Some("Amy"),
            Self::Korean => None,
        }
    }

    /// Whether the stop stream has words for it: #2939's Vietnamese
    /// backchannels. Without them it is not listened to.
    pub fn has_stop_words(self) -> bool {
        self == Self::Vietnamese
    }

    /// Whether it is written in the Latin script, as English is: #926's
    /// check for a turn heard in another script still holds for it.
    pub fn latin_script(self) -> bool {
        self == Self::Vietnamese
    }

    /// Said once when the lamp starts speaking it.
    pub fn changed(self) -> &'static str {
        match self {
            Self::Vietnamese => "Từ giờ mình sẽ nói tiếng Việt nhé.",
            Self::Japanese => "これからは日本語で話しますね。",
            Self::Chinese => "好的，从现在开始我说中文。",
            Self::Korean => "이제부터 한국어로 말할게요.",
        }
    }
}

/// The language in `text`; an unknown value is said and taken as English.
fn from_text(text: &str) -> Option<Language> {
    Language::parse(text).unwrap_or_else(|why| {
        eprintln!("lamp-voice: {why}");
        None
    })
}

/// The language the file at `path` holds; missing or unreadable is English.
pub fn read(path: &Path) -> Option<Language> {
    let mut text = String::new();
    match fs::File::open(path).and_then(|file| file.take(FILE_MOST).read_to_string(&mut text)) {
        Ok(_) => from_text(&text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            eprintln!("lamp-voice: the language file could not be read ({error}): English");
            None
        }
    }
}

/// The language the file `var` names holds; none named is English.
pub fn from_var(var: impl Fn(&str) -> Option<OsString>) -> Option<Language> {
    var(FILE_ENVIRONMENT)
        .filter(|path| Path::new(path).is_absolute())
        .and_then(|path| read(Path::new(&path)))
}

/// [`from_var`] from this process's environment.
pub fn from_environment() -> Option<Language> {
    from_var(|name| std::env::var_os(name))
}

/// [`from_environment`], read once for this process: for checks made on
/// every turn.
pub fn at_start() -> Option<Language> {
    static CHOSEN: std::sync::OnceLock<Option<Language>> = std::sync::OnceLock::new();
    *CHOSEN.get_or_init(from_environment)
}

/// The character told to speak `language`; English leaves it as it is.
pub fn instruction(character: String, language: Option<Language>) -> String {
    let Some(language) = language else {
        return character;
    };
    if character.contains(ENGLISH_RULE) {
        character.replacen(ENGLISH_RULE, language.rule(), 1)
    } else {
        format!(
            "{character}\n\n{}, whatever language you hear.",
            language.rule()
        )
    }
}

/// The code and the words owed when `chosen` is not the language last
/// announced (`said`, absent is English): none when they agree.
fn owed(chosen: Option<Language>, said: Option<&str>) -> Option<(&'static str, &'static str)> {
    let code = chosen.map_or("en", Language::code);
    let said = said.map(str::trim).filter(|said| !said.is_empty());
    if said.unwrap_or("en") == code {
        return None;
    }
    Some((code, chosen.map_or(ENGLISH_CHANGED, Language::changed)))
}

/// The words owed at start with the record at `said`.
fn owed_at(chosen: Option<Language>, said: &Path) -> Option<(&'static str, &'static str)> {
    owed(chosen, fs::read_to_string(said).ok().as_deref())
}

/// `code` recorded as announced, replacing the record whole.
fn record(said: &Path, code: &str) -> std::io::Result<()> {
    let placing = said.with_extension("placing");
    fs::write(&placing, format!("{code}\n"))?;
    fs::rename(&placing, said)
}

const SKILL: &str = "language";
/// The owner just chose it in the app: said after a short quiet, as a
/// reminder is.
const PRIORITY: u8 = crate::say::URGENT_FROM;
const EXPIRES_MS: u64 = 600_000;

/// Says the change once, through the say service, when the file
/// [`FILE_ENVIRONMENT`] names holds a language other than the one last
/// announced; it is recorded only once spoken, so a change not said is
/// said at the next start. An English lamp never changed has nothing owed.
pub fn announce(asks: crate::say::OwnAsks) {
    let said = Path::new(SAID_PATH);
    let Some((code, words)) = owed_at(from_environment(), said) else {
        return;
    };
    let answered = asks.text(SKILL, words, PRIORITY, EXPIRES_MS);
    let _ = std::thread::Builder::new()
        .name("lamp-voice-language".into())
        .spawn(move || {
            let outcome = loop {
                match answered.recv() {
                    Ok(crate::say::Answer::Done(outcome)) => break outcome,
                    Ok(crate::say::Answer::Begun(_)) => {}
                    Err(_) => {
                        break crate::say::Outcome::Dropped("the voice has gone".into());
                    }
                }
            };
            if outcome == crate::say::Outcome::Spoken
                && let Err(error) = record(Path::new(SAID_PATH), code)
            {
                eprintln!("lamp-voice: the language {code} was said but not recorded: {error}");
            }
            eprintln!(
                "lamp-voice: the language {code} announced: {}",
                outcome.line()
            );
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lamp-language-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn chosen(dir: &Path, text: Option<&str>) -> Option<Language> {
        let file = dir.join("language");
        let _ = fs::remove_file(&file);
        if let Some(text) = text {
            fs::write(&file, text).unwrap();
        }
        from_var(|name| (name == FILE_ENVIRONMENT).then(|| file.clone().into()))
    }

    #[test]
    fn english_an_absent_or_empty_file_and_an_unknown_value_are_english() {
        let dir = scratch("english");
        for text in [
            None,
            Some(""),
            Some("en\n"),
            Some("  "),
            Some("fr"),
            Some("JA"),
        ] {
            assert_eq!(chosen(&dir, text), None, "{text:?}");
        }
        assert_eq!(from_var(|_| None), None);
        // Only an absolute path is read.
        assert_eq!(from_var(|_| Some("language".into())), None);
    }

    #[test]
    fn vi_ja_zh_and_ko_are_read() {
        let dir = scratch("read");
        assert_eq!(chosen(&dir, Some("vi\n")), Some(Language::Vietnamese));
        assert_eq!(chosen(&dir, Some("ja\n")), Some(Language::Japanese));
        assert_eq!(chosen(&dir, Some("zh")), Some(Language::Chinese));
        assert_eq!(chosen(&dir, Some(" ko ")), Some(Language::Korean));
    }

    #[test]
    fn english_leaves_the_character_byte_for_byte() {
        for character in [
            crate::config::CHAT_INSTRUCTION,
            crate::config::CAMERA_CHAT_INSTRUCTION,
            include_str!("../../../deploy/character/lamp.txt"),
        ] {
            assert_eq!(instruction(character.to_owned(), None), character);
        }
    }

    #[test]
    fn each_language_replaces_the_english_rule() {
        for (language, rule) in [
            (
                Language::Vietnamese,
                "Always speak Vietnamese, whatever language you hear",
            ),
            (
                Language::Japanese,
                "Always speak Japanese, whatever language you hear",
            ),
            (
                Language::Chinese,
                "Always speak Mandarin Chinese, in simplified characters, whatever language you hear",
            ),
            (
                Language::Korean,
                "Always speak Korean, whatever language you hear",
            ),
        ] {
            for character in [
                crate::config::CHAT_INSTRUCTION,
                include_str!("../../../deploy/character/lamp.txt"),
            ] {
                let told = instruction(character.to_owned(), Some(language));
                assert!(told.contains(rule), "{language:?}");
                assert!(!told.contains(ENGLISH_RULE), "{language:?}");
                assert!(told.starts_with("You are Lamp"), "{language:?}");
            }
            // A character without the rule is told it at its end.
            assert!(
                instruction("Be kind.".into(), Some(language))
                    .ends_with(&format!("{}, whatever language you hear.", language.rule()))
            );
        }
    }

    #[test]
    fn the_change_is_said_once() {
        let dir = scratch("said");
        let said = dir.join("language-said");
        // An English lamp never changed owes nothing, and writes nothing.
        assert_eq!(owed_at(None, &said), None);
        let owed = owed_at(Some(Language::Japanese), &said);
        assert_eq!(owed, Some(("ja", "これからは日本語で話しますね。")));
        record(&said, "ja").unwrap();
        assert_eq!(owed_at(Some(Language::Japanese), &said), None);
        // Another language, then back to English, each said once.
        assert_eq!(owed_at(Some(Language::Korean), &said).unwrap().0, "ko");
        record(&said, "ko").unwrap();
        assert_eq!(
            owed_at(Some(Language::Vietnamese), &said),
            Some(("vi", "Từ giờ mình sẽ nói tiếng Việt nhé."))
        );
        record(&said, "vi").unwrap();
        assert_eq!(owed_at(None, &said), Some(("en", ENGLISH_CHANGED)));
        record(&said, "en").unwrap();
        assert_eq!(owed_at(None, &said), None);
        assert_eq!(fs::read_to_string(&said).unwrap(), "en\n");
    }
}
