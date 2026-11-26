//! A person's own DeepSeek and Deepgram keys (#2094, `docs/own-keys.md`):
//! given at setup or from the app, checked once against the service before
//! they are kept, and kept only in the voice's credential folder.
//!
//! Off unless `LAMP_OWN_KEYS=on`. Root writes them: setup itself, which runs
//! as root, and after setup `lamp-setup keys`, root's helper on its own
//! socket (`lamp-setup-keys.socket`), which `lamp-cloud` asks for the app.
//! It has the network the check needs, which the names' helper does not.
//!
//! A key is in the request line, the check's header and its file, nowhere
//! else: never a reply, an error or a journal line. [`Asked`]'s `Debug`
//! hides it, and the check's error is said by its kind alone.
use crate::serve::write_private;
use serde_json::{Value, json};
use std::{
    fmt, fs, io,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

/// The switch, on setup, `lamp-cloud` and the voice. The helper does not
/// read it: its unit passes `--enable`, and it is closed while the switch is
/// off because its units are not installed, `lamp-cloud` answers "not on
/// this lamp" and only `lamp-cloud` may ask (the reviewer's P3 3 on #2167).
pub const ENVIRONMENT: &str = "LAMP_OWN_KEYS";

/// Whether the switch is on, as `var` gives it.
pub fn on(var: Option<&str>) -> bool {
    var == Some("on")
}

/// The folder the voice loads whole (`LoadCredential=own:<folder>`).
pub const FOLDER: &str = "/etc/lampOS/voice/own";

/// The longest request line read, in bytes.
pub const LINE_MOST: usize = 4_096;
/// The longest key kept, in bytes.
pub const KEY_MOST: usize = 512;
/// How long one check may take, its connection included.
pub const CHECK_WITHIN: Duration = Duration::from_secs(5);
/// DeepSeek's own API, when the person names no endpoint.
pub const DEEPSEEK_BASE_URL: &str = lamp_msgs::own_model::DEEPSEEK_API;
/// Deepgram's call that answers 200 for a key it takes, and nothing more.
pub const DEEPGRAM_CHECK_URL: &str = "https://api.deepgram.com/v1/auth/token";
/// ElevenLabs' API: the check asks `<it>/voices/<voice_id>` (#8367).
pub const ELEVENLABS_API: &str = "https://api.elevenlabs.io/v1";
/// OpenRouter's API (#8420): fixed, so its key goes nowhere else. The check
/// asks `<it>/key` and then `<it>/models/<model>/endpoints`.
pub const OPENROUTER_API: &str = lamp_msgs::own_model::OPENROUTER_API;
/// Gemini's OpenAI-compatible API (#8420): fixed, so its key goes nowhere
/// else. The check asks `<it>/models/<model>`.
pub const GEMINI_API: &str = lamp_msgs::own_model::GEMINI_OPENAI_API;
/// Anthropic's API (#8420): fixed, so its key goes nowhere else. The check
/// asks `<it>/models/<model>` with [`ANTHROPIC_VERSION`]; the voice chats at
/// its OpenAI-compatible `<it>/chat/completions`.
pub const ANTHROPIC_API: &str = lamp_msgs::own_model::ANTHROPIC_API;
/// The `anthropic-version` its model read is asked with.
pub const ANTHROPIC_VERSION: &str = "2023-06-01";
/// The most an OpenAI-compatible endpoint's model list is read (#8362).
pub const MODELS_MOST: usize = 1 << 20;
/// How long the agent's tool check may take (#8735), after the key's own
/// check: with [`CHECK_WITHIN`] still under `lamp-cloud`'s 15 s wait.
pub const TOOLS_WITHIN: Duration = Duration::from_secs(6);
/// The tool check's `max_tokens`: a call to a tool with no arguments, and
/// Gemini's minimal thinking before it.
pub const TOOLS_MAX_TOKENS: u32 = 256;
/// The model DeepSeek's own key is asked for, as the voice asks it
/// (`lamp_voice::second_config::DEEPSEEK_MODEL`).
pub const DEEPSEEK_MODEL: &str = "deepseek-flash";
/// The longest voice ID taken: ElevenLabs' are 20 letters and digits.
pub const VOICE_ID_MOST: usize = 64;
/// The person's ElevenLabs voice ID, kept beside its key.
pub const ELEVENLABS_VOICE_FILE: &str = "elevenlabs.voice";
/// The unit the helper restarts at a quiet moment, so the voice takes it.
pub const VOICE_UNIT: &str = "lamp-voice.service";

/// The one unit whose processes may ask the helper (Third's privacy read on
/// #2094): `lamp-cloud`, which carries the app's request. It runs as
/// `lampos` like the other parts, so its user can't tell it apart; its
/// cgroup can, which only root can move a process into.
pub const ASKER_UNIT: &str = "lamp-cloud.service";

/// Where the system's manager puts [`ASKER_UNIT`] (`Slice=lampos-parts.slice`
/// in its product unit): the asker's cgroup is this or one under it, never a
/// `lamp-cloud.service` elsewhere, such as one a user's own manager could
/// make under `user@<uid>.service` (the reviewer's P3 1 on #2167).
pub const ASKER_CGROUP: &str = "/lampos.slice/lampos-parts.slice/lamp-cloud.service";

/// The refusal to any other peer, before its line is read.
pub const NOT_THE_ASKER: &str = "only the app's link may change the lamp's keys";

/// The one other unit that may ask the change helper, and only for the
/// turn-off (#5747): `lamp-skills`, whose unplug turns the lamp off after a
/// confirmed fold and a spoken yes (unplug.md). Told apart from
/// `lamp-cloud` by its cgroup, as [`ASKER_UNIT`] is.
pub const TURN_OFF_UNIT: &str = "lamp-skills.service";

/// Where the system's manager puts [`TURN_OFF_UNIT`]
/// (`Slice=lampos-parts.slice` in its product unit), as [`ASKER_CGROUP`].
pub const TURN_OFF_CGROUP: &str = "/lampos.slice/lampos-parts.slice/lamp-skills.service";

/// Whether `/proc/<pid>/cgroup`'s text puts the process in [`ASKER_UNIT`]
/// at the system's place for it, [`ASKER_CGROUP`] (or a cgroup under it):
/// the unified hierarchy's line (`0::…`), or systemd's own on a hybrid one
/// (`…:name=systemd:…`).
pub fn from_asker(cgroup: &str) -> bool {
    in_cgroup(cgroup, ASKER_CGROUP)
}

/// [`from_asker`] for [`TURN_OFF_UNIT`] at [`TURN_OFF_CGROUP`].
pub fn from_turn_off_asker(cgroup: &str) -> bool {
    in_cgroup(cgroup, TURN_OFF_CGROUP)
}

fn in_cgroup(cgroup: &str, place: &str) -> bool {
    cgroup.lines().any(|line| {
        let mut fields = line.splitn(3, ':');
        let (Some(_), Some(controllers), Some(path)) =
            (fields.next(), fields.next(), fields.next())
        else {
            return false;
        };
        (controllers.is_empty() || controllers == "name=systemd")
            && path
                .strip_prefix(place)
                .is_some_and(|below| below.is_empty() || below.starts_with('/'))
    })
}

/// The cgroup text of the peer on standard input (the socket systemd hands
/// over), by its credentials (`SO_PEERCRED`). Peer credentials are read on
/// Linux only; elsewhere there is no peer's cgroup.
fn peer_cgroup() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let stdin = std::io::stdin();
        let peer = rustix::net::sockopt::socket_peercred(&stdin).ok()?;
        fs::read_to_string(format!("/proc/{}/cgroup", peer.pid.as_raw_nonzero())).ok()
    }
    #[cfg(not(target_os = "linux"))]
    None
}

/// The peer on standard input as the kernel names it (`SO_PEERCRED`): its
/// uid, and its cgroup's text read now from `/proc/<pid>/cgroup`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Peer {
    pub uid: u32,
    pub cgroup: String,
}

/// [`Peer`] of standard input, when it is a socket with a peer whose
/// cgroup can still be read. Linux only; elsewhere none.
pub fn socket_peer() -> Option<Peer> {
    #[cfg(target_os = "linux")]
    {
        let stdin = std::io::stdin();
        let peer = rustix::net::sockopt::socket_peercred(&stdin).ok()?;
        let cgroup =
            fs::read_to_string(format!("/proc/{}/cgroup", peer.pid.as_raw_nonzero())).ok()?;
        Some(Peer {
            uid: peer.uid.as_raw(),
            cgroup,
        })
    }
    #[cfg(not(target_os = "linux"))]
    None
}

/// Whether standard input is a socket (a helper on a socket systemd hands
/// over), not the helper run by hand.
pub fn stdin_is_socket() -> bool {
    let stdin = std::io::stdin();
    rustix::fs::fstat(&stdin).is_ok_and(|stat| {
        rustix::fs::FileType::from_raw_mode(stat.st_mode) == rustix::fs::FileType::Socket
    })
}

/// Whether the peer on standard input is in [`ASKER_UNIT`], by its
/// credentials and its cgroup.
pub fn peer_is_asker() -> bool {
    peer_cgroup().is_some_and(|cgroup| from_asker(&cgroup))
}

/// Whether the peer on standard input is in [`TURN_OFF_UNIT`], by its
/// credentials and its cgroup.
pub fn peer_is_turn_off_asker() -> bool {
    peer_cgroup().is_some_and(|cgroup| from_turn_off_asker(&cgroup))
}

/// Whether a helper on a socket systemd hands over (the change, Wi-Fi and
/// keys helpers' `Accept=yes` units) must refuse its peer before reading its
/// line: standard input is a socket and its peer isn't in [`ASKER_UNIT`].
/// Standard input that isn't a socket is the helper run by hand, by root,
/// which already may do anything the helper does.
pub fn socket_peer_refused() -> bool {
    let stdin = std::io::stdin();
    let socket = rustix::fs::fstat(&stdin).is_ok_and(|stat| {
        rustix::fs::FileType::from_raw_mode(stat.st_mode) == rustix::fs::FileType::Socket
    });
    socket && !peer_is_asker()
}

/// A service a person may bring their own key for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Service {
    DeepSeek,
    Deepgram,
    /// The lamp's voice (#8367): a key and a voice ID.
    ElevenLabs,
    /// A person's own model at any OpenAI-compatible endpoint, Grok and the
    /// like (#8362 step 1b): a key, its endpoint and a model.
    OpenAiCompatible,
    /// A person's own model at OpenRouter, by name (#8420): a key and a
    /// model, at [`OPENROUTER_API`] alone.
    OpenRouter,
    /// A person's own Gemini model, by name (#8420): a key and a model, at
    /// [`GEMINI_API`] alone.
    Gemini,
    /// A person's own Anthropic model, by name (#8420): a key and a model,
    /// at [`ANTHROPIC_API`] alone.
    Anthropic,
}

impl Service {
    pub const ALL: [Service; 7] = [
        Service::DeepSeek,
        Service::Deepgram,
        Service::ElevenLabs,
        Service::OpenAiCompatible,
        Service::OpenRouter,
        Service::Gemini,
        Service::Anthropic,
    ];

    /// The services that are the lamp's model: one own key among them is
    /// kept at a time, so the model in use is never a guess (#8362). In
    /// `lamp_msgs::own_model::PRECEDENCE`'s order, which the voice and
    /// lamp-cloud's forwarder take (a test holds them the same, #8735).
    pub const MODELS: [Service; 5] = [
        Service::OpenAiCompatible,
        Service::OpenRouter,
        Service::Gemini,
        Service::Anthropic,
        Service::DeepSeek,
    ];

    /// The name the app and the reply use.
    pub fn id(self) -> &'static str {
        match self {
            Self::DeepSeek => "deepseek",
            Self::Deepgram => "deepgram",
            Self::ElevenLabs => "elevenlabs",
            Self::OpenAiCompatible => "openai_compatible",
            Self::OpenRouter => "openrouter",
            Self::Gemini => "gemini",
            Self::Anthropic => "anthropic",
        }
    }

    /// The name a person reads.
    pub fn name(self) -> &'static str {
        match self {
            Self::DeepSeek => "DeepSeek",
            Self::Deepgram => "Deepgram",
            Self::ElevenLabs => "ElevenLabs",
            Self::OpenAiCompatible => "OpenAI-compatible",
            Self::OpenRouter => "OpenRouter",
            Self::Gemini => "Gemini",
            Self::Anthropic => "Anthropic",
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|service| service.id() == id)
    }

    /// The key's file in the folder.
    pub fn key_file(self) -> &'static str {
        match self {
            Self::DeepSeek => "deepseek.key",
            Self::Deepgram => "deepgram.key",
            Self::ElevenLabs => "elevenlabs.key",
            Self::OpenAiCompatible => "openai_compatible.key",
            Self::OpenRouter => "openrouter.key",
            Self::Gemini => "gemini.key",
            Self::Anthropic => "anthropic.key",
        }
    }

    /// The endpoint's file beside the key, for a service that takes one.
    pub fn url_file(self) -> Option<&'static str> {
        match self {
            Self::DeepSeek => Some(DEEPSEEK_URL_FILE),
            Self::OpenAiCompatible => Some("openai_compatible.url"),
            Self::Deepgram
            | Self::ElevenLabs
            | Self::OpenRouter
            | Self::Gemini
            | Self::Anthropic => None,
        }
    }

    /// The model's file beside the key, for a service that is given one.
    pub fn model_file(self) -> Option<&'static str> {
        match self {
            Self::OpenAiCompatible => Some("openai_compatible.model"),
            Self::OpenRouter => Some("openrouter.model"),
            Self::Gemini => Some("gemini.model"),
            Self::Anthropic => Some("anthropic.model"),
            Self::DeepSeek | Self::Deepgram | Self::ElevenLabs => None,
        }
    }

    /// A named model provider's own endpoint (#8420): fixed, never the
    /// app's, so its key goes to that provider alone.
    pub fn endpoint(self) -> Option<&'static str> {
        match self {
            Self::OpenRouter => Some(OPENROUTER_API),
            Self::Gemini => Some(GEMINI_API),
            Self::Anthropic => Some(ANTHROPIC_API),
            _ => None,
        }
    }

    /// The last check's code (`works`, `refused`, `unreached`): never a key.
    pub fn checked_file(self) -> &'static str {
        match self {
            Self::DeepSeek => "deepseek.checked",
            Self::Deepgram => "deepgram.checked",
            Self::ElevenLabs => "elevenlabs.checked",
            Self::OpenAiCompatible => "openai_compatible.checked",
            Self::OpenRouter => "openrouter.checked",
            Self::Gemini => "gemini.checked",
            Self::Anthropic => "anthropic.checked",
        }
    }

    /// `keys.get`'s `offers` entry (#8362, #8367): `kind` sets the model
    /// services and the voice apart, `fields` the plain fields beside the
    /// key. No model list yet: the service's default. A named provider's
    /// fixed `endpoint` (#8420), for the app to show as it is.
    pub fn offer(self) -> Value {
        let mut offer = json!({
            "service": self.id(),
            "name": self.name(),
            "kind": if self == Self::ElevenLabs { "voice" } else { "model" },
            "models": [],
            "base_url": self.url_file().is_some(),
            "fields": match self {
                Self::ElevenLabs => json!(["voice_id"]),
                Self::OpenAiCompatible | Self::OpenRouter | Self::Gemini | Self::Anthropic => {
                    json!(["model"])
                }
                Self::DeepSeek | Self::Deepgram => json!([]),
            },
        });
        if let Some(endpoint) = self.endpoint() {
            offer["endpoint"] = json!(endpoint);
        }
        offer
    }
}

/// DeepSeek's endpoint, kept beside its key.
pub const DEEPSEEK_URL_FILE: &str = "deepseek.url";

/// One key given, checked for its shape. The key is never shown.
#[derive(Clone, PartialEq, Eq)]
pub struct Given {
    pub service: Service,
    pub key: String,
    /// DeepSeek's endpoint; Deepgram has none.
    pub base_url: Option<String>,
    /// ElevenLabs' voice ID (#8367), 1 to 64 letters and digits; the
    /// others have none.
    pub voice_id: Option<String>,
    /// The OpenAI-compatible or OpenRouter model (#8362, #8420); the others
    /// are given none.
    pub model: Option<String>,
}

impl fmt::Debug for Given {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Given")
            .field("service", &self.service)
            .field("key", &"<hidden>")
            .field("base_url", &self.base_url)
            .field("voice_id", &self.voice_id)
            .field("model", &self.model)
            .finish()
    }
}

/// What one request line asks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asked {
    /// Check the key and keep it if it works.
    Set(Given),
    /// ElevenLabs' voice alone, beside the key kept (#8367).
    Voice(String),
    /// "Use the lamp's own again": the key removed.
    Remove(Service),
    /// Whose key each service uses.
    Which,
}

/// A key's shape: 1 to 512 printable ASCII characters, trimmed.
pub fn fit_key(key: &str) -> Result<String, &'static str> {
    let key = key.trim();
    if key.is_empty() || key.len() > KEY_MOST || !key.bytes().all(|b| b.is_ascii_graphic()) {
        return Err("a key is 1 to 512 letters, digits and marks, with no spaces");
    }
    Ok(key.to_owned())
}

/// An endpoint: an explicit https URL with a host, no user, query or
/// fragment (the voice's own rule), with no trailing `/`.
pub fn fit_url(url: &str) -> Result<String, &'static str> {
    let url = url.trim().trim_end_matches('/');
    let host = url
        .strip_prefix("https://")
        .map(|rest| rest.split('/').next().unwrap_or(""));
    if url.len() > 2048
        || url.contains(['?', '#', '@'])
        || url.chars().any(|c| c.is_whitespace() || c.is_control())
        || host.is_none_or(str::is_empty)
    {
        return Err("the endpoint must be an https address");
    }
    Ok(url.to_owned())
}

/// A voice ID's shape: 1 to [`VOICE_ID_MOST`] ASCII letters and digits,
/// trimmed, so it is a path segment of the check and the voice's request.
pub fn fit_voice(id: &str) -> Result<String, &'static str> {
    let id = id.trim();
    if id.is_empty() || id.len() > VOICE_ID_MOST || !id.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err("a voice ID is 1 to 64 letters and digits");
    }
    Ok(id.to_owned())
}

/// A model's name, as the voice takes one in a request's body: 1 to 128
/// letters, digits, `-`, `.`, `_`, `/` and `:`.
pub fn fit_model(model: &str) -> Result<String, &'static str> {
    let model = model.trim();
    if model.is_empty()
        || model.len() > 128
        || !model
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._/:".contains(&b))
    {
        return Err("a model name is letters, digits, '-', '.', '_', '/' and ':'");
    }
    Ok(model.to_owned())
}

/// A model service's key as the app gives it, each part checked for its
/// shape: an OpenAI-compatible one's endpoint and model (#8362); a named
/// provider's model, at its own fixed endpoint, any endpoint sent refused
/// (#8420).
pub fn given_model(
    service: Service,
    key: &str,
    base_url: Option<&str>,
    model: Option<&str>,
) -> Result<Given, &'static str> {
    let key = fit_key(key)?;
    let sent = base_url.map(str::trim).filter(|url| !url.is_empty());
    let (base_url, model) = match (service.endpoint(), sent) {
        (Some(_), Some(_)) => {
            return Err(match service {
                Service::Gemini => "Gemini takes no endpoint",
                Service::Anthropic => "Anthropic takes no endpoint",
                _ => "OpenRouter takes no endpoint",
            });
        }
        (Some(endpoint), None) => (
            endpoint.to_owned(),
            model.ok_or(match service {
                Service::Gemini => "a Gemini key needs its model",
                Service::Anthropic => "an Anthropic key needs its model",
                _ => "an OpenRouter key needs its model",
            })?,
        ),
        (None, sent) => (
            fit_url(sent.ok_or("an OpenAI-compatible key needs its endpoint")?)?,
            model.ok_or("an OpenAI-compatible key needs its model")?,
        ),
    };
    let model = fit_model(model)?;
    Ok(Given {
        service,
        key,
        base_url: Some(base_url),
        voice_id: None,
        model: Some(model),
    })
}

/// A key as setup's request or the app gives it, checked for its shape.
pub fn given(service: Service, key: &str, base_url: Option<&str>) -> Result<Given, &'static str> {
    let key = fit_key(key)?;
    let base_url = match (
        service,
        base_url.map(str::trim).filter(|url| !url.is_empty()),
    ) {
        (Service::DeepSeek, Some(url)) => Some(fit_url(url)?),
        (Service::DeepSeek, None) => Some(DEEPSEEK_BASE_URL.to_owned()),
        (Service::Deepgram, None) => None,
        (Service::Deepgram, Some(_)) => return Err("Deepgram takes no endpoint"),
        (Service::ElevenLabs, None) => None,
        (Service::ElevenLabs, Some(_)) => return Err("ElevenLabs takes no endpoint"),
        (
            Service::OpenAiCompatible | Service::OpenRouter | Service::Gemini | Service::Anthropic,
            url,
        ) => {
            return given_model(service, &key, url, None);
        }
    };
    Ok(Given {
        service,
        key,
        base_url,
        voice_id: None,
        model: None,
    })
}

/// The one line `lamp-cloud` sends: `{"set": service, "key", "base_url"?,
/// "voice_id"?}`, ElevenLabs' `{"set": "elevenlabs", "voice_id"}` alone,
/// `{"remove": service}` or `{"which": true}`. Never echoes a key or a
/// voice ID.
pub fn parse(line: &str) -> Result<Asked, String> {
    if line.len() > LINE_MOST {
        return Err("the request is too long".into());
    }
    let value: Value = serde_json::from_str(line).map_err(|_| "the request is not JSON")?;
    let object = value.as_object().ok_or("the request is not an object")?;
    let service = |field: &str| -> Result<Service, String> {
        object
            .get(field)
            .and_then(Value::as_str)
            .and_then(Service::from_id)
            .ok_or_else(|| {
                "the service is deepseek, deepgram, elevenlabs, openai_compatible, openrouter, gemini or anthropic"
                    .to_owned()
            })
    };
    if object.contains_key("set") {
        if object
            .keys()
            .any(|field| !["set", "key", "base_url", "voice_id", "model"].contains(&field.as_str()))
        {
            return Err("a key is set with set, key, base_url and voice_id only".into());
        }
        let set = service("set")?;
        if set.model_file().is_none() && object.contains_key("model") {
            return Err("a key is set with set, key, base_url and voice_id only".into());
        }
        let voice_id = match (set, object.get("voice_id")) {
            (_, None | Some(Value::Null)) => None,
            (Service::ElevenLabs, Some(Value::String(id))) => Some(fit_voice(id)?),
            (Service::ElevenLabs, Some(_)) => {
                return Err("a voice ID is 1 to 64 letters and digits".into());
            }
            (_, Some(_)) => return Err("only ElevenLabs takes a voice ID".into()),
        };
        if set == Service::ElevenLabs && !object.contains_key("key") {
            return voice_id
                .map(Asked::Voice)
                .ok_or_else(|| "the key must be text".into());
        }
        let key = object
            .get("key")
            .and_then(Value::as_str)
            .ok_or("the key must be text")?;
        let base_url = match object.get("base_url") {
            None | Some(Value::Null) => None,
            Some(Value::String(url)) => Some(url.as_str()),
            Some(_) => return Err("the endpoint must be text".into()),
        };
        if set.model_file().is_some() {
            let model = match object.get("model") {
                None | Some(Value::Null) => None,
                Some(Value::String(model)) => Some(model.as_str()),
                Some(_) => return Err("the model must be text".into()),
            };
            return Ok(Asked::Set(given_model(set, key, base_url, model)?));
        }
        return Ok(Asked::Set(Given {
            voice_id,
            ..given(set, key, base_url)?
        }));
    }
    if object.len() != 1 {
        return Err("the request holds one change".into());
    }
    if object.contains_key("remove") {
        return Ok(Asked::Remove(service("remove")?));
    }
    if object.get("which") == Some(&Value::Bool(true)) {
        return Ok(Asked::Which);
    }
    Err("the request names nothing this lamp does with keys".into())
}

/// What the one check came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The service took the key: it is kept.
    Works,
    /// The service refused it (401 or 403): it is not kept.
    Refused,
    /// ElevenLabs took the key but has no such voice on its account (400
    /// or 404, #8367): nothing is kept.
    VoiceRefused,
    /// No answer that says either: it is not kept, and nothing changes.
    Unreached,
    /// The service took it but it could not be written (a full or failing
    /// disk): the person's key for that service is removed as far as it can
    /// be, so the lamp uses its own, and the voice is restarted to take that
    /// (the reviewer's P3 2 on #2167).
    NotKept,
    /// The OpenAI-compatible endpoint took the key but lists no model by the
    /// name given (#8362): not kept, and its list is never said back.
    NoModel,
    /// A model service took the key, but its chat made no tool call (#8735):
    /// kept for the voice, while the agent stays on the relay.
    WorksNoTools,
}

impl Verdict {
    /// The plain answer, for the person. A key that works says where the
    /// person's words go now, by host (Third's privacy read on #2094).
    pub fn words(self, given: &Given) -> String {
        let service = given.service;
        match self {
            Self::Works if service == Service::ElevenLabs => {
                format!("that key and voice work: {}", goes_where(given))
            }
            Self::Works => format!("that key works: {}", goes_where(given)),
            Self::WorksNoTools => format!(
                "that key works for the lamp's voice: {}. It made no tool call, so the lamp's agent stays on the lamp's own model",
                goes_where(given)
            ),
            Self::Refused => "that key was refused".into(),
            Self::VoiceRefused => {
                "that key works, but that voice ID isn't on its ElevenLabs account; nothing changed"
                    .into()
            }
            Self::Unreached if service == Service::OpenAiCompatible => {
                "the lamp couldn't reach that endpoint to check that key; nothing changed".into()
            }
            Self::NoModel if service == Service::OpenAiCompatible => {
                "that endpoint has no model by that name".into()
            }
            Self::NoModel => format!("{} has no model by that name", service.name()),
            Self::Unreached => format!(
                "the lamp couldn't reach {} to check that key; nothing changed",
                service.name()
            ),
            Self::NotKept => "the key could not be kept; the lamp uses its own now".into(),
        }
    }

    /// The answer's code in the contract (#2094, El 11:1x): `works`,
    /// `refused`, `unreached` or `not_kept`; `works_no_tools` (#8735).
    pub fn code(self) -> &'static str {
        match self {
            Self::Works => "works",
            Self::WorksNoTools => "works_no_tools",
            Self::Refused => "refused",
            Self::VoiceRefused => "voice_refused",
            Self::Unreached => "unreached",
            Self::NotKept => "not_kept",
            Self::NoModel => "bad_field",
        }
    }

    /// From the service's HTTP status.
    pub fn of_status(status: u16) -> Self {
        match status {
            200..=299 => Self::Works,
            401 | 403 => Self::Refused,
            _ => Self::Unreached,
        }
    }
}

/// The endpoint's host, which the person is told and the journal names:
/// never a path, a key or a port's absence guessed.
pub fn host(url: &str) -> &str {
    url.split_once("://")
        .map_or(url, |(_, rest)| rest)
        .split('/')
        .next()
        .unwrap_or("")
}

/// What goes where with the person's own key, by name.
pub fn goes_where(given: &Given) -> String {
    match given.service {
        Service::DeepSeek
        | Service::OpenAiCompatible
        | Service::OpenRouter
        | Service::Gemini
        | Service::Anthropic => format!(
            "your conversations now go to {} with your key: your words, what the lamp remembers that's sent with them, and what it sees when asked to look",
            host(given.base_url.as_deref().unwrap_or(DEEPSEEK_BASE_URL))
        ),
        Service::Deepgram => {
            "your speech now goes to Deepgram under your account and its retention terms".into()
        }
        Service::ElevenLabs => "the lamp now speaks in your ElevenLabs voice, and what it says goes to ElevenLabs under your account and its terms".into(),
    }
}

/// The check, injected for tests.
pub trait Check {
    fn check(&mut self, given: &Given) -> Verdict;
}

/// TLS 1.2 and 1.3 with the `ring` provider and the webpki roots, as the
/// other lampOS clients have it.
fn tls() -> Option<rustls::ClientConfig> {
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    Some(
        rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .ok()?
            .with_root_certificates(roots)
            .with_no_client_auth(),
    )
}

/// The real check: one request, [`CHECK_WITHIN`] at most, no redirect.
/// DeepSeek: `GET <endpoint>/models` with the key as the bearer; Deepgram:
/// `GET` [`DEEPGRAM_CHECK_URL`] with `Token <key>`; ElevenLabs: `GET`
/// [`ELEVENLABS_API`]`/voices/<voice_id>` with `xi-api-key`, which tries
/// the key and the voice at once (#8367).
pub struct HttpCheck {
    /// Deepgram's address, a test's local server in place of it.
    pub deepgram: String,
    /// ElevenLabs' API root, a test's local server in place of it.
    pub elevenlabs: String,
    /// A test lamp's `LAMP_OWN_ELEVENLABS_URL`, read only under its gate
    /// (`lamp_msgs::own_voice`, #8394's P2): the check goes there, as the
    /// voice's speech will.
    pub switched: Option<String>,
    https_only: bool,
    /// Whether a model key's check also asks for a tool call (#8735).
    tools: bool,
}

impl Default for HttpCheck {
    fn default() -> Self {
        Self {
            deepgram: DEEPGRAM_CHECK_URL.to_owned(),
            elevenlabs: ELEVENLABS_API.to_owned(),
            switched: lamp_msgs::own_voice::switched_from_environment(),
            https_only: true,
            tools: true,
        }
    }
}

impl HttpCheck {
    /// A check that may ask a plain local server: tests only.
    #[cfg(test)]
    fn local(deepgram: String) -> Self {
        Self {
            elevenlabs: deepgram.clone(),
            deepgram,
            switched: None,
            https_only: false,
            tools: false,
        }
    }

    fn client(&self) -> Option<reqwest::Client> {
        reqwest::Client::builder()
            .tls_backend_preconfigured(tls()?)
            .https_only(self.https_only)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(CHECK_WITHIN)
            .build()
            .ok()
    }

    /// The agent's tool check (#8735): [`tools_asked`] once, with the key as
    /// the bearer, as the voice chats. Whether a tool call came back.
    async fn calls_tools(&self, given: &Given) -> Option<bool> {
        let (url, body) = tools_asked(given);
        let response = self
            .client()?
            .post(url)
            .bearer_auth(&given.key)
            .header("content-type", "application/json")
            .body(body.to_string())
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return Some(false);
        }
        let reply: Value = serde_json::from_slice(&read_most(response).await?).ok()?;
        Some(
            reply["choices"][0]["message"]["tool_calls"]
                .as_array()
                .is_some_and(|calls| !calls.is_empty()),
        )
    }

    async fn ask(&self, given: &Given) -> Option<Verdict> {
        let client = self.client()?;
        let request = match given.service {
            // OpenRouter's model list is public: `/key` alone refuses a key.
            Service::OpenRouter => client
                .get(format!(
                    "{}/key",
                    given.base_url.as_deref().unwrap_or(OPENROUTER_API)
                ))
                .bearer_auth(&given.key),
            // Gemini's one model, read with the key (#8420).
            Service::Gemini => client
                .get(format!(
                    "{}/models/{}",
                    given.base_url.as_deref().unwrap_or(GEMINI_API),
                    given.model.as_deref().unwrap_or_default()
                ))
                .bearer_auth(&given.key),
            // Anthropic's one model, read with its own headers: its model
            // read takes no bearer (#8420).
            Service::Anthropic => client
                .get(format!(
                    "{}/models/{}",
                    given.base_url.as_deref().unwrap_or(ANTHROPIC_API),
                    given.model.as_deref().unwrap_or_default()
                ))
                .header("x-api-key", &given.key)
                .header("anthropic-version", ANTHROPIC_VERSION),
            Service::DeepSeek | Service::OpenAiCompatible => client
                .get(format!(
                    "{}/models",
                    given.base_url.as_deref().unwrap_or(DEEPSEEK_BASE_URL)
                ))
                .bearer_auth(&given.key),
            Service::Deepgram => client
                .get(&self.deepgram)
                .header("authorization", format!("Token {}", given.key)),
            Service::ElevenLabs => {
                let id = given.voice_id.as_deref().unwrap_or_default();
                match &self.switched {
                    None => client.get(format!("{}/voices/{id}", self.elevenlabs)),
                    // A test lamp's address (lamp_msgs::own_voice): one
                    // word said in that voice, as the speech will ask.
                    Some(base) => {
                        let (_, url) = lamp_msgs::own_voice::check(Some(base), id);
                        let body = serde_json::json!({
                            "text": "Hi.",
                            "model_id": lamp_msgs::own_voice::CHECK_MODEL,
                        });
                        client
                            .post(url)
                            .header("content-type", "application/json")
                            .body(body.to_string())
                    }
                }
                .header("xi-api-key", &given.key)
            }
        };
        // The error is dropped whole: reqwest's names the URL.
        let response = tokio::time::timeout(CHECK_WITHIN, request.send())
            .await
            .ok()?
            .ok()?;
        let status = response.status().as_u16();
        let model = match (given.service, status, given.model.as_deref()) {
            (Service::ElevenLabs, 400 | 404, _) => return Some(Verdict::VoiceRefused),
            // The key taken, then the model (#8420): its endpoints, a public
            // read asked without the key, where 404 is no such model.
            (Service::OpenRouter, 200..=299, Some(model)) => {
                let endpoints = client
                    .get(format!(
                        "{}/models/{model}/endpoints",
                        given.base_url.as_deref().unwrap_or(OPENROUTER_API)
                    ))
                    .send();
                let status = tokio::time::timeout(CHECK_WITHIN, endpoints)
                    .await
                    .ok()?
                    .ok()?
                    .status()
                    .as_u16();
                return Some(match status {
                    404 => Verdict::NoModel,
                    status => Verdict::of_status(status),
                });
            }
            (Service::Gemini | Service::Anthropic, 404, _) => return Some(Verdict::NoModel),
            // Any 2xx is Anthropic's model: an alias is answered by its
            // dated id, so the id isn't matched (#8420).
            // A bad key is a 400 at Gemini, told by its body, read below.
            (Service::OpenAiCompatible | Service::Gemini, 200..=299, Some(model))
            | (Service::Gemini, 400, Some(model)) => model,
            _ => return Some(Verdict::of_status(status)),
        };
        // The endpoint's own list decides the model (#8362): read within
        // the same bound and at most [`MODELS_MOST`], never said back.
        let listed: Value = serde_json::from_slice(&read_most(response).await?).ok()?;
        if given.service == Service::Gemini {
            // The model's own id, `models/<id>` or `<id>`.
            let id = listed["id"].as_str().unwrap_or_default();
            return Some(match status {
                400 if gemini_key_invalid(&listed) => Verdict::Refused,
                400 => Verdict::Unreached,
                _ if id == model || id.strip_prefix("models/") == Some(model) => Verdict::Works,
                _ => Verdict::NoModel,
            });
        }
        let found = listed["data"]
            .as_array()?
            .iter()
            .any(|entry| entry["id"].as_str() == Some(model));
        Some(if found {
            Verdict::Works
        } else {
            Verdict::NoModel
        })
    }
}

/// A reply's body, within [`CHECK_WITHIN`] a chunk and at most
/// [`MODELS_MOST`]; `None` past it.
async fn read_most(mut response: reqwest::Response) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    while let Ok(Ok(Some(chunk))) = tokio::time::timeout(CHECK_WITHIN, response.chunk()).await {
        body.extend_from_slice(&chunk);
        if body.len() > MODELS_MOST {
            return None;
        }
    }
    Some(body)
}

/// The one chat the agent's tool check asks (#8735): `<base>/chat/completions`
/// at the endpoint and model the key was checked with, one tool with no
/// arguments, `tool_choice: "required"` and [`TOOLS_MAX_TOKENS`]. DeepSeek
/// thinking off, as it refuses a forced `tool_choice` while thinking, and
/// Gemini at its minimal thinking, as the voice asks them.
pub fn tools_asked(given: &Given) -> (String, Value) {
    let service = given.service;
    let base = given
        .base_url
        .as_deref()
        .or(service.endpoint())
        .unwrap_or(DEEPSEEK_BASE_URL)
        .trim_end_matches('/');
    let model = given.model.as_deref().unwrap_or(DEEPSEEK_MODEL);
    let mut body = json!({
        "model": model,
        "messages": [{"role": "user", "content": "Call the ping tool."}],
        "tools": [{"type": "function", "function": {
            "name": "ping",
            "description": "Says the lamp is here.",
            "parameters": {"type": "object", "properties": {}},
        }}],
        "tool_choice": "required",
        "max_tokens": TOOLS_MAX_TOKENS,
        "stream": false,
    });
    let deepseek = service == Service::DeepSeek
        || (service == Service::OpenAiCompatible
            && (model.to_ascii_lowercase().starts_with("deepseek")
                || host(base) == "api.deepseek.com"));
    if deepseek {
        body["thinking"] = json!({"type": "disabled"});
    }
    if service == Service::Gemini {
        body["reasoning_effort"] = json!("minimal");
    }
    (format!("{base}/chat/completions"), body)
}

/// Whether Gemini's error body refuses the key (#8420): it answers a bad key
/// with 400, not 401. Its OpenAI-compatible endpoint says "Please pass a valid
/// API key" (seen live on 24 Nov, as an object or a one-item array), its
/// native API `API_KEY_INVALID`. Any other 400 is the request's own.
fn gemini_key_invalid(body: &Value) -> bool {
    let error = match body {
        Value::Array(items) => items.first().map_or(&Value::Null, |item| &item["error"]),
        _ => &body["error"],
    };
    error["message"]
        .as_str()
        .is_some_and(|message| message.contains("valid API key"))
        || error["details"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|detail| detail["reason"] == "API_KEY_INVALID")
}

impl Check for HttpCheck {
    fn check(&mut self, given: &Given) -> Verdict {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return Verdict::Unreached;
        };
        // The whole check within the bound, however many requests it asks.
        let verdict = runtime
            .block_on(async { tokio::time::timeout(CHECK_WITHIN, self.ask(given)).await })
            .ok()
            .flatten()
            .unwrap_or(Verdict::Unreached);
        if verdict != Verdict::Works || !self.tools || !Service::MODELS.contains(&given.service) {
            return verdict;
        }
        // The agent's tool check (#8735), within its own bound: a chat with
        // no tool call, an error or no answer in time keeps the key for the
        // voice and leaves the agent on the relay.
        let tools = runtime
            .block_on(async { tokio::time::timeout(TOOLS_WITHIN, self.calls_tools(given)).await });
        if tools == Ok(Some(true)) {
            Verdict::Works
        } else {
            Verdict::WorksNoTools
        }
    }
}

/// The folder of the person's keys, root's, 0700, each file 0600.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Store(pub PathBuf);

impl Store {
    fn folder(&self) -> io::Result<()> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.0)?;
        fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700))
    }

    /// Keep a key that worked, and DeepSeek's endpoint beside it: the old
    /// key removed first, then the endpoint, then the key, so a voice that
    /// starts in between never sends one key to another key's endpoint.
    pub fn keep(&self, given: &Given) -> io::Result<()> {
        self.folder()?;
        let service = given.service;
        if let (Some(url), Some(file)) = (&given.base_url, service.url_file()) {
            remove(&self.0.join(service.key_file()))?;
            write_private(&self.0.join(file), format!("{url}\n").as_bytes())?;
        }
        if let Some(id) = &given.voice_id {
            write_private(
                &self.0.join(ELEVENLABS_VOICE_FILE),
                format!("{id}\n").as_bytes(),
            )?;
        }
        // An OpenAI-compatible model's name, before its key (#8362).
        if let (Some(model), Some(file)) = (&given.model, service.model_file()) {
            write_private(&self.0.join(file), format!("{model}\n").as_bytes())?;
        }
        write_private(&self.0.join(service.key_file()), given.key.as_bytes())?;
        // One own model at a time (#8362): keeping one model service's key
        // takes the other's away, so the model in use is never a guess.
        if Service::MODELS.contains(&service) {
            for other in Service::MODELS
                .into_iter()
                .filter(|other| *other != service)
            {
                self.remove(other)?;
            }
        }
        Ok(())
    }

    /// The last check's code for `service`, kept beside its key; best
    /// effort, as it holds nothing a voice reads (#8362). Only in a folder
    /// already there: a refused first key leaves nothing on the lamp. A
    /// model key that works adds the agent's tool check on a second line
    /// (#8735): `tools` or `no_tools`.
    pub fn checked(&self, service: Service, verdict: Verdict) {
        let model = Service::MODELS.contains(&service);
        let code = match verdict {
            Verdict::Works if model => "works\ntools",
            Verdict::WorksNoTools => "works\nno_tools",
            Verdict::Works => "works",
            Verdict::Refused | Verdict::NoModel | Verdict::VoiceRefused => "refused",
            Verdict::Unreached => "unreached",
            Verdict::NotKept => return,
        };
        if self.0.is_dir() {
            let _ = write_private(&self.0.join(service.checked_file()), code.as_bytes());
        }
    }

    /// ElevenLabs' kept key with `voice_id` in place of the kept voice, for
    /// a voice changed alone; `None` with no key kept.
    pub fn with_kept_key(&self, voice_id: String) -> Option<Given> {
        let key = fs::read_to_string(self.0.join(Service::ElevenLabs.key_file())).ok()?;
        Some(Given {
            service: Service::ElevenLabs,
            key,
            base_url: None,
            voice_id: Some(voice_id),
            model: None,
        })
    }

    /// The kept ElevenLabs voice ID, if any.
    pub fn voice_id(&self) -> Option<String> {
        fs::read_to_string(self.0.join(ELEVENLABS_VOICE_FILE))
            .ok()
            .map(|id| id.trim().to_owned())
            .filter(|id| !id.is_empty())
    }

    /// Remove a service's key, and its endpoint, model or voice after it.
    /// One that is not there is removed already.
    pub fn remove(&self, service: Service) -> io::Result<()> {
        remove(&self.0.join(service.key_file()))?;
        for file in [service.url_file(), service.model_file()]
            .into_iter()
            .flatten()
        {
            remove(&self.0.join(file))?;
        }
        if service == Service::ElevenLabs {
            remove(&self.0.join(ELEVENLABS_VOICE_FILE))?;
        }
        remove(&self.0.join(service.checked_file()))?;
        Ok(())
    }

    /// The `.checked` record's two lines: the key's check, then a model
    /// key's tool check (#8735). Never a key.
    fn checked_lines(&self, service: Service) -> (Option<String>, Option<String>) {
        let text = fs::read_to_string(self.0.join(service.checked_file())).unwrap_or_default();
        let mut lines = text
            .lines()
            .map(|line| line.trim().to_owned())
            .filter(|line| !line.is_empty());
        (lines.next(), lines.next())
    }

    /// Whether the person's key is there for `service`.
    pub fn has(&self, service: Service) -> bool {
        self.0.join(service.key_file()).is_file()
    }

    /// `{"deepseek": "own" | "lamp", "deepgram": "own" | "lamp", "since":
    /// {"deepseek", "deepgram"}, "deepseek_host"}`: whose key, since when it
    /// was kept (RFC 3339 UTC, `null` for the lamp's own) and the host
    /// DeepSeek's goes to (`null` for the lamp's own). Never a key. #8735:
    /// `in_use.agent` (`own` | `relay`) and `in_use.agent_why`, and each
    /// offer's `agent`, from the tool check kept in `.checked`.
    pub fn which(&self) -> Value {
        let mut which = serde_json::Map::new();
        let mut since = serde_json::Map::new();
        for service in Service::ALL {
            let kept = fs::metadata(self.0.join(service.key_file()))
                .ok()
                .filter(fs::Metadata::is_file);
            which.insert(
                service.id().into(),
                if kept.is_some() { "own" } else { "lamp" }.into(),
            );
            since.insert(
                service.id().into(),
                json!(
                    kept.and_then(|meta| meta.modified().ok())
                        .map(crate::serve::rfc3339)
                ),
            );
        }
        let host = self
            .has(Service::DeepSeek)
            .then(|| fs::read_to_string(self.0.join(DEEPSEEK_URL_FILE)).ok())
            .flatten()
            .map(|url| host(url.trim()).to_owned())
            .filter(|host| !host.is_empty());
        which.insert("since".into(), Value::Object(since));
        which.insert("deepseek_host".into(), json!(host));
        // #8367: the voice ID in use beside its key; a voice ID isn't a
        // secret, the key never comes back.
        let voice = self
            .has(Service::ElevenLabs)
            .then(|| self.voice_id())
            .flatten();
        which.insert("voice_id".into(), json!({"elevenlabs": voice}));
        // #8362: the OpenAI-compatible endpoint's host, the model and the
        // last check by service, and which own model is in use. Never a key.
        let read = |service: Service, file: Option<&str>| {
            self.has(service)
                .then(|| fs::read_to_string(self.0.join(file?)).ok())
                .flatten()
                .map(|text| text.trim().to_owned())
                .filter(|text| !text.is_empty())
        };
        let compatible = Service::OpenAiCompatible;
        which.insert(
            "openai_compatible_host".into(),
            json!(
                read(compatible, compatible.url_file())
                    .map(|url| self::host(&url).to_owned())
                    .filter(|host| !host.is_empty())
            ),
        );
        let mut model = serde_json::Map::new();
        let mut checked = serde_json::Map::new();
        for service in Service::ALL {
            model.insert(
                service.id().into(),
                json!(read(service, service.model_file())),
            );
            let code = self
                .checked_lines(service)
                .0
                .filter(|code| ["works", "refused", "unreached"].contains(&code.as_str()));
            checked.insert(service.id().into(), json!(code));
        }
        which.insert("model".into(), Value::Object(model));
        which.insert("checked".into(), Value::Object(checked));
        let in_use = Service::MODELS
            .into_iter()
            .find(|service| self.has(*service));
        // #8735: what the agent uses, `own` only for an in-use model key whose
        // chat made a tool call, and why not, by a short code.
        let why = match in_use.map(|service| self.checked_lines(service)) {
            Some((Some(code), Some(tools))) if code == "works" && tools == "tools" => None,
            Some((_, Some(tools))) if tools == "no_tools" => Some("no_tool_calls"),
            Some((Some(code), _)) if code == "refused" => Some("refused"),
            // Kept before the tool check was asked: set again to check it.
            Some(_) => Some("unchecked"),
            None if self.has(Service::ElevenLabs) => Some("tts_only"),
            None => Some("no_model_key"),
        };
        which.insert(
            "in_use".into(),
            json!({
                "model": in_use.map(Service::id),
                "agent": if why.is_none() { "own" } else { "relay" },
                "agent_why": why,
            }),
        );
        which.insert(
            "offers".into(),
            Value::Array(
                Service::ALL
                    .into_iter()
                    .map(|service| {
                        let mut offer = service.offer();
                        // A model service serves the agent unless its kept
                        // key's tool check failed (#8735).
                        offer["agent"] = json!(
                            Service::MODELS.contains(&service)
                                && !(self.has(service)
                                    && self.checked_lines(service).1.as_deref()
                                        == Some("no_tools"))
                        );
                        offer
                    })
                    .collect(),
            ),
        );
        Value::Object(which)
    }
}

fn remove(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    }
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

/// Check a key and keep it only if it works. The plain answer, and whether
/// the lamp's keys changed. A key that worked but could not be written may
/// have removed the one kept before (`keep` takes the old key away first):
/// so the service's key is removed as far as it can be, said as
/// [`Verdict::NotKept`], and counted as a change, for the voice to restart.
///
/// The check's code is kept in `.checked` here, for setup's keys and
/// `keys.set`'s alike, so `keys.get` says the agent's tool check (#8735).
pub fn check_and_keep(given: &Given, store: &Store, check: &mut impl Check) -> (Verdict, bool) {
    let (verdict, changed) = match check.check(given) {
        verdict @ (Verdict::Works | Verdict::WorksNoTools) => match store.keep(given) {
            Ok(()) => (verdict, true),
            Err(_) => {
                let _ = store.remove(given.service);
                (Verdict::NotKept, true)
            }
        },
        verdict => (verdict, false),
    };
    store.checked(given.service, verdict);
    (verdict, changed)
}

/// A refusal in the contract's shape: the person's words and its code.
pub fn refused(why: &str, code: &str) -> Value {
    json!({"refused": why, "code": code})
}

/// What an answered request did, for the journal and what follows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Done {
    /// The voice must take the change at its next start.
    pub restart_voice: bool,
    /// `lamp-cloud` must take it too: a model key changed (#8735), which
    /// #8742's forwarder loads by `LoadCredential=`, copied only at its start.
    pub restart_cloud: bool,
    /// The journal's line: which service and what happened, never a key.
    pub said: String,
}

/// After a key change is answered: the voice restarted once it has rested
/// 1 s, a turn in flight waited out ([`crate::change::KEY_CHANGE_QUIET`],
/// #8710), and no update is at work, lamp-update's lock held through both
/// restarts. `lamp-cloud`, when `done.restart_cloud`, waits an update's
/// [`lamp_update::quiet::QUIET`] (45 s) of rest after the voice's restart:
/// it holds the reply only in memory until its online loop relays it, and a
/// link down or a slow publish must not lose it (Sixth's P2 on #8741). All
/// within [`crate::change::QUIET_MOST`]. The journal's lines.
pub fn restart_after(
    done: &Done,
    ears: &mut impl crate::change::Ears,
    records: &lamp_update::records::Records,
    systemd: &mut impl crate::change::Systemd,
) -> Vec<String> {
    use crate::change;
    let started = ears.now_ms();
    let _held = match change::quiet_and_unlocked(
        ears,
        records,
        change::KEY_CHANGE_QUIET,
        change::QUIET_MOST,
    ) {
        Ok(lock) => lock,
        Err(why) => {
            return vec![format!(
                "the lamp was not quiet with no update at work within {} min ({why}); {} at its next start",
                change::QUIET_MOST.as_secs() / 60,
                if done.restart_cloud {
                    "the voice and lamp-cloud take them"
                } else {
                    "the voice takes them"
                }
            )];
        }
    };
    let mut said = vec![match systemd.systemctl(&["try-restart", VOICE_UNIT]) {
        Ok(()) => "the voice restarted to take them, at a quiet moment".to_owned(),
        Err(why) => format!("the voice could not be restarted: {why}"),
    }];
    if done.restart_cloud {
        let spent = std::time::Duration::from_millis(ears.now_ms().saturating_sub(started));
        if let Err(why) = change::wait_for_rest(
            ears,
            lamp_update::quiet::QUIET,
            change::QUIET_MOST.saturating_sub(spent),
        ) {
            said.push(format!(
                "lamp-cloud was not restarted: the lamp was not quiet for {} s within {} min ({why}); it loads the person's model key at its next start",
                lamp_update::quiet::QUIET.as_secs(),
                change::QUIET_MOST.as_secs() / 60
            ));
            return said;
        }
        said.push(match systemd.systemctl(&["try-restart", ASKER_UNIT]) {
            Ok(()) => "lamp-cloud restarted to load the person's model key for the agent (its LoadCredential= is copied only at its start, #8742)".to_owned(),
            Err(why) => format!("lamp-cloud could not be restarted to load the person's model key: {why}"),
        });
    }
    said
}

/// The helper's reply line to one request: `{"done": data}` or
/// `{"refused": why}`. Neither ever holds a key or an endpoint.
pub fn answer(
    line: &str,
    store: &Store,
    check: &mut impl Check,
    enabled: bool,
) -> (Value, Option<Done>) {
    if !enabled {
        return (
            refused("own keys are off on this lamp", "unavailable"),
            None,
        );
    }
    let asked = match parse(line.trim_end_matches(['\n', '\r'])) {
        Ok(asked) => asked,
        Err(why) => return (refused(&why, "bad_field"), None),
    };
    match asked {
        Asked::Which => (json!({"done": store.which()}), None),
        Asked::Remove(service) => match store.remove(service) {
            Ok(()) => (
                json!({"done": {"service": service.id(), "uses": "lamp"}}),
                Some(Done {
                    restart_voice: true,
                    restart_cloud: Service::MODELS.contains(&service),
                    said: format!("the person's {} key was removed", service.name()),
                }),
            ),
            Err(error) => (
                refused(
                    &format!("the key could not be removed: {}", error.kind()),
                    "unavailable",
                ),
                None,
            ),
        },
        Asked::Voice(id) => match store.with_kept_key(id) {
            Some(given) => set(given, store, check),
            None => (
                refused(
                    "a voice ID is changed beside a kept ElevenLabs key: give the key too",
                    "bad_field",
                ),
                None,
            ),
        },
        Asked::Set(mut given) => {
            if given.service == Service::ElevenLabs && given.voice_id.is_none() {
                given.voice_id = store.voice_id();
                if given.voice_id.is_none() {
                    return (
                        refused("ElevenLabs needs a voice ID with its key", "bad_field"),
                        None,
                    );
                }
            }
            set(given, store, check)
        }
    }
}

/// Check `given`, keep it if it works, and the reply and the journal's line.
fn set(given: Given, store: &Store, check: &mut impl Check) -> (Value, Option<Done>) {
    let (verdict, changed) = check_and_keep(&given, store, check);
    let service = given.service;
    let said = format!(
        "the person's {} key: {}",
        service.name(),
        match verdict {
            Verdict::Works => "it works, kept",
            Verdict::WorksNoTools =>
                "it works but made no tool call, kept; the agent stays on the relay",
            Verdict::Refused => "refused by the service, not kept",
            Verdict::VoiceRefused => "the voice ID was not on its account, not kept",
            Verdict::Unreached => "the check reached nothing, not kept",
            Verdict::NotKept => "it works but could not be written, the lamp's own used",
            Verdict::NoModel => "the endpoint lists no such model, not kept",
        }
    );
    let done = Some(Done {
        restart_voice: changed,
        restart_cloud: changed && Service::MODELS.contains(&service),
        said,
    });
    if matches!(verdict, Verdict::Works | Verdict::WorksNoTools) {
        (
            json!({"done": {"service": service.id(), "uses": "own", "code": verdict.code(), "answer": verdict.words(&given)}}),
            done,
        )
    } else {
        (refused(&verdict.words(&given), verdict.code()), done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    /// The model in use is the one the voice and lamp-cloud's forwarder
    /// take (#8735): the same services in the same order.
    #[test]
    fn the_models_are_in_the_shared_precedence() {
        let ours: Vec<_> = Service::MODELS.iter().map(|service| service.id()).collect();
        let shared: Vec<_> = lamp_msgs::own_model::PRECEDENCE
            .iter()
            .map(|model| model.id())
            .collect();
        assert_eq!(ours, shared);
    }

    /// A local server that answers `status` once; what it was asked.
    fn answering(status: u16) -> (String, Served) {
        answering_with(&format!(
            "HTTP/1.1 {status} X\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
        ))
    }

    /// A local server that answers `reply` to the one request it takes, and
    /// fails the test if a second comes before [`Served::join`]: a check
    /// has returned by then, so a second request of its own has connected,
    /// and one more look finds it.
    fn answering_with(reply: &str) -> (String, Served) {
        answering_each(&[reply])
    }

    /// [`answering_with`] for a check of more than one request: each reply
    /// in turn, one request each, and none after the last.
    fn answering_each(replies: &[&str]) -> (String, Served) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let replies: Vec<String> = replies.iter().map(|reply| (*reply).to_owned()).collect();
        let (stop, stopped) = std::sync::mpsc::channel::<()>();
        let thread = std::thread::spawn(move || {
            let mut asked = Vec::new();
            for reply in replies {
                let (mut socket, _) = listener.accept().unwrap();
                let mut request = vec![0; 8_192];
                let n = socket.read(&mut request).unwrap();
                socket.write_all(reply.as_bytes()).unwrap();
                drop(socket);
                asked.push(String::from_utf8_lossy(&request[..n]).to_lowercase());
            }
            // Up to the check's bound, as before, unless stopped sooner.
            let _ = stopped.recv_timeout(Duration::from_millis(300));
            listener.set_nonblocking(true).unwrap();
            assert!(listener.accept().is_err(), "a request more than asked for");
            asked
        });
        (format!("http://{address}"), Served { stop, thread })
    }

    /// The server [`answering_each`] runs.
    struct Served {
        stop: std::sync::mpsc::Sender<()>,
        thread: std::thread::JoinHandle<Vec<String>>,
    }

    impl Served {
        /// What it was asked, once it is stopped and has looked once more.
        fn join(self) -> std::thread::Result<String> {
            self.join_each().map(|asked| asked.concat())
        }

        /// [`Self::join`], each request apart.
        fn join_each(self) -> std::thread::Result<Vec<String>> {
            let _ = self.stop.send(());
            self.thread.join()
        }
    }

    /// The service's error body is never read or passed on: a 401 whose
    /// body names a made-up key and the provider's words is said only as
    /// "refused", in the reply and in the journal's line alike.
    #[test]
    fn the_services_error_body_is_never_passed_on() {
        const BODY: &str = r#"{"error":{"message":"Incorrect API key provided: sk-made-up-body-2094","type":"provider_said_this"}}"#;
        /// The real check, sent to the local server in place of the
        /// endpoint the request named (which must be https).
        struct Local(String);
        impl Check for Local {
            fn check(&mut self, given: &Given) -> Verdict {
                let local = Given {
                    base_url: Some(self.0.clone()),
                    ..given.clone()
                };
                HttpCheck::local(String::new()).check(&local)
            }
        }
        let (base, served) = answering_with(&format!(
            "HTTP/1.1 401 Unauthorized\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{BODY}",
            BODY.len()
        ));
        let folder =
            std::env::temp_dir().join(format!("lamp-setup-keys-body-{}", std::process::id()));
        let store = Store(folder.join("own"));
        let line = r#"{"set":"deepseek","key":"sk-made-up-request-2094","base_url":"https://api.deepseek.com"}"#;
        let (reply, done) = answer(line, &store, &mut Local(base), true);
        let asked = served.join().unwrap();
        assert!(asked.starts_with("get /models http/1.1"), "{asked}");
        assert_eq!(
            reply,
            json!({"refused": "that key was refused", "code": "refused"})
        );
        let said = done.unwrap().said;
        assert_eq!(
            said,
            "the person's DeepSeek key: refused by the service, not kept"
        );
        for part in [
            "sk-",
            "made-up",
            "Incorrect",
            "provider_said_this",
            "error",
            "401",
        ] {
            assert!(!reply.to_string().contains(part), "{part} in {reply}");
            assert!(!said.contains(part), "{part} in {said}");
        }
        assert!(!store.0.exists());
        let _ = fs::remove_dir_all(folder);
    }

    /// One request each, the key in its header alone, and the status read
    /// as works, refused or not reached.
    #[test]
    fn the_check_is_one_request_with_the_key_in_its_header() {
        for (status, verdict) in [
            (200, Verdict::Works),
            (401, Verdict::Refused),
            (403, Verdict::Refused),
            (500, Verdict::Unreached),
        ] {
            let (base, served) = answering(status);
            let deepseek = Given {
                service: Service::DeepSeek,
                key: "sk-planted-2094".into(),
                base_url: Some(format!("{base}/v1")),
                voice_id: None,
                model: None,
            };
            let mut check = HttpCheck::local(String::new());
            assert_eq!(check.check(&deepseek), verdict);
            let asked = served.join().unwrap();
            assert!(asked.starts_with("get /v1/models http/1.1"), "{asked}");
            assert!(asked.contains("authorization: bearer sk-planted-2094"));
            let (base, served) = answering(status);
            let deepgram = given(Service::Deepgram, "dg-planted-2094", None).unwrap();
            let mut check = HttpCheck::local(format!("{base}/v1/auth/token"));
            assert_eq!(check.check(&deepgram), verdict);
            let asked = served.join().unwrap();
            assert!(asked.starts_with("get /v1/auth/token http/1.1"), "{asked}");
            assert!(asked.contains("authorization: token dg-planted-2094"));
        }
        // A redirect is never followed, off the host or on it: one request
        // with the key, and the check is not reached (the first builder's 3).
        let (base, served) = answering_with(
            "HTTP/1.1 302 Found\r\nlocation: http://127.0.0.1:9/steal\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
        );
        let deepseek = Given {
            service: Service::DeepSeek,
            key: "sk-planted-2094".into(),
            base_url: Some(base),
            voice_id: None,
            model: None,
        };
        assert_eq!(
            HttpCheck::local(String::new()).check(&deepseek),
            Verdict::Unreached
        );
        served.join().unwrap();
        // Nothing listening: not reached.
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = closed.local_addr().unwrap();
        drop(closed);
        let mut check = HttpCheck::local(format!("http://{address}/v1/auth/token"));
        let deepgram = given(Service::Deepgram, "dg-planted-2094", None).unwrap();
        assert_eq!(check.check(&deepgram), Verdict::Unreached);
        // The real one asks only over https.
        let mut real = HttpCheck {
            deepgram: format!("http://{address}/v1/auth/token"),
            ..HttpCheck::default()
        };
        assert_eq!(real.check(&deepgram), Verdict::Unreached);
    }

    /// #8367: ElevenLabs' check is one request for the voice, the key in
    /// `xi-api-key` alone: 2xx works, 401 or 403 refuses the key, 400 or
    /// 404 the voice ID, anything else is not reached.
    #[test]
    fn a_switched_check_asks_the_given_address_for_one_word_with_the_key() {
        for (status, verdict) in [
            (200, Verdict::Works),
            (401, Verdict::Refused),
            (404, Verdict::VoiceRefused),
        ] {
            let (base, served) = answering(status);
            let voice = Given {
                voice_id: Some("Voice8367".into()),
                ..given(Service::ElevenLabs, "sk-planted-8367", None).unwrap()
            };
            let mut check = HttpCheck::local(format!("{base}/v1"));
            check.switched = Some(format!("{base}/relay"));
            assert_eq!(check.check(&voice), verdict, "{status}");
            let asked = served.join().unwrap();
            assert!(
                asked.starts_with(
                    "post /relay/elevenlabs/text-to-speech/voice8367/stream?output_format=pcm_24000 http/1.1"
                ),
                "{asked}"
            );
            assert!(asked.contains("xi-api-key: sk-planted-8367"), "{asked}");
        }
    }

    #[test]
    fn the_elevenlabs_check_asks_for_the_voice_with_the_key_in_its_header() {
        for (status, verdict) in [
            (200, Verdict::Works),
            (401, Verdict::Refused),
            (403, Verdict::Refused),
            (400, Verdict::VoiceRefused),
            (404, Verdict::VoiceRefused),
            (500, Verdict::Unreached),
        ] {
            let (base, served) = answering(status);
            let voice = Given {
                voice_id: Some("Voice8367".into()),
                ..given(Service::ElevenLabs, "sk-planted-8367", None).unwrap()
            };
            let mut check = HttpCheck::local(format!("{base}/v1"));
            assert_eq!(check.check(&voice), verdict, "{status}");
            let asked = served.join().unwrap();
            assert!(
                asked.starts_with("get /v1/voices/voice8367 http/1.1"),
                "{asked}"
            );
            assert!(asked.contains("xi-api-key: sk-planted-8367"), "{asked}");
        }
        let voice = given(Service::ElevenLabs, "sk-planted-8367", None).unwrap();
        assert_eq!(HttpCheck::default().elevenlabs, ELEVENLABS_API);
        assert_eq!(
            Verdict::VoiceRefused.words(&voice),
            "that key works, but that voice ID isn't on its ElevenLabs account; nothing changed"
        );
        assert!(given(Service::ElevenLabs, "k", Some("https://x.invalid")).is_err());
        for bad in ["", "../x", "a b", &"v".repeat(65)] {
            assert!(fit_voice(bad).is_err(), "{bad}");
        }
    }

    /// #8362 step 1b: an OpenAI-compatible key's one check is
    /// `GET <endpoint>/models` with the key as the bearer. It works only when
    /// the endpoint lists the model asked for, so a typed name can't leave
    /// the voice with no model; otherwise it is refused plainly as the
    /// model, and the endpoint's list is never said back.
    #[test]
    fn an_openai_compatible_check_finds_the_model_in_the_endpoints_list() {
        const LISTED: &str = r#"{"object":"list","data":[{"id":"grok-4","object":"model"},{"id":"grok-3-mini","object":"model"}]}"#;
        struct Local(String);
        impl Check for Local {
            fn check(&mut self, given: &Given) -> Verdict {
                let local = Given {
                    base_url: Some(self.0.clone()),
                    ..given.clone()
                };
                HttpCheck::local(String::new()).check(&local)
            }
        }
        for (model, works) in [("grok-4", true), ("grok-9", false)] {
            let (base, served) = answering_with(&format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{LISTED}",
                LISTED.len()
            ));
            let folder = std::env::temp_dir().join(format!(
                "lamp-setup-keys-compatible-{model}-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&folder);
            let store = Store(folder.join("own"));
            let line = json!({"set": "openai_compatible", "key": "sk-made-up-8362", "base_url": "https://api.compatible.invalid/v1", "model": model}).to_string();
            let (reply, done) = answer(&line, &store, &mut Local(format!("{base}/v1")), true);
            // Before the join, so a request refused before its check fails
            // here instead of leaving the server waiting.
            assert!(done.is_some(), "the check never ran: {reply}");
            let asked = served.join().unwrap();
            assert!(asked.starts_with("get /v1/models http/1.1"), "{asked}");
            assert!(asked.contains("authorization: bearer sk-made-up-8362"));
            assert!(!reply.to_string().contains("sk-made-up"), "{reply}");
            assert!(!done.as_ref().unwrap().said.contains("sk-made-up"));
            if works {
                assert_eq!(reply["done"]["code"], "works", "{reply}");
                assert!(store.0.join("openai_compatible.key").is_file());
            } else {
                assert_eq!(
                    reply,
                    json!({"refused": "that endpoint has no model by that name", "code": "bad_field"})
                );
                assert!(!reply.to_string().contains("grok-3-mini"), "{reply}");
                assert!(!store.0.join("openai_compatible.key").exists());
            }
            let _ = fs::remove_dir_all(&folder);
        }
    }

    /// #8735: a model key that works is asked once more, a chat forcing one
    /// tool with the key as the bearer: a tool call back is `Works`, a chat
    /// without one `WorksNoTools`, which is kept.
    #[test]
    fn a_model_check_asks_once_for_a_tool_call() {
        const LISTED: &str = r#"{"object":"list","data":[{"id":"grok-4","object":"model"}]}"#;
        const CALLED: &str = r#"{"choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":"c1","type":"function","function":{"name":"ping","arguments":"{}"}}]}}]}"#;
        const SAID: &str = r#"{"choices":[{"message":{"role":"assistant","content":"pong"}}]}"#;
        let ok = |body: &str| {
            format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            )
        };
        for (chat, verdict) in [
            (ok(CALLED), Verdict::Works),
            (ok(SAID), Verdict::WorksNoTools),
            (
                "HTTP/1.1 400 X\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".into(),
                Verdict::WorksNoTools,
            ),
        ] {
            let (base, served) = answering_each(&[&ok(LISTED), &chat]);
            let given = Given {
                service: Service::OpenAiCompatible,
                key: "sk-made-up-8735".into(),
                base_url: Some(format!("{base}/v1")),
                voice_id: None,
                model: Some("grok-4".into()),
            };
            let mut check = HttpCheck::local(String::new());
            check.tools = true;
            assert_eq!(check.check(&given), verdict);
            let asked = served.join_each().unwrap();
            assert!(asked[0].starts_with("get /v1/models http/1.1"), "{asked:?}");
            let tools = &asked[1];
            assert!(
                tools.starts_with("post /v1/chat/completions http/1.1"),
                "{tools}"
            );
            assert!(tools.contains("authorization: bearer sk-made-up-8735"));
            // The body is `tools_asked`'s, pinned here; the server reads one
            // segment of the request, which may end before the body.
            let (url, body) = tools_asked(&given);
            assert_eq!(url, format!("{base}/v1/chat/completions"));
            assert_eq!(body["model"], "grok-4");
            assert_eq!(body["tool_choice"], "required");
            assert_eq!(body["tools"][0]["function"]["name"], "ping");
            assert_eq!(body["max_tokens"], TOOLS_MAX_TOKENS);
            assert!(body.get("thinking").is_none());
        }
        // DeepSeek thinking off, Gemini at its floor, each at its own chat.
        let deepseek = given(Service::DeepSeek, "k", None).unwrap();
        let (url, body) = tools_asked(&deepseek);
        assert_eq!(url, "https://api.deepseek.com/chat/completions");
        assert_eq!(body["model"], DEEPSEEK_MODEL);
        assert_eq!(body["thinking"], json!({"type": "disabled"}));
        let gemini = Given {
            service: Service::Gemini,
            model: Some("gemini-3.6-flash".into()),
            base_url: None,
            ..deepseek
        };
        let (url, body) = tools_asked(&gemini);
        assert_eq!(url, format!("{GEMINI_API}/chat/completions"));
        assert_eq!(body["reasoning_effort"], "minimal");
        assert!(body.get("thinking").is_none());
    }

    /// #8420: OpenRouter's check is two requests to its fixed endpoint:
    /// `GET /key` with the key as the bearer, since its model list is public
    /// and proves nothing about a key, then `GET /models/<model>/endpoints`
    /// without it, where a 404 means no such model. Refused on the first
    /// asks nothing more; the key is never said back.
    #[test]
    fn an_openrouter_check_asks_for_the_key_then_the_models_endpoints() {
        fn status(status: u16) -> String {
            format!("HTTP/1.1 {status} X\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
        }
        /// The real check, sent to the local server in place of OpenRouter.
        struct Local(String);
        impl Check for Local {
            fn check(&mut self, given: &Given) -> Verdict {
                assert_eq!(
                    given.base_url.as_deref(),
                    Some("https://openrouter.ai/api/v1")
                );
                let local = Given {
                    base_url: Some(self.0.clone()),
                    ..given.clone()
                };
                HttpCheck::local(String::new()).check(&local)
            }
        }
        let line = json!({"set": "openrouter", "key": "sk-or-made-up-8420", "model": "openai/gpt-4o-mini"}).to_string();
        for (replies, code, asks) in [
            (vec![status(200), status(200)], "works", 2),
            (vec![status(200), status(404)], "bad_field", 2),
            (vec![status(401)], "refused", 1),
            (vec![status(200), status(500)], "unreached", 2),
        ] {
            let replies: Vec<&str> = replies.iter().map(String::as_str).collect();
            let (base, served) = answering_each(&replies);
            let folder = std::env::temp_dir().join(format!(
                "lamp-setup-keys-openrouter-{code}-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&folder);
            let store = Store(folder.join("own"));
            let (reply, done) = answer(&line, &store, &mut Local(format!("{base}/api/v1")), true);
            assert!(done.is_some(), "the check never ran: {reply}");
            let asked = served.join_each().unwrap();
            assert_eq!(asked.len(), asks, "{code}: {asked:?}");
            assert!(
                asked[0].starts_with("get /api/v1/key http/1.1"),
                "{asked:?}"
            );
            assert!(asked[0].contains("authorization: bearer sk-or-made-up-8420"));
            if asks == 2 {
                assert!(
                    asked[1]
                        .starts_with("get /api/v1/models/openai/gpt-4o-mini/endpoints http/1.1"),
                    "{asked:?}"
                );
                assert!(!asked[1].contains("sk-or-made-up"), "{asked:?}");
            }
            let said = done.unwrap().said;
            assert!(!reply.to_string().contains("sk-or"), "{reply}");
            assert!(!said.contains("sk-or"), "{said}");
            if code == "works" {
                assert_eq!(reply["done"]["code"], "works", "{reply}");
                assert!(store.0.join("openrouter.key").is_file());
            } else {
                assert_eq!(reply["code"], code, "{reply}");
                assert!(!store.0.join("openrouter.key").exists());
            }
            if code == "bad_field" {
                assert_eq!(reply["refused"], "OpenRouter has no model by that name");
            }
            let _ = fs::remove_dir_all(&folder);
        }
    }

    /// #8420: Gemini's check is one request to its fixed OpenAI-compatible
    /// endpoint, `GET /models/<model>` with the key as the bearer. Its id
    /// comes back as `models/<id>` or `<id>`; 404 is no such model. A bad
    /// key is a 400 there, not a 401: refused only by its own error body,
    /// as seen live on 24 Nov with a made-up key ("Please pass a valid API
    /// key", `INVALID_ARGUMENT`; the native API's `API_KEY_INVALID`); any
    /// other 400 is not the key's. The key is never said back.
    #[test]
    fn a_gemini_check_reads_the_one_model_and_refuses_a_bad_key_by_its_400() {
        fn reply(status: u16, body: &str) -> String {
            format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            )
        }
        /// The real check, sent to the local server in place of Gemini.
        struct Local(String);
        impl Check for Local {
            fn check(&mut self, given: &Given) -> Verdict {
                assert_eq!(
                    given.base_url.as_deref(),
                    Some("https://generativelanguage.googleapis.com/v1beta/openai")
                );
                let local = Given {
                    base_url: Some(self.0.clone()),
                    ..given.clone()
                };
                HttpCheck::local(String::new()).check(&local)
            }
        }
        let live_400 = r#"{"error":{"code":400,"message":"Please pass a valid API key","status":"INVALID_ARGUMENT"}}"#;
        let native_400 = r#"{"error":{"code":400,"message":"API key not valid. Please pass a valid API key.","status":"INVALID_ARGUMENT","details":[{"@type":"type.googleapis.com/google.rpc.ErrorInfo","reason":"API_KEY_INVALID","domain":"googleapis.com"}]}}"#;
        let plain_400 = r#"{"error":{"code":400,"message":"Invalid JSON payload received.","status":"INVALID_ARGUMENT"}}"#;
        let line =
            json!({"set": "gemini", "key": "AIza-made-up-8420", "model": "gemini-2.5-flash"})
                .to_string();
        for (served, code) in [
            (
                reply(
                    200,
                    r#"{"id":"models/gemini-2.5-flash","object":"model","owned_by":"google"}"#,
                ),
                "works",
            ),
            (
                reply(
                    200,
                    r#"{"id":"gemini-2.5-flash","object":"model","owned_by":"google"}"#,
                ),
                "works",
            ),
            (
                reply(404, r#"{"error":{"code":404,"status":"NOT_FOUND"}}"#),
                "bad_field",
            ),
            (reply(400, live_400), "refused"),
            (reply(400, native_400), "refused"),
            (reply(401, ""), "refused"),
            (reply(403, ""), "refused"),
            (reply(400, plain_400), "unreached"),
            (reply(500, ""), "unreached"),
        ] {
            let (base, served) = answering_with(&served);
            let folder = std::env::temp_dir().join(format!(
                "lamp-setup-keys-gemini-{code}-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&folder);
            let store = Store(folder.join("own"));
            let (reply, done) = answer(
                &line,
                &store,
                &mut Local(format!("{base}/v1beta/openai")),
                true,
            );
            assert!(done.is_some(), "the check never ran: {reply}");
            let asked = served.join().unwrap();
            assert!(
                asked.starts_with("get /v1beta/openai/models/gemini-2.5-flash http/1.1"),
                "{asked}"
            );
            assert!(
                asked.contains("authorization: bearer aiza-made-up-8420"),
                "{asked}"
            );
            let said = done.unwrap().said;
            assert!(!reply.to_string().contains("made-up"), "{reply}");
            assert!(!said.contains("made-up"), "{said}");
            if code == "works" {
                assert_eq!(reply["done"]["code"], "works", "{reply}");
                assert!(store.0.join("gemini.key").is_file());
            } else {
                assert_eq!(reply["code"], code, "{reply}");
                assert!(!store.0.join("gemini.key").exists());
            }
            if code == "bad_field" {
                assert_eq!(reply["refused"], "Gemini has no model by that name");
            }
            let _ = fs::remove_dir_all(&folder);
        }
    }

    /// #8420: Anthropic's check is one request to its fixed API, `GET
    /// /models/<model>` with the key as `x-api-key` and `anthropic-version:
    /// 2023-06-01`, never a bearer: its model read takes no bearer (seen
    /// live on 24 Nov with a made-up key: 401, `authentication_error`,
    /// "invalid x-api-key"). Any 2xx is the model, an alias answered by its
    /// dated id; 404 is no such model; 401 and 403 refuse the key; anything
    /// else is unreached. The key is never said back.
    #[test]
    fn an_anthropic_check_reads_the_one_model_with_its_own_headers() {
        fn reply(status: u16, body: &str) -> String {
            format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            )
        }
        /// The real check, sent to the local server in place of Anthropic.
        struct Local(String);
        impl Check for Local {
            fn check(&mut self, given: &Given) -> Verdict {
                assert_eq!(
                    given.base_url.as_deref(),
                    Some("https://api.anthropic.com/v1")
                );
                let local = Given {
                    base_url: Some(self.0.clone()),
                    ..given.clone()
                };
                HttpCheck::local(String::new()).check(&local)
            }
        }
        let live_401 = r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"},"request_id":"req_made_up"}"#;
        let line =
            json!({"set": "anthropic", "key": "sk-ant-made-up-8420", "model": "claude-sonnet-4-5"})
                .to_string();
        for (served, code) in [
            (
                reply(
                    200,
                    r#"{"type":"model","id":"claude-sonnet-4-5-20250929","display_name":"Claude Sonnet 4.5","created_at":"2025-09-29T00:00:00Z"}"#,
                ),
                "works",
            ),
            (
                reply(
                    404,
                    r#"{"type":"error","error":{"type":"not_found_error","message":"model: claude-sonnet-4-5"}}"#,
                ),
                "bad_field",
            ),
            (reply(401, live_401), "refused"),
            (
                reply(
                    403,
                    r#"{"type":"error","error":{"type":"permission_error","message":"no"}}"#,
                ),
                "refused",
            ),
            (reply(400, r#"{"type":"error"}"#), "unreached"),
            (reply(529, r#"{"type":"error"}"#), "unreached"),
        ] {
            let (base, served) = answering_with(&served);
            let folder = std::env::temp_dir().join(format!(
                "lamp-setup-keys-anthropic-{code}-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&folder);
            let store = Store(folder.join("own"));
            let (reply, done) = answer(&line, &store, &mut Local(format!("{base}/v1")), true);
            assert!(done.is_some(), "the check never ran: {reply}");
            let asked = served.join().unwrap();
            assert!(
                asked.starts_with("get /v1/models/claude-sonnet-4-5 http/1.1"),
                "{asked}"
            );
            assert!(asked.contains("x-api-key: sk-ant-made-up-8420"), "{asked}");
            assert!(asked.contains("anthropic-version: 2023-06-01"), "{asked}");
            assert!(!asked.contains("authorization"), "{asked}");
            let said = done.unwrap().said;
            assert!(!reply.to_string().contains("made-up"), "{reply}");
            assert!(!said.contains("made-up"), "{said}");
            if code == "works" {
                assert_eq!(reply["done"]["code"], "works", "{reply}");
                assert!(store.0.join("anthropic.key").is_file());
            } else {
                assert_eq!(reply["code"], code, "{reply}");
                assert!(!store.0.join("anthropic.key").exists());
            }
            if code == "bad_field" {
                assert_eq!(reply["refused"], "Anthropic has no model by that name");
            }
            let _ = fs::remove_dir_all(&folder);
        }
    }
}
