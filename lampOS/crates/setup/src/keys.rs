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

/// Whether `/proc/<pid>/cgroup`'s text puts the process in [`ASKER_UNIT`]
/// at the system's place for it, [`ASKER_CGROUP`] (or a cgroup under it):
/// the unified hierarchy's line (`0::…`), or systemd's own on a hybrid one
/// (`…:name=systemd:…`).
pub fn from_asker(cgroup: &str) -> bool {
    cgroup.lines().any(|line| {
        let mut fields = line.splitn(3, ':');
        let (Some(_), Some(controllers), Some(path)) =
            (fields.next(), fields.next(), fields.next())
        else {
            return false;
        };
        (controllers.is_empty() || controllers == "name=systemd")
            && path
                .strip_prefix(ASKER_CGROUP)
                .is_some_and(|below| below.is_empty() || below.starts_with('/'))
    })
}

/// Whether the peer on standard input (the socket systemd hands over) is
/// in [`ASKER_UNIT`], by its credentials (`SO_PEERCRED`) and its cgroup.
/// Peer credentials are read on Linux only; elsewhere no peer is.
pub fn peer_is_asker() -> bool {
    #[cfg(target_os = "linux")]
    {
        let stdin = std::io::stdin();
        rustix::net::sockopt::socket_peercred(&stdin).is_ok_and(|peer| {
            fs::read_to_string(format!("/proc/{}/cgroup", peer.pid.as_raw_nonzero()))
                .is_ok_and(|cgroup| from_asker(&cgroup))
        })
    }
    #[cfg(not(target_os = "linux"))]
    false
}

/// A service a person may bring their own key for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Service {
    DeepSeek,
    Deepgram,
}

impl Service {
    pub const ALL: [Service; 2] = [Service::DeepSeek, Service::Deepgram];

    /// The name the app and the reply use.
    pub fn id(self) -> &'static str {
        match self {
            Self::DeepSeek => "deepseek",
            Self::Deepgram => "deepgram",
        }
    }

    /// The name a person reads.
    pub fn name(self) -> &'static str {
        match self {
            Self::DeepSeek => "DeepSeek",
            Self::Deepgram => "Deepgram",
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
        }
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
}

impl fmt::Debug for Given {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Given")
            .field("service", &self.service)
            .field("key", &"<hidden>")
            .field("base_url", &self.base_url)
            .finish()
    }
}

/// What one request line asks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asked {
    /// Check the key and keep it if it works.
    Set(Given),
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
    };
    Ok(Given {
        service,
        key,
        base_url,
    })
}

/// The one line `lamp-cloud` sends: `{"set": service, "key", "base_url"?}`,
/// `{"remove": service}` or `{"which": true}`. Never echoes a key.
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
            .ok_or_else(|| "the service is deepseek or deepgram".to_owned())
    };
    if object.contains_key("set") {
        if object
            .keys()
            .any(|field| !["set", "key", "base_url"].contains(&field.as_str()))
        {
            return Err("a key is set with set, key and base_url only".into());
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
        return Ok(Asked::Set(given(service("set")?, key, base_url)?));
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
            Self::Works => format!("that key works: {}", goes_where(given)),
            Self::Refused => "that key was refused".into(),
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
/// `GET` [`DEEPGRAM_CHECK_URL`] with `Token <key>`.
pub struct HttpCheck {
    /// Deepgram's address, a test's local server in place of it.
    pub deepgram: String,
    https_only: bool,
}

impl Default for HttpCheck {
    fn default() -> Self {
        Self {
            deepgram: DEEPGRAM_CHECK_URL.to_owned(),
            https_only: true,
        }
    }
}

impl HttpCheck {
    /// A check that may ask a plain local server: tests only.
    #[cfg(test)]
    fn local(deepgram: String) -> Self {
        Self {
            deepgram,
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
            .map_or(Verdict::Unreached, Verdict::of_status)
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
        write_private(&self.0.join(given.service.key_file()), given.key.as_bytes())
    }

    /// Remove a service's key, and DeepSeek's endpoint after it. One that is
    /// not there is removed already.
    pub fn remove(&self, service: Service) -> io::Result<()> {
        remove(&self.0.join(service.key_file()))?;
        if service == Service::DeepSeek {
            remove(&self.0.join(DEEPSEEK_URL_FILE))?;
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
        Asked::Set(given) => {
            let (verdict, changed) = check_and_keep(&given, store, check);
            let service = given.service;
            let said = format!(
                "the person's {} key: {}",
                service.name(),
                match verdict {
                    Verdict::Works => "it works, kept",
                    Verdict::Refused => "refused by the service, not kept",
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    /// A local server that answers `status` once; what it was asked.
    fn answering(status: u16) -> (String, std::thread::JoinHandle<String>) {
        answering_with(&format!(
            "HTTP/1.1 {status} X\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
        ))
    }

    /// A local server that answers `reply` to the one request it takes, and
    /// fails the test if a second comes within the check's bound.
    fn answering_with(reply: &str) -> (String, std::thread::JoinHandle<String>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let reply = reply.to_owned();
        let served = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = vec![0; 8_192];
            let n = socket.read(&mut request).unwrap();
            socket.write_all(reply.as_bytes()).unwrap();
            drop(socket);
            listener.set_nonblocking(true).unwrap();
            std::thread::sleep(Duration::from_millis(300));
            assert!(listener.accept().is_err(), "a second request with the key");
            String::from_utf8_lossy(&request[..n]).to_lowercase()
        });
        (format!("http://{address}"), served)
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
}
