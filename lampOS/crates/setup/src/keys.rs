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
pub const DEEPSEEK_BASE_URL: &str = "https://api.deepseek.com";
/// Deepgram's call that answers 200 for a key it takes, and nothing more.
pub const DEEPGRAM_CHECK_URL: &str = "https://api.deepgram.com/v1/auth/token";
/// ElevenLabs' API: the check asks `<it>/voices/<voice_id>` (#8367).
pub const ELEVENLABS_API: &str = "https://api.elevenlabs.io/v1";
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
}

impl Service {
    pub const ALL: [Service; 3] = [Service::DeepSeek, Service::Deepgram, Service::ElevenLabs];

    /// The name the app and the reply use.
    pub fn id(self) -> &'static str {
        match self {
            Self::DeepSeek => "deepseek",
            Self::Deepgram => "deepgram",
            Self::ElevenLabs => "elevenlabs",
        }
    }

    /// The name a person reads.
    pub fn name(self) -> &'static str {
        match self {
            Self::DeepSeek => "DeepSeek",
            Self::Deepgram => "Deepgram",
            Self::ElevenLabs => "ElevenLabs",
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
        }
    }

    /// `keys.get`'s `offers` entry (#8362, #8367): `kind` sets the model
    /// services and the voice apart, `fields` the plain fields beside the
    /// key. No model list yet: the service's default.
    pub fn offer(self) -> Value {
        json!({
            "service": self.id(),
            "name": self.name(),
            "kind": if self == Self::ElevenLabs { "voice" } else { "model" },
            "models": [],
            "base_url": self == Self::DeepSeek,
            "fields": if self == Self::ElevenLabs { json!(["voice_id"]) } else { json!([]) },
        })
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
}

impl fmt::Debug for Given {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Given")
            .field("service", &self.service)
            .field("key", &"<hidden>")
            .field("base_url", &self.base_url)
            .field("voice_id", &self.voice_id)
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
    };
    Ok(Given {
        service,
        key,
        base_url,
        voice_id: None,
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
            .ok_or_else(|| "the service is deepseek, deepgram or elevenlabs".to_owned())
    };
    if object.contains_key("set") {
        if object
            .keys()
            .any(|field| !["set", "key", "base_url", "voice_id"].contains(&field.as_str()))
        {
            return Err("a key is set with set, key, base_url and voice_id only".into());
        }
        let set = service("set")?;
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
            Self::Refused => "that key was refused".into(),
            Self::VoiceRefused => {
                "that key works, but that voice ID isn't on its ElevenLabs account; nothing changed"
                    .into()
            }
            Self::Unreached => format!(
                "the lamp couldn't reach {} to check that key; nothing changed",
                service.name()
            ),
            Self::NotKept => "the key could not be kept; the lamp uses its own now".into(),
        }
    }

    /// The answer's code in the contract (#2094, El 11:1x): `works`,
    /// `refused`, `unreached` or `not_kept`.
    pub fn code(self) -> &'static str {
        match self {
            Self::Works => "works",
            Self::Refused => "refused",
            Self::VoiceRefused => "voice_refused",
            Self::Unreached => "unreached",
            Self::NotKept => "not_kept",
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
        Service::DeepSeek => format!(
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
    /// A test lamp's `LAMP_OWN_ELEVENLABS_URL` (`lamp_msgs::own_voice`):
    /// the check goes there, as the voice's speech will.
    pub switched: Option<String>,
    https_only: bool,
}

impl Default for HttpCheck {
    fn default() -> Self {
        Self {
            deepgram: DEEPGRAM_CHECK_URL.to_owned(),
            elevenlabs: ELEVENLABS_API.to_owned(),
            switched: lamp_msgs::own_voice::switched_from_environment(),
            https_only: true,
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
        }
    }

    async fn ask(&self, given: &Given) -> Option<u16> {
        let client = reqwest::Client::builder()
            .tls_backend_preconfigured(tls()?)
            .https_only(self.https_only)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(CHECK_WITHIN)
            .build()
            .ok()?;
        let request = match given.service {
            Service::DeepSeek => client
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
        Some(response.status().as_u16())
    }
}

impl Check for HttpCheck {
    fn check(&mut self, given: &Given) -> Verdict {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return Verdict::Unreached;
        };
        runtime
            .block_on(self.ask(given))
            .map_or(Verdict::Unreached, |status| match (given.service, status) {
                (Service::ElevenLabs, 400 | 404) => Verdict::VoiceRefused,
                _ => Verdict::of_status(status),
            })
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
        if let Some(url) = &given.base_url {
            remove(&self.0.join(given.service.key_file()))?;
            write_private(
                &self.0.join(DEEPSEEK_URL_FILE),
                format!("{url}\n").as_bytes(),
            )?;
        }
        if let Some(id) = &given.voice_id {
            write_private(
                &self.0.join(ELEVENLABS_VOICE_FILE),
                format!("{id}\n").as_bytes(),
            )?;
        }
        write_private(&self.0.join(given.service.key_file()), given.key.as_bytes())
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
        })
    }

    /// The kept ElevenLabs voice ID, if any.
    pub fn voice_id(&self) -> Option<String> {
        fs::read_to_string(self.0.join(ELEVENLABS_VOICE_FILE))
            .ok()
            .map(|id| id.trim().to_owned())
            .filter(|id| !id.is_empty())
    }

    /// Remove a service's key, and DeepSeek's endpoint after it. One that is
    /// not there is removed already.
    pub fn remove(&self, service: Service) -> io::Result<()> {
        remove(&self.0.join(service.key_file()))?;
        match service {
            Service::DeepSeek => remove(&self.0.join(DEEPSEEK_URL_FILE))?,
            Service::ElevenLabs => remove(&self.0.join(ELEVENLABS_VOICE_FILE))?,
            Service::Deepgram => {}
        }
        Ok(())
    }

    /// Whether the person's key is there for `service`.
    pub fn has(&self, service: Service) -> bool {
        self.0.join(service.key_file()).is_file()
    }

    /// `{"deepseek": "own" | "lamp", "deepgram": "own" | "lamp", "since":
    /// {"deepseek", "deepgram"}, "deepseek_host"}`: whose key, since when it
    /// was kept (RFC 3339 UTC, `null` for the lamp's own) and the host
    /// DeepSeek's goes to (`null` for the lamp's own). Never a key.
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
        which.insert(
            "offers".into(),
            Value::Array(Service::ALL.into_iter().map(Service::offer).collect()),
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
pub fn check_and_keep(given: &Given, store: &Store, check: &mut impl Check) -> (Verdict, bool) {
    match check.check(given) {
        Verdict::Works => match store.keep(given) {
            Ok(()) => (Verdict::Works, true),
            Err(_) => {
                let _ = store.remove(given.service);
                (Verdict::NotKept, true)
            }
        },
        verdict => (verdict, false),
    }
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
    /// The journal's line: which service and what happened, never a key.
    pub said: String,
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
            Verdict::Refused => "refused by the service, not kept",
            Verdict::VoiceRefused => "the voice ID was not on its account, not kept",
            Verdict::Unreached => "the check reached nothing, not kept",
            Verdict::NotKept => "it works but could not be written, the lamp's own used",
        }
    );
    let done = Some(Done {
        restart_voice: changed,
        said,
    });
    if verdict == Verdict::Works {
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
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let reply = reply.to_owned();
        let (stop, stopped) = std::sync::mpsc::channel::<()>();
        let thread = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = vec![0; 8_192];
            let n = socket.read(&mut request).unwrap();
            socket.write_all(reply.as_bytes()).unwrap();
            drop(socket);
            // Up to the check's bound, as before, unless stopped sooner.
            let _ = stopped.recv_timeout(Duration::from_millis(300));
            listener.set_nonblocking(true).unwrap();
            assert!(listener.accept().is_err(), "a second request with the key");
            String::from_utf8_lossy(&request[..n]).to_lowercase()
        });
        (format!("http://{address}"), Served { stop, thread })
    }

    /// The server [`answering_with`] runs.
    struct Served {
        stop: std::sync::mpsc::Sender<()>,
        thread: std::thread::JoinHandle<String>,
    }

    impl Served {
        /// What it was asked, once it is stopped and has looked once more.
        fn join(self) -> std::thread::Result<String> {
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
}
