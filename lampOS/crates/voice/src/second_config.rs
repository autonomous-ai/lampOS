//! The second voice route's private configuration: an LLM, a speech-to-text
//! and a text-to-speech service, with V1's field names. Off unless
//! `LAMP_VOICE_SECOND_CONFIG` names the file. Keys are never written in it:
//! each is a private credential file read like `provider.json`'s.
use crate::config::{Unread, read_credential, read_private, unread_fields, unreadable};
use serde::Deserialize;
use std::{ffi::OsString, fmt, io, path::Path};

pub const SECOND_CONFIG_ENV: &str = "LAMP_VOICE_SECOND_CONFIG";

/// Not `deny_unknown_fields`: a field a newer build writes is read past and
/// named once at start, as `provider.json`'s are (#1505, #1680).
#[derive(Deserialize)]
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
    #[serde(flatten)]
    unread: Unread,
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
/// The company relay's host: a route whose `llm_base_url` is on it reaches
/// DeepSeek through it too, with no `deepseek_base_url` ([`relay_deepseek`]).
const COMPANY_RELAY_HOST: &str = "campaign-api.autonomous.ai";

/// DeepSeek through the relay `llm_base_url` names, when it is the
/// company's: `…/api/v1/ai` gives `…/api/v1/ai/v1/deepseek`, as `/v1` is
/// added to its other services (`net::openai_base`). Lamps carry no provider
/// keys (the lead, #1711), so such a route with no `deepseek_base_url` and
/// no `LAMP_SECOND_DEEPSEEK_BASE_URL` reaches DeepSeek there, not DeepSeek's
/// own API with a `deepseek.key` it doesn't have. That keeps a `second.json`
/// an update round kept from before the product unit dropped the variable
/// working (El's P1 on #2978). None for any other host.
fn relay_deepseek(llm_base_url: &str) -> Option<String> {
    let host = llm_base_url.strip_prefix("https://")?.split('/').next()?;
    if host != COMPANY_RELAY_HOST {
        return None;
    }
    let base = llm_base_url.trim_end_matches('/');
    Some(if base.ends_with("/ai") {
        format!("{base}/v1/deepseek")
    } else {
        format!("{base}/deepseek")
    })
}
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
    /// The person's own DeepSeek key ([`OWN_KEYS_ENV`], #2094): over the
    /// relay's and the unit's `deepseek.key`, at `deepseek_base_url`.
    pub own_deepseek: Option<Key>,
    /// The relay the person's key went over, if there was one: a request
    /// their endpoint refuses (401, 403) goes there, with the relay's key.
    pub relay_deepseek_base_url: Option<String>,
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
/// The same for text to speech (#2882): over `second.json`'s `tts_base_url`
/// (or `llm_base_url`), so a lamp can speak through a near relay.
pub const SPEECH_BASE_URL_ENV: &str = "LAMP_SECOND_SPEECH_BASE_URL";
/// The same for speech to text through the relay (#2882): over
/// `stt_base_url` (or `llm_base_url`). Deepgram's own address, with its own
/// key, is not the relay's and stays.
pub const LISTENING_BASE_URL_ENV: &str = "LAMP_SECOND_LISTENING_BASE_URL";
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
    #[cfg(test)]
    pub(crate) fn for_test(key: &str) -> Self {
        Self(key.to_owned())
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
    /// Which services' addresses the voice unit's environment gave: the
    /// relay's near addresses never go over them (`LAMP_RELAY_ROUTES`, #2882).
    pub pinned: Pinned,
}

/// Which of [`GEMINI_BASE_URL_ENV`], [`DEEPSEEK_BASE_URL_ENV`],
/// [`SPEECH_BASE_URL_ENV`] and [`LISTENING_BASE_URL_ENV`] were given ([`SecondConfig::with_environment`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Pinned {
    pub gemini: bool,
    pub chat: bool,
    pub speech: bool,
    pub listening: bool,
}

/// Google's Gemini API as `second.json` sets it, the defaults applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gemini {
    pub base_url: String,
    pub chat_model: String,
    pub speech_model: String,
}

/// `None` when the variable is unset or empty: the second route is off.
/// Where the product's voice unit names the second route's configuration
/// (`deploy/product/lamp-voice.service`).
pub const PRODUCT_PATH: &str = "/etc/lampOS/voice/second.json";
/// The product's configuration, as `lamp-update install` places it
/// (`deploy/product/settings/second.json`, its `install::SECOND`).
pub const PRODUCT: &str = include_str!("../../../deploy/product/settings/second.json");

/// The product's configuration placed at [`PRODUCT_PATH`] when the voice's
/// unit asks for the second route there and nothing is there (#1940, the
/// reviewer's P1 at 00:09). An update round brings this binary and the
/// voice's new unit, but never what is under `/etc/lampOS`: only `lamp-update
/// install` places it, and a round runs the updater the lamp already has
/// (docs/updates.md). Without this, the first round would start a voice
/// that can't start, put every part back, and refuse every later build. It
/// is placed as install places it: this process user's, 0600, written whole
/// and then given its name. Its bytes are install's default, so a later
/// install keeps it up to date as its own. Any other path, a route other
/// than `second`, or a file already there (even another one) is left
/// alone. Returns whether it placed it.
pub fn place_product_if_missing(
    route: Option<&std::ffi::OsStr>,
    configured: Option<&std::ffi::OsStr>,
) -> io::Result<bool> {
    place_if_missing(route, configured, Path::new(PRODUCT_PATH), PRODUCT)
}

fn place_if_missing(
    route: Option<&std::ffi::OsStr>,
    configured: Option<&std::ffi::OsStr>,
    product: &Path,
    text: &str,
) -> io::Result<bool> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    if route != Some(std::ffi::OsStr::new("second")) || configured.map(Path::new) != Some(product) {
        return Ok(false);
    }
    match std::fs::symlink_metadata(product) {
        Ok(_) => return Ok(false),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let folder = product
        .parent()
        .ok_or_else(|| io::Error::other("the configuration's path has no folder"))?;
    let placing = folder.join(".second.json.placing");
    // One a cut left half written is not this one.
    let _ = std::fs::remove_file(&placing);
    {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&placing)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
    }
    std::fs::rename(&placing, product)?;
    std::fs::File::open(folder)?.sync_all()?;
    Ok(true)
}

/// The second route's file as the coordinator's own listeners take it (the
/// stop words', the wait words'), with the unit's key terms
/// ([`KEY_TERMS_ENV`]) over the file's, as the worker reads them. Without
/// them those streams asked with the lamp's name alone (#2602: the relay's
/// 1-term sessions beside the worker's 10-term ones). A bad
/// [`KEY_TERMS_ENV`] is refused in the journal and the file's key terms are
/// kept: these listeners do not go unheard over it (the worker, reading it
/// through [`SecondConfig::with_environment`], still fails on it).
pub fn load_from_env() -> io::Result<Option<SecondConfig>> {
    from_setting(std::env::var_os(SECOND_CONFIG_ENV), |name| {
        std::env::var_os(name)
    })
}

pub(crate) fn from_setting(
    value: Option<OsString>,
    var: impl Fn(&str) -> Option<OsString>,
) -> io::Result<Option<SecondConfig>> {
    match value {
        Some(path) if !path.is_empty() => {
            let file = load_from_path(Path::new(&path))?;
            Ok(Some(file.clone().with_key_terms_from(var).unwrap_or_else(
                |error| {
                    eprintln!("lamp-voice second: {error}: refused, the file's key terms are kept");
                    file
                },
            )))
        }
        _ => Ok(None),
    }
}

/// The second route's file. One that does not read is refused, naming the
/// file and why: a broken configuration is not run on. Fields this build
/// does not read are named once in the journal.
pub fn load_from_path(path: &Path) -> io::Result<SecondConfig> {
    let (config, line) = load_reporting(path)?;
    if let Some(line) = line {
        eprintln!("lamp-voice second: {line}");
    }
    Ok(config)
}

/// The file, and the journal line for its fields this build does not read.
fn load_reporting(path: &Path) -> io::Result<(SecondConfig, Option<String>)> {
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
    let raw: Raw = serde_json::from_slice(&data)
        .map_err(|error| unreadable("second route configuration", path, error))?;
    let line = unread_fields(path, &raw.unread)?;
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
        deepseek_base_url: given(raw.deepseek_base_url)
            .map(checked_url)
            .transpose()?
            .or_else(|| relay_deepseek(&llm_base_url)),
        own_deepseek: None,
        relay_deepseek_base_url: None,
    };
    let config = SecondConfig {
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
        pinned: Pinned::default(),
    };
    Ok((config, line))
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

/// `on`: a person's own DeepSeek and Deepgram keys are used when the
/// voice unit has them (#2094, `docs/own-keys.md`); anything else, or
/// unset, and they are never read.
pub const OWN_KEYS_ENV: &str = "LAMP_OWN_KEYS";
/// The person's keys as the voice unit's credentials: root's helper keeps
/// them in one folder, loaded whole (`LoadCredential=own:<folder>`), so
/// each file is `own_<name>`.
pub const OWN_DEEPSEEK_KEY: &str = "own_deepseek.key";
/// The DeepSeek endpoint the person gave with their key, one https URL.
pub const OWN_DEEPSEEK_URL: &str = "own_deepseek.url";
pub const OWN_DEEPGRAM_KEY: &str = "own_deepgram.key";

impl SecondConfig {
    /// With the person's own keys when `on` and the unit has them: DeepSeek's
    /// chat, search and observation pass at their endpoint (absent:
    /// DeepSeek's own API) with their key, over the relay; speech to text
    /// straight to Deepgram with theirs. One journal line each, naming whose
    /// key, never a key or a URL. A key that can't be read is passed over
    /// for the relay, said by its error's kind. Off: as it was, no line.
    pub fn with_own_keys(
        mut self,
        on: bool,
        credentials: Option<&std::ffi::OsStr>,
    ) -> (Self, Vec<String>) {
        if !on {
            return (self, Vec::new());
        }
        let own = |name: &str| match unit_credential(credentials, name) {
            Ok(key) => Ok(Some(key)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.kind()),
        };
        let mut lines = Vec::new();
        let deepseek = own(OWN_DEEPSEEK_KEY).and_then(|key| {
            let Some(key) = key else { return Ok(None) };
            let url = match own(OWN_DEEPSEEK_URL)? {
                Some(url) => checked_url(url.expose().trim().to_owned())
                    .map_err(|_| io::ErrorKind::InvalidData)?,
                None => DEEPSEEK_BASE_URL.to_owned(),
            };
            Ok(Some((key, url)))
        });
        lines.push(match deepseek {
            Ok(Some((key, url))) => {
                let line = format!(
                    "DeepSeek: the person's own key, at {} (LAMP_OWN_KEYS)",
                    host(&url)
                );
                self.hosted.relay_deepseek_base_url = self.hosted.deepseek_base_url.replace(url);
                self.hosted.own_deepseek = Some(key);
                line
            }
            Ok(None) => "DeepSeek: the relay, no key of the person's (LAMP_OWN_KEYS)".to_owned(),
            Err(kind) => format!(
                "DeepSeek: the relay, the person's key could not be read ({kind}) (LAMP_OWN_KEYS)"
            ),
        });
        lines.push(match own(OWN_DEEPGRAM_KEY) {
            Ok(Some(key)) => {
                self.stt = Stt::Deepgram { key };
                "Deepgram: the person's own key (LAMP_OWN_KEYS)".to_owned()
            }
            Ok(None) => "Deepgram: the relay, no key of the person's (LAMP_OWN_KEYS)".to_owned(),
            Err(kind) => format!(
                "Deepgram: the relay, the person's key could not be read ({kind}) (LAMP_OWN_KEYS)"
            ),
        });
        (self, lines)
    }
}

/// A URL's host, as the journal names where the person's words go: never
/// its path.
pub fn host(url: &str) -> &str {
    url.split_once("://")
        .map_or(url, |(_, rest)| rest)
        .split('/')
        .next()
        .unwrap_or("")
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
    /// With [`GEMINI_BASE_URL_ENV`], [`DEEPSEEK_BASE_URL_ENV`],
    /// [`SPEECH_BASE_URL_ENV`], [`LISTENING_BASE_URL_ENV`], [`SPEECH_MODEL_ENV`] and
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
            self.pinned.gemini = true;
        }
        if let Some(url) = given(DEEPSEEK_BASE_URL_ENV)? {
            self.hosted.deepseek_base_url =
                Some(checked_url(url).map_err(named(DEEPSEEK_BASE_URL_ENV))?);
            self.pinned.chat = true;
        }
        if let Some(url) = given(SPEECH_BASE_URL_ENV)? {
            self.tts_base_url = checked_url(url).map_err(named(SPEECH_BASE_URL_ENV))?;
            self.pinned.speech = true;
        }
        if let Some(url) = given(LISTENING_BASE_URL_ENV)? {
            let url = checked_url(url).map_err(named(LISTENING_BASE_URL_ENV))?;
            if let Stt::Autonomous { base_url, .. } = &mut self.stt {
                *base_url = url;
            }
            self.pinned.listening = true;
        }
        // The chosen voice's model: Gemini's, or the configured voice's
        // (ElevenLabs, OpenAI); only the voice in use reads its own (#1712:
        // the owner, "use v4 turbo").
        if let Some(model) = given(SPEECH_MODEL_ENV)? {
            let model = gemini_model(model).map_err(named(SPEECH_MODEL_ENV))?;
            self.gemini.speech_model = model.clone();
            self.tts_model = Some(model);
        }
        self = self.with_key_terms_from(&var)?;
        self.stt_flux = var(FLUX_ENV).is_some_and(|value| value == "on");
        Ok(self)
    }
}

impl SecondConfig {
    /// [`KEY_TERMS_ENV`] as `var` gives it, over the file's; unset or empty,
    /// the file's stay. Refused naming the variable.
    fn with_key_terms_from(mut self, var: impl Fn(&str) -> Option<OsString>) -> io::Result<Self> {
        if let Some(terms) = var(KEY_TERMS_ENV).filter(|value| !value.is_empty()) {
            let terms = terms.into_string().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{KEY_TERMS_ENV} is not Unicode"),
                )
            })?;
            self.stt_key_terms = key_terms(&terms).map_err(|error| {
                io::Error::new(error.kind(), format!("{KEY_TERMS_ENV}: {error}"))
            })?;
        }
        Ok(self)
    }
}

/// A service URL as the file's are taken: https, a host, no `?`, `#` or `@`,
/// at most 2048 bytes; the relay's near addresses are checked the same (#2882).
pub(crate) fn checked_url(url: String) -> io::Result<String> {
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
        assert!(from_setting(None, |_| None).unwrap().is_none());
        assert!(
            from_setting(Some(OsString::new()), |_| None)
                .unwrap()
                .is_none()
        );
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
        let error = load_from_path(&config).unwrap_err();
        assert!(error.to_string().contains("llm_api_key"), "{error}");
        assert!(!error.to_string().contains("sk-inline"));
    }

    /// A key pasted into the wrong field is refused, and neither the error
    /// nor a journal line quotes it (#2061): serde's kind and place only.
    #[test]
    fn a_key_pasted_into_a_known_field_is_never_quoted() {
        for (name, extra) in [
            (
                "planted-provider",
                r#","tts_provider":"sk-test-PLANTED-0000""#,
            ),
            ("planted-extra", r#","llm_extra":"sk-test-PLANTED-0000""#),
        ] {
            let (_, config) = written(name, extra);
            let error = load_reporting(&config).err().unwrap();
            assert!(!format!("{error} {error:?}").contains("PLANTED"), "{error}");
        }
        let (_, config) = written(
            "planted-extra-place",
            r#","llm_extra":"sk-test-PLANTED-0000""#,
        );
        let error = load_from_path(&config).unwrap_err().to_string();
        let (why, column) = error.rsplit_once(" column ").unwrap();
        assert!(why.ends_with(": Data at line 1"), "{error}");
        assert!(column.parse::<usize>().is_ok(), "{error}");
    }

    /// A field a newer build writes is read past and named once at start
    /// (#1505, #1680); a known field with a bad value is still refused.
    #[test]
    fn a_field_from_a_newer_build_is_read_past_and_named() {
        let (_, config) = written("newer", r#","tts_speed":1.2,"newer":{"a":[1]}"#);
        let (route, line) = load_reporting(&config).unwrap();
        assert_eq!(route.llm_model, "m");
        assert_eq!(
            line.unwrap(),
            format!(
                "{}: fields this build does not read, ignored: newer, tts_speed",
                config.display()
            )
        );
        let (_, plain) = written("plain", "");
        assert_eq!(load_reporting(&plain).unwrap().1, None);
        let (_, bad) = written("newer-bad", r#","newer":1,"stt_language":7"#);
        let error = load_from_path(&bad).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains(bad.to_str().unwrap()), "{error}");
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
            (SPEECH_BASE_URL_ENV, "http://plain.example.invalid"),
            (LISTENING_BASE_URL_ENV, "https://relay.example.invalid/#x"),
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

    /// #2882: speech's and listening's own addresses go over the file's,
    /// unset or empty leave the file's, and listening's leaves Deepgram's.
    #[test]
    fn the_units_speech_and_listening_addresses_go_over_the_files() {
        let (_, config) = written("speech-listening-env", "");
        let route = || load_from_path(&config).unwrap();
        // Another host than the company relay's: a complete API base.
        let near = "https://near.example.invalid/api/v1/ai/v1";
        let set = route()
            .with_environment(|name| {
                [SPEECH_BASE_URL_ENV, LISTENING_BASE_URL_ENV]
                    .contains(&name)
                    .then(|| near.into())
            })
            .unwrap();
        assert_eq!(set.tts_base_url, near);
        assert!(matches!(&set.stt, Stt::Autonomous { base_url, .. } if base_url == near));
        assert_ne!(route().tts_base_url, near);
        assert!(matches!(&route().stt, Stt::Autonomous { base_url, .. } if base_url != near));
        let unset = route().with_environment(|_| Some("".into())).unwrap();
        assert_eq!(unset.tts_base_url, route().tts_base_url);
        assert!(
            matches!((&unset.stt, &route().stt), (Stt::Autonomous { base_url: a, .. }, Stt::Autonomous { base_url: b, .. }) if a == b)
        );
        let mut deepgram = route();
        deepgram.stt = Stt::Deepgram {
            key: Key("deepgram-test-key".into()),
        };
        let kept = deepgram
            .with_environment(|name| (name == LISTENING_BASE_URL_ENV).then(|| near.into()))
            .unwrap();
        assert!(matches!(kept.stt, Stt::Deepgram { .. }));
    }

    /// #2882: each address the unit's environment gives is pinned, so the
    /// relay's near address never goes over it; unset or empty, none is.
    #[test]
    fn the_units_addresses_are_pinned_and_only_those() {
        let (_, config) = written("pinned-env", "");
        let route = || load_from_path(&config).unwrap();
        assert_eq!(route().pinned, Pinned::default());
        let empty = route().with_environment(|_| Some("".into())).unwrap();
        assert_eq!(empty.pinned, Pinned::default());
        for (name, pinned) in [
            (
                GEMINI_BASE_URL_ENV,
                Pinned {
                    gemini: true,
                    ..Pinned::default()
                },
            ),
            (
                DEEPSEEK_BASE_URL_ENV,
                Pinned {
                    chat: true,
                    ..Pinned::default()
                },
            ),
            (
                SPEECH_BASE_URL_ENV,
                Pinned {
                    speech: true,
                    ..Pinned::default()
                },
            ),
            (
                LISTENING_BASE_URL_ENV,
                Pinned {
                    listening: true,
                    ..Pinned::default()
                },
            ),
        ] {
            let set = route()
                .with_environment(|asked| {
                    (asked == name).then(|| "https://near.example.invalid/ai".into())
                })
                .unwrap();
            assert_eq!(set.pinned, pinned, "{name}");
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
                own_deepseek: None,
                relay_deepseek_base_url: None,
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

    /// #2094: the person's keys, absent, present and removed, give the
    /// relay, their own and the relay again; off, they are never read; one
    /// that can't be read is passed over; no line holds a key or the URL.
    /// El's P1 on #2978: the product's own `second.json`, as `lamp-update`
    /// places it (the file a round keeps from an older updater is the same),
    /// reaches DeepSeek through the company relay with no
    /// `LAMP_SECOND_DEEPSEEK_BASE_URL`, and its chat may be routed near
    /// (#2882). An own key over it keeps the relay to fall back to (El's
    /// P2). Another host gets nothing; a `deepseek_base_url` or the
    /// variable goes over the default.
    #[test]
    fn the_company_relay_reaches_deepseek_through_itself() {
        const RELAY: &str = "https://campaign-api.autonomous.ai/api/v1/ai/v1/deepseek";
        assert_eq!(
            relay_deepseek("https://campaign-api.autonomous.ai/api/v1/ai").as_deref(),
            Some(RELAY)
        );
        assert_eq!(
            relay_deepseek("https://campaign-api.autonomous.ai/api/v1/ai/").as_deref(),
            Some(RELAY)
        );
        assert_eq!(relay_deepseek("https://llm.example.invalid/v1"), None);
        assert_eq!(
            relay_deepseek("http://campaign-api.autonomous.ai/api/v1/ai"),
            None
        );
        assert_eq!(
            relay_deepseek("https://campaign-api.autonomous.ai.example.invalid/ai"),
            None
        );
        // The product's file, its key at a test path.
        let folder = directory("product-relay-deepseek");
        let llm = folder.join("credential");
        private(&llm, "relay-secret\n");
        let product = include_str!("../../../deploy/product/settings/second.json")
            .replace("/etc/lampOS/voice/credential", &llm.display().to_string());
        let file = folder.join("second.json");
        private(&file, &product);
        let route = load_from_path(&file).unwrap();
        assert_eq!(route.hosted.deepseek_base_url.as_deref(), Some(RELAY));
        assert_eq!(route.pinned, Pinned::default());
        let deepseek = crate::second_provider::worker::RouteFlags {
            chat: Some(crate::second_provider::worker::ChatRoute::DeepSeek),
            ..Default::default()
        };
        assert!(crate::second_provider::worker::relay_routable(&route, deepseek, None).chat);
        // Its key is the relay's, with no deepseek.key (El's P3).
        assert_eq!(
            crate::second_provider::worker::deepseek_key(&route, None)
                .unwrap()
                .expose(),
            "relay-secret"
        );
        // The product unit, which no longer gives the variable.
        let unit = include_str!("../../../deploy/product/lamp-voice.service");
        assert!(
            !unit.contains(&format!("{DEEPSEEK_BASE_URL_ENV}=")),
            "{unit}"
        );
        // An own key over it: the relay to fall back to.
        let units = directory("product-relay-own");
        private(&units.join(OWN_DEEPSEEK_KEY), "sk-own-2978-000000000000\n");
        let (own, _) = route.clone().with_own_keys(true, Some(units.as_os_str()));
        assert_eq!(
            own.hosted.deepseek_base_url.as_deref(),
            Some(DEEPSEEK_BASE_URL)
        );
        assert_eq!(own.hosted.relay_deepseek_base_url.as_deref(), Some(RELAY));
        // A file's own address, or the variable, goes over the default.
        let named = product.replace(
            "\"tts_provider\"",
            "\"deepseek_base_url\":\"https://other.example.invalid/deepseek\",\"tts_provider\"",
        );
        private(&file, &named);
        assert_eq!(
            load_from_path(&file)
                .unwrap()
                .hosted
                .deepseek_base_url
                .as_deref(),
            Some("https://other.example.invalid/deepseek")
        );
        let pinned = route
            .with_environment(|name| {
                (name == DEEPSEEK_BASE_URL_ENV)
                    .then(|| "https://pinned.example.invalid/deepseek".into())
            })
            .unwrap();
        assert_eq!(
            pinned.hosted.deepseek_base_url.as_deref(),
            Some("https://pinned.example.invalid/deepseek")
        );
        assert!(pinned.pinned.chat);
    }

    #[test]
    fn the_persons_own_keys_are_used_only_with_the_switch_and_never_shown() {
        let planted = "sk-planted-2094-0000000000000000";
        let (_, config) = written("own-keys", "");
        let relay = load_from_path(&config).unwrap();
        let units = directory("own-keys-units");
        let dir = Some(units.as_os_str());
        // Absent: the relay.
        let (absent, lines) = relay.clone().with_own_keys(true, dir);
        assert_eq!(absent, relay);
        assert!(
            lines.iter().all(|line| line.contains("the relay")),
            "{lines:?}"
        );
        // Present.
        private(&units.join(OWN_DEEPSEEK_KEY), &format!("{planted}\n"));
        private(&units.join(OWN_DEEPGRAM_KEY), planted);
        let (own, lines) = relay.clone().with_own_keys(true, dir);
        assert_eq!(own.hosted.own_deepseek.as_ref().unwrap().expose(), planted);
        assert_eq!(
            own.hosted.deepseek_base_url.as_deref(),
            Some(DEEPSEEK_BASE_URL)
        );
        // No relay was set in this file: none to fall back to.
        assert_eq!(own.hosted.relay_deepseek_base_url, None);
        assert!(matches!(&own.stt, Stt::Deepgram { key } if key.expose() == planted));
        assert_eq!(
            lines,
            [
                "DeepSeek: the person's own key, at api.deepseek.com (LAMP_OWN_KEYS)",
                "Deepgram: the person's own key (LAMP_OWN_KEYS)"
            ]
        );
        assert!(!format!("{own:?}").contains(planted));
        // Their endpoint, taken; one that is not https is not.
        private(
            &units.join(OWN_DEEPSEEK_URL),
            "https://deepseek.example.invalid/v1\n",
        );
        let mut relayed = relay.clone();
        relayed.hosted.deepseek_base_url = Some("https://relay.example.invalid/deepseek".into());
        let (own, lines) = relayed.with_own_keys(true, dir);
        assert_eq!(
            own.hosted.deepseek_base_url.as_deref(),
            Some("https://deepseek.example.invalid/v1")
        );
        assert_eq!(
            own.hosted.relay_deepseek_base_url.as_deref(),
            Some("https://relay.example.invalid/deepseek")
        );
        // The host by name, never its path.
        assert_eq!(
            lines[0],
            "DeepSeek: the person's own key, at deepseek.example.invalid (LAMP_OWN_KEYS)"
        );
        private(
            &units.join(OWN_DEEPSEEK_URL),
            "http://deepseek.example.invalid",
        );
        let (passed, lines) = relay.clone().with_own_keys(true, dir);
        assert_eq!(passed.hosted, relay.hosted);
        assert!(lines[0].contains("could not be read"), "{lines:?}");
        assert!(
            !lines.join(" ").contains("deepseek.example.invalid"),
            "{lines:?}"
        );
        fs::remove_file(units.join(OWN_DEEPSEEK_URL)).unwrap();
        // Off: never read.
        let (off, lines) = relay.clone().with_own_keys(false, dir);
        assert_eq!(off, relay);
        assert!(lines.is_empty());
        // Not private: passed over for the relay, never shown.
        fs::set_permissions(
            units.join(OWN_DEEPGRAM_KEY),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        let (open, lines) = relay.clone().with_own_keys(true, dir);
        assert_eq!(open.stt, relay.stt);
        assert!(lines[1].contains("could not be read"), "{lines:?}");
        // Removed: the relay again.
        fs::remove_file(units.join(OWN_DEEPSEEK_KEY)).unwrap();
        fs::remove_file(units.join(OWN_DEEPGRAM_KEY)).unwrap();
        let (removed, lines) = relay.clone().with_own_keys(true, dir);
        assert_eq!(removed, relay);
        for line in &lines {
            assert!(
                line.contains("the relay") && !line.contains(planted),
                "{line}"
            );
        }
        // No unit credentials at all: the relay.
        let (none, _) = relay.clone().with_own_keys(true, None);
        assert_eq!(none, relay);
    }

    /// #2094, #2846: the product's voice unit turns the switch on and loads
    /// the keys' folder as `own`, so each file is the `own_<file>` read here,
    /// the files root's helper writes.
    #[test]
    fn the_product_unit_loads_the_own_keys_folder_the_names_here_read() {
        let product = include_str!("../../../deploy/product/lamp-voice.service");
        assert!(product.contains(&format!("\nEnvironment={OWN_KEYS_ENV}=on\n")));
        assert!(product.contains("\nLoadCredential=own:/etc/lampOS/voice/own\n"));
        assert!(product.contains("\nConfigurationDirectory=lampOS/voice/own\n"));
        for (name, file) in [
            (OWN_DEEPSEEK_KEY, "deepseek.key"),
            (OWN_DEEPSEEK_URL, "deepseek.url"),
            (OWN_DEEPGRAM_KEY, "deepgram.key"),
        ] {
            assert_eq!(name, format!("own_{file}"));
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

    /// #1940, the reviewer's P1 at 00:09: an update round brings the
    /// second route's unit without `second.json`, so the voice places the
    /// product's: only for `second` at the product's path, only when
    /// nothing is there, private, and loadable at once.
    #[test]
    fn the_products_configuration_is_placed_only_when_the_route_asks_and_none_is_there() {
        use std::ffi::OsStr;
        let directory = directory("placed");
        let product = directory.join("second.json");
        let at = Some(product.as_os_str());
        let second = Some(OsStr::new("second"));
        // Not asked: another route, no route, or another path.
        for (route, configured) in [
            (Some(OsStr::new("gemini")), at),
            (None, at),
            (second, Some(OsStr::new("/elsewhere/second.json"))),
            (second, None),
        ] {
            assert!(!place_if_missing(route, configured, &product, PRODUCT).unwrap());
            assert!(!product.exists());
        }
        // Asked and missing: placed whole, 0600, a leftover half-write gone.
        fs::write(directory.join(".second.json.placing"), "{").unwrap();
        assert!(place_if_missing(second, at, &product, PRODUCT).unwrap());
        assert_eq!(fs::read_to_string(&product).unwrap(), PRODUCT);
        assert_eq!(
            fs::metadata(&product).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(!directory.join(".second.json.placing").exists());
        // The voice can read it as it reads its own.
        assert!(read_private(&product, 16_384).is_ok());
        // Already there, even another: left alone.
        private(&product, "{\"llm_base_url\":\"https://x.example.invalid\"}");
        assert!(!place_if_missing(second, at, &product, PRODUCT).unwrap());
        assert!(
            fs::read_to_string(&product)
                .unwrap()
                .contains("x.example.invalid")
        );
        assert_eq!(PRODUCT_PATH, "/etc/lampOS/voice/second.json");
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
