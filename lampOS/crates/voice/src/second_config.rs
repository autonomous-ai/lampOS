//! The second voice route's private configuration: an LLM, a speech-to-text
//! and a text-to-speech service, with V1's field names. Off unless
//! `LAMP_VOICE_SECOND_CONFIG` names the file. Keys are never written in it:
//! each is a private credential file read like `provider.json`'s.
use crate::config::{read_credential, read_private};
use serde::Deserialize;
use std::{ffi::OsString, fmt, io, path::Path};

pub const SECOND_CONFIG_ENV: &str = "LAMP_VOICE_SECOND_CONFIG";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    llm_base_url: String,
    llm_model: String,
    llm_credential_file: String,
    #[serde(default)]
    stt_base_url: Option<String>,
    #[serde(default)]
    stt_credential_file: Option<String>,
    #[serde(default)]
    stt_language: Option<String>,
    #[serde(default)]
    deepgram_credential_file: Option<String>,
    #[serde(default)]
    tts_base_url: Option<String>,
    #[serde(default)]
    tts_credential_file: Option<String>,
    #[serde(default)]
    tts_provider: Option<String>,
    #[serde(default)]
    tts_voice: Option<String>,
    #[serde(default)]
    tts_model: Option<String>,
    /// Fields the configured chat request also carries, for an
    /// OpenAI-compatible endpoint that needs its own (a model's reasoning
    /// setting, say). The request's own fields cannot be replaced.
    #[serde(default)]
    llm_extra: Option<serde_json::Map<String, serde_json::Value>>,
    /// Google's Gemini API, for `LAMP_SECOND_CHAT=gemini` and
    /// `LAMP_SECOND_SPEECH=gemini`: its base URL (absent: Google's own)
    /// and the models (absent: [`GEMINI_CHAT_MODEL`],
    /// [`GEMINI_SPEECH_MODEL`]). Its key is the voice unit's credential
    /// [`GEMINI_CREDENTIAL`], never a field here.
    #[serde(default)]
    gemini_base_url: Option<String>,
    #[serde(default)]
    gemini_chat_model: Option<String>,
    #[serde(default)]
    gemini_speech_model: Option<String>,
    /// A web search the model may call (`web_search`): its URL and key file
    /// together, and fields of its own for the request. Absent: no search.
    #[serde(default)]
    search_url: Option<String>,
    #[serde(default)]
    search_credential_file: Option<String>,
    #[serde(default)]
    search_extra: Option<serde_json::Map<String, serde_json::Value>>,
    /// The model `LAMP_SECOND_CHAT=openrouter` asks through OpenRouter, a
    /// slug from its public model list; absent: [`OPENROUTER_MODEL`].
    #[serde(default)]
    openrouter_model: Option<String>,
    /// OpenRouter's `reasoning.effort` for it; absent: `minimal` for a
    /// Google model (Gemini 3 cannot think less), `none` for any other.
    #[serde(default)]
    openrouter_reasoning_effort: Option<String>,
    /// The model `LAMP_SECOND_CHAT=deepseek` asks of DeepSeek's own API;
    /// absent: [`DEEPSEEK_MODEL`].
    #[serde(default)]
    deepseek_model: Option<String>,
    /// Where `LAMP_SECOND_CHAT=deepseek` and `LAMP_SECOND_SEARCH=deepseek`
    /// go instead of DeepSeek's own API: the Autonomous relay (#1934), with
    /// the relay's credential (`llm_credential_file`) in place of the unit's
    /// `deepseek.key`. Absent: DeepSeek's own, as before.
    #[serde(default)]
    deepseek_base_url: Option<String>,
}

/// OpenRouter's OpenAI-compatible API.
pub const OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";
/// The owner's pick (19 Oct): DeepSeek V4.1 Flash, its dated 17 October
/// snapshot, pinned by OpenRouter's own slug for it, read from
/// `GET https://openrouter.ai/api/v1/models` on 19 Oct (text and image in,
/// tools); never the moving alias `~deepseek/deepseek-flash-latest`.
pub const OPENROUTER_MODEL: &str = "deepseek/deepseek-v4.1-flash-1017";
/// The second candidate through OpenRouter (`openrouter_model`), from the
/// same list.
pub const OPENROUTER_SECOND_MODEL: &str = "google/gemini-3.6-flash";
/// DeepSeek's own OpenAI-compatible API (`/chat/completions`, `/models`,
/// `/v1/tools/search`, `/v1/tools/fetch`). Its peak hours, 01:00 to 04:00
/// and 06:00 to 10:00 UTC on weekdays, cost double (DeepSeek's pricing page,
/// 19 Oct); there is no switch for them.
pub const DEEPSEEK_BASE_URL: &str = "https://api.deepseek.com";
/// DeepSeek's name for DeepSeek-V4.1-Flash-1017 (images and tools in;
/// DeepSeek's docs, 19 Oct: the dated name is not given as a model name).
/// Asked with thinking off.
pub const DEEPSEEK_MODEL: &str = "deepseek-flash";
/// DeepSeek's models that take no picture: a look never goes to one.
pub const DEEPSEEK_TEXT_ONLY: [&str; 1] = ["deepseek-v4-pro"];

/// The OpenRouter and DeepSeek chats as `second.json` sets them, the
/// defaults applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hosted {
    pub openrouter_model: String,
    pub openrouter_effort: String,
    pub deepseek_model: String,
    /// The relay DeepSeek's chat and search go through, if not DeepSeek's
    /// own API ([`DEEPSEEK_BASE_URL_ENV`] over `deepseek_base_url`).
    pub deepseek_base_url: Option<String>,
}

/// A web search service the model may call, as `second.json` names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchService {
    pub url: String,
    pub key: Key,
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Google's Gemini API, called directly.
pub const GEMINI_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";
/// The text model `LAMP_SECOND_CHAT=gemini` asks, with thinking at its
/// floor (the research of 19 Oct: the fastest first token among the
/// sub-second models with the higher score).
pub const GEMINI_CHAT_MODEL: &str = "gemini-3.6-flash";
/// The voice `LAMP_SECOND_SPEECH=gemini` asks: half ElevenLabs Flash's
/// price a minute of speech, rated above it (the research of 19 Oct).
pub const GEMINI_SPEECH_MODEL: &str = "gemini-3.8-flash-lite-tts";
/// The Gemini endpoint and voice from the voice unit's environment, over
/// `second.json`'s (the first builder on #1712: the configuration file is
/// the owner's, a unit's drop-in the battery's). Passed to the second
/// route's worker alone (`process.rs`), checked as the file's are.
pub const GEMINI_BASE_URL_ENV: &str = "LAMP_SECOND_GEMINI_BASE_URL";
/// The same for DeepSeek's chat and search through the relay (#1934): over
/// `second.json`'s `deepseek_base_url`.
pub const DEEPSEEK_BASE_URL_ENV: &str = "LAMP_SECOND_DEEPSEEK_BASE_URL";
pub const SPEECH_MODEL_ENV: &str = "LAMP_SECOND_SPEECH_MODEL";
/// Words speech to text is told to expect, beside the lamp's name, from the
/// voice unit's environment, comma-separated: the places people here ask
/// about (#1712: on lamp-4ace on 19 Oct "Hanoi" was heard as "Hannover"
/// twice, and the weather came back for Hannover).
pub const KEY_TERMS_ENV: &str = "LAMP_SECOND_KEY_TERMS";
/// At most this many, each at most [`KEY_TERM_BYTES`].
pub const MOST_KEY_TERMS: usize = 20;
pub const KEY_TERM_BYTES: usize = 40;
/// `on`: the route's lines go to Deepgram's Flux in place of Nova-3, when
/// they are in English, straight to Deepgram or through the Autonomous
/// proxy (`net::stt::flux`, on trial for #1712's hearing half); anything
/// else, or unset, Nova-3.
pub const FLUX_ENV: &str = "LAMP_SECOND_FLUX";
/// Fields of the chat request that `llm_extra` may not replace.
const OWN_FIELDS: [&str; 5] = ["model", "messages", "stream", "tools", "tool_choice"];

/// A key read from a credential file. Never shown by Debug.
#[derive(Clone, PartialEq, Eq)]
pub struct Key(String);
impl Key {
    pub fn expose(&self) -> &str {
        &self.0
    }
}
impl fmt::Debug for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Key([REDACTED])")
    }
}

/// Which speech-to-text service the route uses: Deepgram when its
/// credential file is given, as in V1, otherwise the Autonomous one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stt {
    Deepgram { key: Key },
    Autonomous { base_url: String, key: Key },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TtsProvider {
    OpenAi,
    ElevenLabs,
}

impl TtsProvider {
    /// The voice's name in a journal line.
    pub fn name(self) -> &'static str {
        match self {
            Self::OpenAi => "OpenAI's voice",
            Self::ElevenLabs => "ElevenLabs",
        }
    }
}

/// The route with V1's fallbacks applied: an empty or absent STT or TTS
/// URL or credential file is the LLM's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecondConfig {
    pub llm_base_url: String,
    pub llm_model: String,
    pub llm_key: Key,
    pub stt: Stt,
    pub stt_language: Option<String>,
    /// Words speech to text is told to expect beside the lamp's name
    /// ([`KEY_TERMS_ENV`]); none unless set.
    pub stt_key_terms: Vec<String>,
    /// Lines go to Flux ([`FLUX_ENV`]); off unless set.
    pub stt_flux: bool,
    pub tts_base_url: String,
    pub tts_key: Key,
    pub tts_provider: TtsProvider,
    pub tts_voice: Option<String>,
    /// The configured voice's model (`tts_model`, or [`SPEECH_MODEL_ENV`]);
    /// absent, the provider's own default (`net::tts::default_model`).
    pub tts_model: Option<String>,
    /// `llm_extra`; empty when absent.
    pub llm_extra: serde_json::Map<String, serde_json::Value>,
    pub gemini: Gemini,
    /// `search_url` with `search_credential_file`; `None`: no web search.
    pub search: Option<SearchService>,
    pub hosted: Hosted,
}

/// Google's Gemini API as `second.json` sets it, the defaults applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gemini {
    pub base_url: String,
    pub chat_model: String,
    pub speech_model: String,
}

/// `None` when the variable is unset or empty: the second route is off.
pub fn load_from_env() -> io::Result<Option<SecondConfig>> {
    from_setting(std::env::var_os(SECOND_CONFIG_ENV))
}

fn from_setting(value: Option<OsString>) -> io::Result<Option<SecondConfig>> {
    match value {
        Some(path) if !path.is_empty() => load_from_path(Path::new(&path)).map(Some),
        _ => Ok(None),
    }
}

pub fn load_from_path(path: &Path) -> io::Result<SecondConfig> {
    let data = read_private(path, 16_384)?;
    // Gemini's key moved to the voice unit's credential: a file that still
    // names the old field is refused by name, so the person setting the lamp
    // up is told where the key goes now, and the key is never read here.
    if serde_json::from_slice::<serde_json::Value>(&data)
        .ok()
        .and_then(|value| value.get("gemini_credential_file").cloned())
        .is_some()
    {
        return Err(invalid(
            io::ErrorKind::InvalidInput,
            "gemini_credential_file is no longer read: Gemini's key is the voice unit's credential gemini.key (LoadCredential=gemini.key:<file>)",
        ));
    }
    let raw: Raw = serde_json::from_slice(&data).map_err(|_| {
        invalid(
            io::ErrorKind::InvalidData,
            "invalid second route configuration",
        )
    })?;
    let given = |value: Option<String>| value.filter(|v| !v.is_empty());
    let tts_provider = match given(raw.tts_provider).as_deref() {
        None | Some("openai") => TtsProvider::OpenAi,
        Some("elevenlabs") => TtsProvider::ElevenLabs,
        Some(_) => {
            return Err(invalid(
                io::ErrorKind::InvalidInput,
                "tts_provider must be openai or elevenlabs",
            ));
        }
    };
    if raw.llm_model.is_empty() {
        return Err(invalid(
            io::ErrorKind::InvalidInput,
            "llm_model is required",
        ));
    }
    let llm_base_url = checked_url(raw.llm_base_url)?;
    let stt_base_url = given(raw.stt_base_url).unwrap_or_else(|| llm_base_url.clone());
    let tts_base_url = given(raw.tts_base_url).unwrap_or_else(|| llm_base_url.clone());
    let stt_file =
        given(raw.stt_credential_file).unwrap_or_else(|| raw.llm_credential_file.clone());
    let tts_file =
        given(raw.tts_credential_file).unwrap_or_else(|| raw.llm_credential_file.clone());
    let stt = match given(raw.deepgram_credential_file) {
        Some(file) => Stt::Deepgram { key: key(&file)? },
        None => Stt::Autonomous {
            base_url: checked_url(stt_base_url)?,
            key: key(&stt_file)?,
        },
    };
    let llm_extra = raw.llm_extra.unwrap_or_default();
    if llm_extra.len() > 16 || OWN_FIELDS.iter().any(|own| llm_extra.contains_key(*own)) {
        return Err(invalid(
            io::ErrorKind::InvalidInput,
            "llm_extra may add at most 16 fields, none of the request's own",
        ));
    }
    let model = |name: Option<String>, default: &str| {
        gemini_model(given(name).unwrap_or_else(|| default.to_owned()))
    };
    let gemini = Gemini {
        base_url: checked_url(
            given(raw.gemini_base_url).unwrap_or_else(|| GEMINI_BASE_URL.to_owned()),
        )?,
        chat_model: model(raw.gemini_chat_model, GEMINI_CHAT_MODEL)?,
        speech_model: model(raw.gemini_speech_model, GEMINI_SPEECH_MODEL)?,
    };
    let search = match (given(raw.search_url), given(raw.search_credential_file)) {
        (None, None) => None,
        (Some(url), Some(file)) => Some(SearchService {
            url: checked_url(url)?,
            key: key(&file)?,
            extra: raw.search_extra.unwrap_or_default(),
        }),
        _ => {
            return Err(invalid(
                io::ErrorKind::InvalidInput,
                "search_url and search_credential_file go together",
            ));
        }
    };
    // A model's slug goes in the request's body: letters, digits and the
    // marks OpenRouter's slugs use.
    let slug = |name: Option<String>, default: &str| -> io::Result<String> {
        let name = given(name).unwrap_or_else(|| default.to_owned());
        if name.len() > 128
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '/' | ':'))
        {
            return Err(invalid(
                io::ErrorKind::InvalidInput,
                "a model name is letters, digits, '-', '.', '_', '/' and ':'",
            ));
        }
        Ok(name)
    };
    let openrouter_model = slug(raw.openrouter_model, OPENROUTER_MODEL)?;
    let openrouter_effort = match given(raw.openrouter_reasoning_effort) {
        Some(effort) => match effort.as_str() {
            "none" | "minimal" | "low" | "medium" | "high" => effort,
            _ => {
                return Err(invalid(
                    io::ErrorKind::InvalidInput,
                    "openrouter_reasoning_effort is none, minimal, low, medium or high",
                ));
            }
        },
        None => least_effort(&openrouter_model).to_owned(),
    };
    let hosted = Hosted {
        openrouter_model,
        openrouter_effort,
        deepseek_model: slug(raw.deepseek_model, DEEPSEEK_MODEL)?,
        deepseek_base_url: given(raw.deepseek_base_url).map(checked_url).transpose()?,
    };
    Ok(SecondConfig {
        search,
        hosted,
        llm_key: key(&raw.llm_credential_file)?,
        llm_base_url,
        llm_model: raw.llm_model,
        stt,
        stt_language: given(raw.stt_language),
        stt_key_terms: Vec::new(),
        stt_flux: false,
        tts_base_url: checked_url(tts_base_url)?,
        tts_key: key(&tts_file)?,
        tts_provider,
        tts_voice: given(raw.tts_voice),
        tts_model: given(raw.tts_model).map(gemini_model).transpose()?,
        llm_extra,
        gemini,
    })
}

/// Where systemd gives a unit its credentials (`LoadCredential=`).
pub const CREDENTIALS_ENV: &str = "CREDENTIALS_DIRECTORY";
/// The voice unit's credential for OpenRouter
/// (`LoadCredential=openrouter.key:/etc/lampOS/policy/jev.key`, the key the
/// Policy already uses).
pub const OPENROUTER_CREDENTIAL: &str = "openrouter.key";
/// The voice unit's credential for DeepSeek's own API
/// (`LoadCredential=deepseek.key:/etc/lampOS/voice/deepseek.key`).
pub const DEEPSEEK_CREDENTIAL: &str = "deepseek.key";
/// The voice unit's credential for Google's Gemini API, for
/// `LAMP_SECOND_CHAT=gemini` and `LAMP_SECOND_SPEECH=gemini`
/// (`LoadCredential=gemini.key:/etc/lampOS/voice/gemini.key`).
pub const GEMINI_CREDENTIAL: &str = "gemini.key";

/// The key in the unit's credential `name`, in `directory` (the unit's
/// `CREDENTIALS_DIRECTORY`), read as the other keys are; why not, naming the
/// credential and never its contents.
pub fn unit_credential(directory: Option<&std::ffi::OsStr>, name: &str) -> io::Result<Key> {
    let directory = directory
        .filter(|directory| Path::new(directory).is_absolute())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "the voice unit has no credentials ({CREDENTIALS_ENV} is not set): {name} is given by LoadCredential={name}:<file>"
                ),
            )
        })?;
    credential(&Path::new(directory).join(name)).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("the voice unit's credential {name}: {error}"),
        )
    })
}

/// The least OpenRouter's `reasoning.effort` may be for `model`: Gemini 3
/// cannot stop thinking, so `minimal`; any other is asked not to think.
pub fn least_effort(model: &str) -> &'static str {
    if model.starts_with("google/") {
        "minimal"
    } else {
        "none"
    }
}

/// A key from a private credential file, read as `second.json`'s are.
pub(crate) fn credential(file: &Path) -> io::Result<Key> {
    key(file
        .to_str()
        .ok_or_else(|| invalid(io::ErrorKind::InvalidInput, "credential path must be UTF-8"))?)
}

fn invalid(kind: io::ErrorKind, message: &'static str) -> io::Error {
    io::Error::new(kind, message)
}

fn key(file: &str) -> io::Result<Key> {
    if !Path::new(file).is_absolute() {
        return Err(invalid(
            io::ErrorKind::InvalidInput,
            "credential path must be absolute",
        ));
    }
    let text = read_credential(Path::new(file))?;
    if text.trim().is_empty() {
        return Err(invalid(
            io::ErrorKind::InvalidData,
            "credential file is empty",
        ));
    }
    Ok(Key(text))
}

/// An explicit https URL. Authentication belongs only in the key; userinfo,
/// queries and fragments are refused, as for the live endpoint.
/// [`KEY_TERMS_ENV`]'s words: comma-separated, each letters (any script),
/// digits, spaces, `-`, `.` and `'`, none empty.
fn key_terms(terms: &str) -> io::Result<Vec<String>> {
    let terms: Vec<String> = terms
        .split(',')
        .map(|term| term.trim().to_owned())
        .collect();
    let fits = |term: &String| {
        !term.is_empty()
            && term.len() <= KEY_TERM_BYTES
            && term
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '.' | '\''))
    };
    if terms.len() > MOST_KEY_TERMS || !terms.iter().all(fits) {
        return Err(invalid(
            io::ErrorKind::InvalidInput,
            "at most 20 comma-separated words, each of letters, digits, spaces, '-', '.' and '\'', at most 40 bytes",
        ));
    }
    Ok(terms)
}

/// A Gemini model's name, a path segment of the request's URL.
fn gemini_model(name: String) -> io::Result<String> {
    if name.len() > 128
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_'))
    {
        return Err(invalid(
            io::ErrorKind::InvalidInput,
            "a model name is letters, digits, '-', '.' and '_'",
        ));
    }
    Ok(name)
}

impl SecondConfig {
    /// With [`GEMINI_BASE_URL_ENV`], [`DEEPSEEK_BASE_URL_ENV`], [`SPEECH_MODEL_ENV`] and
    /// [`KEY_TERMS_ENV`] as `var` gives them, over the file's; unset or
    /// empty, the file's stay. Refused as the file's fields are, naming the
    /// variable. [`FLUX_ENV`] is on for `on` alone.
    pub fn with_environment(mut self, var: impl Fn(&str) -> Option<OsString>) -> io::Result<Self> {
        let given = |name: &str| -> io::Result<Option<String>> {
            var(name)
                .filter(|value| !value.is_empty())
                .map(|value| {
                    value.into_string().map_err(|_| {
                        io::Error::new(
                            io::ErrorKind::InvalidInput,
                            format!("{name} is not Unicode"),
                        )
                    })
                })
                .transpose()
        };
        fn named(name: &'static str) -> impl Fn(io::Error) -> io::Error {
            move |error| io::Error::new(error.kind(), format!("{name}: {error}"))
        }
        if let Some(url) = given(GEMINI_BASE_URL_ENV)? {
            self.gemini.base_url = checked_url(url).map_err(named(GEMINI_BASE_URL_ENV))?;
        }
        if let Some(url) = given(DEEPSEEK_BASE_URL_ENV)? {
            self.hosted.deepseek_base_url =
                Some(checked_url(url).map_err(named(DEEPSEEK_BASE_URL_ENV))?);
        }
        // The chosen voice's model: Gemini's, or the configured voice's
        // (ElevenLabs, OpenAI); only the voice in use reads its own (#1712:
        // the owner, "use v4 turbo").
        if let Some(model) = given(SPEECH_MODEL_ENV)? {
            let model = gemini_model(model).map_err(named(SPEECH_MODEL_ENV))?;
            self.gemini.speech_model = model.clone();
            self.tts_model = Some(model);
        }
        if let Some(terms) = given(KEY_TERMS_ENV)? {
            self.stt_key_terms = key_terms(&terms).map_err(named(KEY_TERMS_ENV))?;
        }
        self.stt_flux = var(FLUX_ENV).is_some_and(|value| value == "on");
        Ok(self)
    }
}

fn checked_url(url: String) -> io::Result<String> {
    let host = url
        .strip_prefix("https://")
        .map(|rest| rest.split('/').next().unwrap_or(""));
    if url.len() > 2048 || url.contains(['?', '#', '@']) || host.is_none_or(str::is_empty) {
        return Err(invalid(
            io::ErrorKind::InvalidInput,
            "service URL must be https with a host",
        ));
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

    fn directory(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "lamp-voice-second-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    fn private(path: &Path, text: &str) {
        fs::write(path, text).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }

    /// The configuration with `extra` fields, beside an LLM key file.
    fn written(name: &str, extra: &str) -> (PathBuf, PathBuf) {
        let directory = directory(name);
        let llm = directory.join("llm.key");
        private(&llm, "llm-secret\n");
        let config = directory.join("second.json");
        private(
            &config,
            &format!(
                r#"{{"llm_base_url":"https://llm.example.invalid/v1","llm_model":"m","llm_credential_file":"{}"{extra}}}"#,
                llm.display()
            ),
        );
        (directory, config)
    }

    #[test]
    fn an_unset_or_empty_variable_leaves_the_route_off() {
        assert!(from_setting(None).unwrap().is_none());
        assert!(from_setting(Some(OsString::new())).unwrap().is_none());
    }

    #[test]
    fn absent_stt_and_tts_settings_are_the_llms() {
        let (_, config) = written("fallback", r#","stt_base_url":"","tts_voice":"alloy""#);
        let route = load_from_path(&config).unwrap();
        assert_eq!(route.llm_key.expose(), "llm-secret");
        assert_eq!(
            route.stt,
            Stt::Autonomous {
                base_url: "https://llm.example.invalid/v1".into(),
                key: route.llm_key.clone(),
            }
        );
        assert_eq!(route.tts_base_url, route.llm_base_url);
        assert_eq!(route.tts_key, route.llm_key);
        assert_eq!(route.tts_provider, TtsProvider::OpenAi);
        assert_eq!(route.tts_voice.as_deref(), Some("alloy"));
        assert!(!format!("{route:?}").contains("llm-secret"));
    }

    #[test]
    fn a_deepgram_credential_selects_deepgram() {
        let directory = directory("deepgram");
        let deepgram = directory.join("deepgram.key");
        private(&deepgram, "dg-secret");
        let (_, config) = written(
            "deepgram",
            &format!(
                r#","deepgram_credential_file":"{}","tts_provider":"elevenlabs","tts_base_url":"https://tts.example.invalid""#,
                deepgram.display()
            ),
        );
        let route = load_from_path(&config).unwrap();
        assert_eq!(
            route.stt,
            Stt::Deepgram {
                key: Key("dg-secret".into())
            }
        );
        assert_eq!(route.tts_provider, TtsProvider::ElevenLabs);
        assert_eq!(route.tts_base_url, "https://tts.example.invalid");
    }

    #[test]
    fn a_key_written_in_the_configuration_is_refused() {
        let (_, config) = written("inline", r#","llm_api_key":"sk-inline""#);
        assert!(load_from_path(&config).is_err());
    }

    #[test]
    fn an_unknown_tts_provider_is_refused() {
        let (_, config) = written("provider", r#","tts_provider":"polly""#);
        let error = load_from_path(&config).unwrap_err();
        assert!(error.to_string().contains("openai or elevenlabs"));
    }

    #[test]
    fn a_credential_others_can_read_is_refused() {
        let (directory, config) = written("mode", "");
        fs::set_permissions(directory.join("llm.key"), fs::Permissions::from_mode(0o644)).unwrap();
        let error = load_from_path(&config).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn a_plain_http_url_is_refused() {
        let (_, config) = written("http", r#","tts_base_url":"http://tts.example.invalid""#);
        assert!(load_from_path(&config).is_err());
    }

    /// Without its fields, Gemini is Google's own API, its key left to the
    /// Live route's, and the research's two models; nothing extra is sent.
    #[test]
    fn gemini_defaults_to_googles_api_and_the_researched_models() {
        let (_, config) = written("gemini-defaults", "");
        let route = load_from_path(&config).unwrap();
        assert_eq!(
            route.gemini,
            Gemini {
                base_url: GEMINI_BASE_URL.into(),
                chat_model: GEMINI_CHAT_MODEL.into(),
                speech_model: GEMINI_SPEECH_MODEL.into(),
            }
        );
        assert!(route.llm_extra.is_empty());
    }

    /// Gemini's key is the voice unit's credential `gemini.key`, never a
    /// field of this file: a file that still names one is refused, so a key
    /// put there is never passed over in silence.
    #[test]
    fn gemini_and_the_extra_chat_fields_are_taken_as_written() {
        let directory = directory("gemini-set");
        let google = directory.join("google.key");
        private(&google, "g-secret\n");
        let (_, config) = written(
            "gemini-key-field",
            &format!(r#","gemini_credential_file":"{}""#, google.display()),
        );
        let refused = load_from_path(&config).unwrap_err().to_string();
        assert!(refused.contains("gemini_credential_file"), "{refused}");
        assert!(!refused.contains("g-secret"));
        let (_, config) = written(
            "gemini-set",
            r#","gemini_base_url":"https://gemini.example.invalid/v1beta","gemini_chat_model":"gemini-3.5-flash-lite","gemini_speech_model":"gemini-3.8-flash-tts","llm_extra":{"thinking":{"type":"disabled"}}"#,
        );
        let route = load_from_path(&config).unwrap();
        assert_eq!(
            route.gemini.base_url,
            "https://gemini.example.invalid/v1beta"
        );
        assert_eq!(route.gemini.chat_model, "gemini-3.5-flash-lite");
        assert_eq!(route.gemini.speech_model, "gemini-3.8-flash-tts");
        assert_eq!(
            serde_json::Value::Object(route.llm_extra.clone()),
            serde_json::json!({"thinking": {"type": "disabled"}})
        );
    }

    /// #1712: the unit's Gemini endpoint and voice are taken over the
    /// file's, unset or empty leave the file's, and a bad one is refused
    /// naming its variable, as the file's fields are.
    #[test]
    fn the_units_gemini_endpoint_voice_and_key_terms_go_over_the_files() {
        let (_, config) = written("gemini-env", "");
        let route = || load_from_path(&config).unwrap();
        let relay = "https://campaign-api.autonomous.ai/api/v1/ai/v1/google-search/v1beta";
        let set = route()
            .with_environment(|name| match name {
                GEMINI_BASE_URL_ENV => Some(relay.into()),
                SPEECH_MODEL_ENV => Some("gemini-3.8-flash-tts".into()),
                _ => None,
            })
            .unwrap();
        assert_eq!(set.gemini.base_url, relay);
        assert_eq!(set.gemini.speech_model, "gemini-3.8-flash-tts");
        assert_eq!(set.tts_model.as_deref(), Some("gemini-3.8-flash-tts"));
        assert_eq!(route().tts_model, None, "the provider's own default");
        let (_, turbo) = written(
            "tts-model",
            r#","tts_provider":"elevenlabs","tts_model":"eleven_v4_turbo""#,
        );
        assert_eq!(
            load_from_path(&turbo).unwrap().tts_model.as_deref(),
            Some("eleven_v4_turbo")
        );
        let (_, odd) = written("tts-model-odd", r#","tts_model":"../x""#);
        assert!(load_from_path(&odd).is_err());
        assert_eq!(set.gemini.chat_model, GEMINI_CHAT_MODEL);
        let unset = route().with_environment(|_| Some("".into())).unwrap();
        assert_eq!(unset.gemini, route().gemini);
        assert!(set.stt_key_terms.is_empty());
        let places = route()
            .with_environment(|name| {
                (name == KEY_TERMS_ENV).then(|| " Hanoi, Ho Chi Minh City,Đà Nẵng ".into())
            })
            .unwrap();
        assert_eq!(
            places.stt_key_terms,
            ["Hanoi", "Ho Chi Minh City", "Đà Nẵng"]
        );
        assert!(!places.stt_flux && !set.stt_flux);
        let flux = |value: &str| {
            route()
                .with_environment(|name| (name == FLUX_ENV).then(|| value.into()))
                .unwrap()
                .stt_flux
        };
        assert!(flux("on"));
        for off in ["", "off", "ON", "yes"] {
            assert!(!flux(off), "{off:?}");
        }
        let many = vec!["Hanoi"; MOST_KEY_TERMS + 1].join(",");
        for (name, value) in [
            (GEMINI_BASE_URL_ENV, "http://plain.example.invalid"),
            (GEMINI_BASE_URL_ENV, "https://relay.example.invalid/?key=1"),
            (DEEPSEEK_BASE_URL_ENV, "http://plain.example.invalid"),
            (
                DEEPSEEK_BASE_URL_ENV,
                "https://relay.example.invalid/?key=1",
            ),
            (SPEECH_MODEL_ENV, "tts/../chat"),
            (KEY_TERMS_ENV, "Hanoi,,Hue"),
            (KEY_TERMS_ENV, "Hanoi&keyterm=x"),
            (KEY_TERMS_ENV, many.as_str()),
        ] {
            let refused = route()
                .with_environment(|asked| (asked == name).then(|| value.into()))
                .unwrap_err()
                .to_string();
            assert!(refused.starts_with(name), "{refused}");
        }
    }

    /// OpenRouter's pinned DeepSeek snapshot and DeepSeek's own Flash by
    /// default, each asked to think as little as it can; a model set is
    /// taken, a strange name or effort refused.
    #[test]
    fn the_hosted_chats_default_to_the_owners_picks_and_think_least() {
        let (_, config) = written("hosted-defaults", "");
        assert_eq!(
            load_from_path(&config).unwrap().hosted,
            Hosted {
                openrouter_model: "deepseek/deepseek-v4.1-flash-1017".into(),
                openrouter_effort: "none".into(),
                deepseek_model: "deepseek-flash".into(),
                deepseek_base_url: None,
            }
        );
        let (_, config) = written(
            "hosted-set",
            r#","openrouter_model":"google/gemini-3.6-flash","deepseek_model":"deepseek-v4-pro""#,
        );
        let hosted = load_from_path(&config).unwrap().hosted;
        assert_eq!(hosted.openrouter_model, "google/gemini-3.6-flash");
        assert_eq!(hosted.openrouter_effort, "minimal");
        assert_eq!(hosted.deepseek_model, "deepseek-v4-pro");
        let (_, config) = written("hosted-effort", r#","openrouter_reasoning_effort":"low""#);
        assert_eq!(
            load_from_path(&config).unwrap().hosted.openrouter_effort,
            "low"
        );
        for extra in [
            r#","openrouter_reasoning_effort":"max""#,
            r#","openrouter_model":"deepseek flash""#,
            r#","deepseek_model":"x\"y""#,
        ] {
            let (_, config) = written("hosted-refused", extra);
            assert!(load_from_path(&config).is_err(), "{extra}");
        }
    }

    /// A unit's credential is read from its `CREDENTIALS_DIRECTORY`, as the
    /// other keys are; without one, or without the file, the error names the
    /// credential and nothing of a key.
    #[test]
    fn a_units_credential_is_read_from_its_directory_and_never_shown() {
        let directory = directory("unit-credentials");
        private(&directory.join("openrouter.key"), "or-secret\n");
        let key = unit_credential(Some(directory.as_os_str()), OPENROUTER_CREDENTIAL).unwrap();
        assert_eq!(key.expose(), "or-secret");
        assert_eq!(format!("{key:?}"), "Key([REDACTED])");
        let missing = unit_credential(Some(directory.as_os_str()), DEEPSEEK_CREDENTIAL)
            .unwrap_err()
            .to_string();
        assert!(missing.contains("deepseek.key"), "{missing}");
        let unset = unit_credential(None, DEEPSEEK_CREDENTIAL)
            .unwrap_err()
            .to_string();
        assert!(unset.contains("CREDENTIALS_DIRECTORY"), "{unset}");
        assert!(
            unit_credential(
                Some(std::ffi::OsStr::new("relative")),
                OPENROUTER_CREDENTIAL
            )
            .is_err()
        );
    }

    /// A web search is named by its URL and key file together, or not at all.
    #[test]
    fn a_search_service_needs_its_url_and_key_together() {
        let (_, config) = written("search-none", "");
        assert_eq!(load_from_path(&config).unwrap().search, None);
        let directory = directory("search-set");
        let search = directory.join("search.key");
        private(&search, "s-secret");
        let (_, config) = written(
            "search-set",
            &format!(
                r#","search_url":"https://search.example.invalid/search","search_credential_file":"{}","search_extra":{{"type":"instant"}}"#,
                search.display()
            ),
        );
        let route = load_from_path(&config).unwrap();
        let service = route.search.unwrap();
        assert_eq!(service.url, "https://search.example.invalid/search");
        assert_eq!(service.key, Key("s-secret".into()));
        assert_eq!(service.extra["type"], "instant");
        let (_, config) = written(
            "search-half",
            r#","search_url":"https://search.example.invalid/search""#,
        );
        assert!(load_from_path(&config).is_err());
    }

    /// #1712, the first builder at 21:08 and 21:14: the product's template
    /// (`deploy/product/settings/second.json`, which install places) is a
    /// configuration this loader takes, on the relay with the one key a lamp
    /// already has, `/etc/lampOS/voice/credential`: speech to text and the
    /// voice fall back to the text model's base and key, and the voice is
    /// ElevenLabs.
    #[test]
    fn the_products_template_loads_on_the_relay_with_the_lamps_one_key() {
        let template = include_str!("../../../deploy/product/settings/second.json");
        let value: serde_json::Value = serde_json::from_str(template).unwrap();
        let named: Vec<&str> = value
            .as_object()
            .unwrap()
            .iter()
            .filter(|(field, _)| field.ends_with("_credential_file"))
            .filter_map(|(_, path)| path.as_str())
            .collect();
        assert_eq!(named, ["/etc/lampOS/voice/credential"]);
        let directory = directory("template");
        let key = directory.join("credential");
        private(&key, "relay-secret\n");
        let config = directory.join("second.json");
        private(
            &config,
            &template.replace("/etc/lampOS/voice/credential", &key.display().to_string()),
        );
        let route = load_from_path(&config).unwrap();
        assert_eq!(route.llm_key.expose(), "relay-secret");
        assert_eq!(route.tts_key, route.llm_key);
        assert_eq!(route.tts_base_url, route.llm_base_url);
        assert_eq!(route.tts_provider, TtsProvider::ElevenLabs);
        assert_eq!(
            route.stt,
            Stt::Autonomous {
                base_url: route.llm_base_url.clone(),
                key: route.llm_key.clone(),
            }
        );
        assert!(
            route
                .llm_base_url
                .starts_with("https://campaign-api.autonomous.ai/")
        );
        fs::remove_dir_all(&directory).unwrap();
    }

    /// A model name goes into the request's URL: only a plain name; and
    /// `llm_extra` cannot replace what the request itself says.
    #[test]
    fn a_strange_model_name_or_extra_field_is_refused() {
        for extra in [
            r#","gemini_chat_model":"../models/x""#,
            r#","gemini_speech_model":"tts?key=1""#,
            r#","llm_extra":{"model":"other"}"#,
            r#","llm_extra":{"messages":[]}"#,
            r#","gemini_base_url":"http://gemini.example.invalid""#,
        ] {
            let (_, config) = written("gemini-refused", extra);
            assert!(load_from_path(&config).is_err(), "{extra}");
        }
    }
}
