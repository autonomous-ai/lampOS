//! The setup server (#806): V1's setup API with V1's reply shapes, so V1's own
//! page works unchanged behind V1's nginx (where `os-server` listened), or a
//! small page of this crate's own when V1's is not on the lamp.
//!
//! It runs only while the lamp is not set up: the keeper starts it instead of
//! the lamp when the setup marker is missing (#702). Once the home's Wi-Fi is
//! joined and the person's settings are saved, it writes the marker, and the
//! keeper starts the lamp. It goes on answering for a while, so the page can
//! follow the lamp onto the home network and see that it is done.

use crate::wifi::{self, Host, Joined};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fmt,
    fs::{self, OpenOptions},
    io::{self, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

fn default_interface() -> String {
    "wlan0".into()
}
fn default_hotspot() -> String {
    "192.168.100.1".into()
}
fn default_credential() -> PathBuf {
    "/etc/lampOS/voice/credential".into()
}
fn default_policy_key() -> PathBuf {
    "/etc/lampOS/policy/jev.key".into()
}
fn default_zoneinfo() -> PathBuf {
    "/usr/share/zoneinfo".into()
}
fn default_localtime() -> PathBuf {
    "/etc/localtime".into()
}
fn default_timezone_file() -> PathBuf {
    "/etc/timezone".into()
}
fn default_zone_env() -> PathBuf {
    "/etc/lampOS/zone.env".into()
}
fn default_backend() -> PathBuf {
    "/etc/lampOS/setup/backend.json".into()
}

/// The server's written configuration.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Where to answer: `127.0.0.1:5000` behind V1's nginx, or `0.0.0.0:80`
    /// with `page` when serving this crate's own page.
    pub listen: String,
    /// Written once setup is done; the keeper starts the lamp when it appears.
    pub marker: PathBuf,
    /// The voice's credential file, named by `provider.json`. The key is
    /// the user's, posted down by the account page or the app as V1 takes it,
    /// and written here root's, 0600 (the owner, #806, 2026-10-16 16:13).
    #[serde(default = "default_credential")]
    pub credential: PathBuf,
    /// The Policy's judge key (`jev.key`), the same key, written the same way.
    #[serde(default = "default_policy_key")]
    pub policy_key: PathBuf,
    /// The person's name, for the voice to give the model.
    pub name: PathBuf,
    /// wpa_supplicant's two-letter country.
    pub country: String,
    #[serde(default = "default_interface")]
    pub interface: String,
    #[serde(default = "default_hotspot")]
    pub hotspot_address: String,
    /// Serve this crate's page at `/` and `/setup`, and 204 to the phones'
    /// sign-in checks, as V1's nginx does.
    #[serde(default)]
    pub page: bool,
    #[serde(default = "default_zoneinfo")]
    pub zoneinfo: PathBuf,
    #[serde(default = "default_localtime")]
    pub localtime: PathBuf,
    #[serde(default = "default_timezone_file")]
    pub timezone_file: PathBuf,
    /// The zone schedule and skills read (`LAMP_ZONE`, `LAMP_UTC_OFFSET_MINUTES`),
    /// which install first writes from the image's zone: written again with
    /// the person's (#820). Expression and memory follow `/etc/localtime`.
    #[serde(default = "default_zone_env")]
    pub zone_env: PathBuf,
    /// The backend's link from the setup request (#1396): the MQTT broker, its
    /// credentials, the two channels, the device's id and the backend's base.
    /// The Autonomous app posts them as it does to V1; kept root's, 0600, for
    /// `lamp-cloud` to bring the lamp online in the app later. Nothing reads
    /// it yet; the reset's plan removes it.
    #[serde(default = "default_backend")]
    pub backend: PathBuf,
    /// Run once the hotspot is up and once serving ends: V1's nginx, which
    /// serves V1's page and passes `/api/` here, is on only during setup.
    #[serde(default)]
    pub web_start: Vec<String>,
    #[serde(default)]
    pub web_stop: Vec<String>,
    /// Run once the hotspot is up, before answering, to free the port this
    /// answers on. With `page`, `systemctl stop nginx`: the image's
    /// `device-ap-mode` restarts nginx whether or not V1's page is used, and
    /// nginx holds port 80. A failure is only logged, since a lamp without
    /// nginx has nothing to stop; a port still held ends serving at once.
    #[serde(default)]
    pub free_port: Vec<String>,
    /// Show the setup light on the ring (`crate::light`): soft white while
    /// waiting, soft blue while joining, amber after a failed join. Off when
    /// left out; the product's configuration turns it on.
    #[serde(default)]
    pub light: bool,
}

impl Config {
    pub fn check(&self) -> Result<(), String> {
        for path in [
            &self.marker,
            &self.credential,
            &self.policy_key,
            &self.name,
            &self.zoneinfo,
            &self.localtime,
            &self.timezone_file,
            &self.zone_env,
            &self.backend,
        ] {
            if !path.is_absolute() {
                return Err(format!("{} is not absolute", path.display()));
            }
        }
        wifi::wpa_conf(&self.country, "check", "").map_err(|_| "country is two capitals")?;
        self.listen
            .parse::<std::net::SocketAddr>()
            .map_err(|_| "listen is an address and port")?;
        for argv in [&self.web_start, &self.web_stop, &self.free_port] {
            if argv
                .first()
                .is_some_and(|program| !Path::new(program).is_absolute())
            {
                return Err(
                    "web_start, web_stop and free_port are commands with an absolute program"
                        .into(),
                );
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Idle,
    Connecting,
    Connected,
    Failed,
}

/// V1's `GET /api/device/setup/status` data, field for field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Status {
    pub phase: Phase,
    pub lan_ip: String,
    pub error: String,
    pub mac: String,
    /// Setup runs since start, so the page can tell its own verdict from a
    /// leftover one.
    pub run: u64,
    pub set_up_completed: bool,
    pub runtime_phase: String,
    /// Which lamp this is, beyond V1's fields: the Autonomous app keeps V1's
    /// Lamp and this one as two products and stops before the setup POST when
    /// the lamp on the hotspot is not the one claimed (#1396, Fourth's (a)).
    pub generation: &'static str,
}

/// The `generation` a V2 lamp's setup reports; V1's server has no such field.
pub const GENERATION: &str = "lamp-v2";

impl Status {
    pub fn new(mac: String) -> Self {
        Self {
            phase: Phase::Idle,
            lan_ip: String::new(),
            error: String::new(),
            mac,
            run: 0,
            set_up_completed: false,
            runtime_phase: String::new(),
            generation: GENERATION,
        }
    }
}

/// V1's `SetupRequest`. V1's page and the Autonomous app also send channel,
/// voice and other fields; those are accepted and ignored. The backend's link
/// (the MQTT four, `fa_channel`, `fd_channel`, `device_id`, `llm_base_url`)
/// is kept (#1396). `llm_api_key` is the user's voice key, as in V1 (#806);
/// `name` and `timezone` are V2's own.
#[derive(Clone, Default, Deserialize)]
pub struct SetupRequest {
    #[serde(default)]
    pub ssid: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub llm_api_key: String,
    #[serde(default, deserialize_with = "text")]
    pub llm_base_url: String,
    #[serde(default, deserialize_with = "text")]
    pub device_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub timezone: String,
    #[serde(default, deserialize_with = "text")]
    pub mqtt_endpoint: String,
    /// A number from the app, a string from a link: either is taken.
    #[serde(default, deserialize_with = "port")]
    pub mqtt_port: Option<u16>,
    #[serde(default, deserialize_with = "text")]
    pub mqtt_username: String,
    #[serde(default, deserialize_with = "text")]
    pub mqtt_password: String,
    #[serde(default, deserialize_with = "text")]
    pub fa_channel: String,
    #[serde(default, deserialize_with = "text")]
    pub fd_channel: String,
}

/// A link field as text: a string as given, a number or `true`/`false` as its
/// digits or word, and `null` (or anything else) as empty. The backend's link
/// never fails a setup by its shape (the reviewer on #1406): a field it cannot
/// use leaves the link out, and `backend_link` names it.
fn text<'de, D: serde::Deserializer<'de>>(given: D) -> Result<String, D::Error> {
    Ok(match Value::deserialize(given)? {
        Value::String(text) => text,
        Value::Number(number) => number.to_string(),
        Value::Bool(word) => word.to_string(),
        _ => String::new(),
    })
}

/// A port given as a number or as digits; anything else, or 0, is none.
fn port<'de, D: serde::Deserializer<'de>>(given: D) -> Result<Option<u16>, D::Error> {
    Ok(match Value::deserialize(given)? {
        Value::Number(number) => number.as_u64().and_then(|port| u16::try_from(port).ok()),
        Value::String(digits) => digits.trim().parse().ok(),
        _ => None,
    }
    .filter(|port| *port != 0))
}

/// The backend's link as setup received it (#1396), written for `lamp-cloud`.
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct Backend {
    pub device_id: String,
    pub llm_base_url: String,
    pub mqtt_endpoint: String,
    pub mqtt_port: u16,
    pub mqtt_username: String,
    pub mqtt_password: String,
    pub fa_channel: String,
    pub fd_channel: String,
}

impl fmt::Debug for Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Backend")
            .field("device_id", &self.device_id)
            .field("llm_base_url", &self.llm_base_url)
            .field("mqtt_endpoint", &self.mqtt_endpoint)
            .field("mqtt_port", &self.mqtt_port)
            .field("mqtt_username", &self.mqtt_username)
            .field("mqtt_password", &HIDDEN)
            .field("fa_channel", &self.fa_channel)
            .field("fd_channel", &self.fd_channel)
            .finish()
    }
}

/// The link, when the request names one (an `mqtt_endpoint`) and it is whole:
/// the device's id, a port and both channels too, every field at most 512
/// bytes with no space or control character (the password may hold spaces;
/// `llm_base_url`, the username and the password may be empty, for an
/// anonymous broker). `Err` names the fields that are missing or not valid,
/// never their values; an empty list means no link was named, as from V1's
/// page or this crate's own. Either way setup goes on: the lamp joins the
/// Wi-Fi and talks without it, and only its place in the app waits.
pub fn backend_link(request: &SetupRequest) -> Result<Backend, Vec<&'static str>> {
    if request.mqtt_endpoint.trim().is_empty() {
        return Err(vec![]);
    }
    let mut bad = vec![];
    let mut plain = |name: &'static str, field: &str, needed: bool| {
        let field = field.trim();
        if field.len() > 512
            || !field.bytes().all(|b| b.is_ascii_graphic())
            || (needed && field.is_empty())
        {
            bad.push(name);
        }
        field.to_owned()
    };
    let device_id = plain("device_id", &request.device_id, true);
    let llm_base_url = plain("llm_base_url", &request.llm_base_url, false);
    let mqtt_endpoint = plain("mqtt_endpoint", &request.mqtt_endpoint, true);
    let mqtt_username = plain("mqtt_username", &request.mqtt_username, false);
    let fa_channel = plain("fa_channel", &request.fa_channel, true);
    let fd_channel = plain("fd_channel", &request.fd_channel, true);
    let secret = &request.mqtt_password;
    if secret.len() > 512 || secret.chars().any(char::is_control) {
        bad.push("mqtt_password");
    }
    if request.mqtt_port.is_none() {
        bad.push("mqtt_port");
    }
    if !bad.is_empty() {
        return Err(bad);
    }
    Ok(Backend {
        device_id,
        llm_base_url,
        mqtt_endpoint,
        mqtt_port: request.mqtt_port.unwrap_or_default(),
        mqtt_username,
        mqtt_password: secret.clone(),
        fa_channel,
        fd_channel,
    })
}

/// A request that passed every check, ready to act on.
#[derive(Clone, PartialEq, Eq)]
pub struct Accepted {
    pub ssid: String,
    pub password: String,
    /// The user's voice key, when one was posted down. None keeps the key
    /// already there, as on a lamp that ran V1 (`install` copied it).
    pub credential: Option<String>,
    pub name: Option<String>,
    pub timezone: Option<String>,
    /// The backend's link, when the request carried a whole one.
    pub backend: Option<Backend>,
}

/// Shown in place of a secret, so a debug line never logs the Wi-Fi password
/// or the user's key (Nine on #1120).
const HIDDEN: &str = "<hidden>";

impl fmt::Debug for SetupRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SetupRequest")
            .field("ssid", &self.ssid)
            .field("password", &HIDDEN)
            .field("llm_api_key", &HIDDEN)
            .field("llm_base_url", &self.llm_base_url)
            .field("device_id", &self.device_id)
            .field("name", &self.name)
            .field("timezone", &self.timezone)
            .field("mqtt_endpoint", &self.mqtt_endpoint)
            .field("mqtt_port", &self.mqtt_port)
            .field("mqtt_username", &self.mqtt_username)
            .field("mqtt_password", &HIDDEN)
            .field("fa_channel", &self.fa_channel)
            .field("fd_channel", &self.fd_channel)
            .finish()
    }
}

impl fmt::Debug for Accepted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Accepted")
            .field("ssid", &self.ssid)
            .field("password", &HIDDEN)
            .field("credential", &self.credential.as_ref().map(|_| HIDDEN))
            .field("name", &self.name)
            .field("timezone", &self.timezone)
            .field("backend", &self.backend)
            .finish()
    }
}

/// Check a request before anything is touched; the message is for the page.
pub fn check(request: &SetupRequest, config: &Config) -> Result<Accepted, String> {
    wifi::wpa_conf(&config.country, &request.ssid, &request.password).map_err(|why| {
        match why {
            wifi::Refused::SsidEmpty => "choose a Wi-Fi network",
            wifi::Refused::SsidTooLong => "the Wi-Fi name is longer than 32 bytes",
            wifi::Refused::SsidControl => "the Wi-Fi name has a character the lamp cannot use",
            wifi::Refused::PasswordLength => "a Wi-Fi password has 8 to 63 characters",
            wifi::Refused::PasswordCharacters => {
                "the Wi-Fi password has a character the lamp cannot use"
            }
            wifi::Refused::Country => "the lamp's country is not set",
        }
        .to_owned()
    })?;
    let credential = request.llm_api_key.trim();
    if credential.len() > 512 || !credential.bytes().all(|b| b.is_ascii_graphic()) {
        return Err("the voice's key has a character it cannot have, or is too long".into());
    }
    // No key keeps the one there, so a lamp without one (new, or after a
    // reset wiped it) would finish setup with Wi-Fi and no voice, which V1
    // refused (`validate:"required"`). Either file missing needs the key, so
    // the voice and the Policy are set up together (the reviewer on #1120).
    if credential.is_empty() && !(config.credential.is_file() && config.policy_key.is_file()) {
        return Err("the lamp needs its voice key".into());
    }
    let name = request.name.trim();
    if name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err("a name has at most 64 characters".into());
    }
    // A zone the lamp's zone data lacks does not block setup: the lamp keeps
    // its own zone, as a V1 lamp did, and the person can set it later.
    let timezone = request.timezone.trim();
    Ok(Accepted {
        ssid: request.ssid.trim().to_owned(),
        password: request.password.clone(),
        credential: (!credential.is_empty()).then(|| credential.to_owned()),
        name: (!name.is_empty()).then(|| name.to_owned()),
        timezone: zone_known(timezone, &config.zoneinfo).then(|| timezone.to_owned()),
        backend: backend_link(request).ok(),
    })
}

/// An IANA name such as `Asia/Ho_Chi_Minh` whose file, links followed, is a
/// zone file (`TZif`) inside the zone data. That refuses `localtime`, which on
/// Debian links to `/etc/localtime` and would link it to itself, and the data's
/// text files such as `leapseconds`.
pub fn zone_known(zone: &str, zoneinfo: &Path) -> bool {
    if zone.is_empty()
        || zone.len() > 64
        || !zone
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
        || !zone
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/_+-".contains(&b))
    {
        return false;
    }
    let (Ok(root), Ok(file)) = (zoneinfo.canonicalize(), zoneinfo.join(zone).canonicalize()) else {
        return false;
    };
    let mut magic = [0u8; 4];
    file.starts_with(&root)
        && fs::File::open(&file)
            .and_then(|mut opened| io::Read::read_exact(&mut opened, &mut magic))
            .is_ok()
        && &magic == b"TZif"
}

/// What a finished setup writes. Injected for tests.
pub trait Save {
    /// The user's voice key, to the voice's credential and the Policy's key.
    fn credential(&mut self, key: &str) -> io::Result<()>;
    fn name(&mut self, name: &str) -> io::Result<()>;
    fn timezone(&mut self, zone: &str) -> io::Result<()>;
    /// The backend's link, root's and private (#1396).
    fn backend(&mut self, link: &Backend) -> io::Result<()>;
    fn marker(&mut self) -> io::Result<()>;
}

/// The temporary files a setup that crashed mid-write may have left, each of
/// which may hold a secret, the user's key among them: removed at every
/// start (Thirteen on #1016).
pub fn leftover_temporaries(config: &Config) -> Vec<PathBuf> {
    [
        &config.name,
        &config.marker,
        &config.credential,
        &config.policy_key,
        &config.backend,
    ]
    .into_iter()
    .map(|path| path.with_extension("lamp-setup-tmp"))
    .collect()
}

/// Write a private file durably: temporary file, sync, rename, directory sync.
pub fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temporary = stage_private(path, bytes)?;
    put_in_place(&temporary, path)
}

/// `zone.env` for `zone`, `offset_minutes` east of UTC now: the two lines
/// install writes (`crates/update`), which schedule and skills read.
pub fn zone_env(zone: &str, offset_minutes: i32) -> String {
    format!(
        "# The lamp's zone, for schedule and skills (docs/install.md).\n\
         # Written by lamp-setup from the zone the person's phone gave.\n\
         LAMP_ZONE={zone}\nLAMP_UTC_OFFSET_MINUTES={offset_minutes}\n"
    )
}

/// The first half of `write_private`: `bytes` in a new private temporary
/// file beside `path`, synced. Returns the temporary file.
fn stage_private(path: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
    stage(path, bytes, 0o600)
}

/// `bytes` in a new temporary file beside `path` with `mode`, synced.
fn stage(path: &Path, bytes: &[u8], mode: u32) -> io::Result<PathBuf> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("lamp-setup-tmp");
    let _ = fs::remove_file(&temporary);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(&temporary)?;
    if let Err(why) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        let _ = fs::remove_file(&temporary);
        return Err(why);
    }
    Ok(temporary)
}

/// The second half: the temporary file renamed over `path`, its folder synced.
fn put_in_place(temporary: &Path, path: &Path) -> io::Result<()> {
    if let Err(why) = fs::rename(temporary, path) {
        let _ = fs::remove_file(temporary);
        return Err(why);
    }
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

/// The files named in the configuration.
pub struct Files<'a>(pub &'a Config);

impl Save for Files<'_> {
    fn credential(&mut self, key: &str) -> io::Result<()> {
        // Both written before either is renamed, so the voice and the Policy
        // keep one key unless a rename itself fails (Nine on #1120).
        let voice = stage_private(&self.0.credential, key.as_bytes())?;
        let policy = match stage_private(&self.0.policy_key, key.as_bytes()) {
            Ok(policy) => policy,
            Err(why) => {
                let _ = fs::remove_file(&voice);
                return Err(why);
            }
        };
        if let Err(why) = put_in_place(&policy, &self.0.policy_key) {
            let _ = fs::remove_file(&voice);
            return Err(why);
        }
        put_in_place(&voice, &self.0.credential)
    }
    fn name(&mut self, name: &str) -> io::Result<()> {
        write_private(&self.0.name, format!("{name}\n").as_bytes())
    }
    fn timezone(&mut self, zone: &str) -> io::Result<()> {
        // V1's `timezone.go`: /etc/localtime links to the zone, /etc/timezone
        // names it. The link is made beside and renamed into place.
        let link = self.0.localtime.with_extension("lamp-setup-tmp");
        let _ = fs::remove_file(&link);
        std::os::unix::fs::symlink(self.0.zoneinfo.join(zone), &link)?;
        fs::rename(&link, &self.0.localtime)?;
        if let Some(parent) = self.0.localtime.parent() {
            fs::File::open(parent)?.sync_all()?;
        }
        let mut zone_line = zone.to_owned();
        zone_line.push('\n');
        let temporary = self.0.timezone_file.with_extension("lamp-setup-tmp");
        let _ = fs::remove_file(&temporary);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o644)
            .open(&temporary)?;
        file.write_all(zone_line.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temporary, &self.0.timezone_file)?;
        if let Some(parent) = self.0.timezone_file.parent() {
            fs::File::open(parent)?.sync_all()?;
        }
        // Schedule and skills read the zone from install's file (#820). A
        // zone the parts' own zone data lacks leaves it as it was: schedule
        // would refuse to start on it. The offset is today's, for a unit that
        // still takes one; the product's skills follow /etc/localtime (#1384).
        if let Ok(local) = lamp_local_time::LocalTime::zone(zone) {
            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |since| since.as_millis() as u64);
            let text = zone_env(zone, local.offset_minutes_at(now_ms));
            let temporary = stage(&self.0.zone_env, text.as_bytes(), 0o644)?;
            put_in_place(&temporary, &self.0.zone_env)?;
        }
        Ok(())
    }
    fn backend(&mut self, link: &Backend) -> io::Result<()> {
        let mut bytes = serde_json::to_vec_pretty(link).map_err(io::Error::other)?;
        bytes.push(b'\n');
        write_private(&self.0.backend, &bytes)
    }
    fn marker(&mut self) -> io::Result<()> {
        write_private(&self.0.marker, b"")
    }
}

/// Bring the lamp up for setup, in this order: the hotspot, as V1's
/// `os-server` raises it at a first boot; `free_port`; `web_start`; and only
/// then the lamp's name, for `mac`. On a fresh image the hotspot script,
/// `device-ap-mode`, is what names the host `lamp-xxxx`. A name read before
/// it is the image's own, and a page that lost the hotspot before it learnt
/// `lan_ip` would look for the wrong `<name>.local`.
pub fn start(
    config: &Config,
    host: &mut impl Host,
    mut run: impl FnMut(&[String]) -> io::Result<()>,
    hostname: impl FnOnce() -> String,
    mut log: impl FnMut(&str),
) -> io::Result<Status> {
    host.hotspot()?;
    if !config.free_port.is_empty()
        && let Err(error) = run(&config.free_port)
    {
        log(&format!(
            "{} failed ({error}); answering goes on",
            config.free_port.join(" ")
        ));
    }
    if !config.web_start.is_empty() {
        run(&config.web_start)?;
    }
    Ok(Status::new(hostname()))
}

/// [`start`] when serving may be stopped (`systemctl stop`, a shutdown):
/// `None` when it was. A stop seen first skips the hotspot. A start that
/// fails with a stop pending is that stop, not a failure: systemd's stop
/// reaches the whole unit, so the hotspot script dies of it too, and the
/// unit must not read failed (the reviewers on #1336 and #1341). The start's
/// error is kept in the line, in case the script failed by itself first.
pub fn start_unless_stopped(
    mut stopped: impl FnMut() -> bool,
    start: impl FnOnce() -> io::Result<Status>,
    mut log: impl FnMut(&str),
) -> io::Result<Option<Status>> {
    if stopped() {
        log("asked to stop before the hotspot");
        return Ok(None);
    }
    match start() {
        Ok(status) => Ok(Some(status)),
        Err(error) if stopped() => {
            log(&format!(
                "asked to stop while the hotspot came up ({error})"
            ));
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

/// One setup run, after V1's 2 s pause: join, then save, then the marker.
/// `station_ip` is the address the home network gave, polled every second
/// while joining so the page learns it before the hotspot is gone.
pub fn run(
    status: &Arc<Mutex<Status>>,
    accepted: &Accepted,
    country: &str,
    host: &mut impl Host,
    save: &mut impl Save,
    station_ip: impl Fn() -> Option<String> + Send + Sync + 'static,
) {
    let station_ip = Arc::new(station_ip);
    {
        let mut status = status.lock().unwrap();
        status.run += 1;
        status.phase = Phase::Connecting;
        status.error.clear();
        status.lan_ip.clear();
    }
    let watching = Arc::new(Mutex::new(true));
    let watcher = {
        let status = Arc::clone(status);
        let watching = Arc::clone(&watching);
        let station_ip = Arc::clone(&station_ip);
        thread::spawn(move || {
            while *watching.lock().unwrap() {
                if let Some(address) = station_ip() {
                    status.lock().unwrap().lan_ip = address;
                }
                thread::sleep(Duration::from_secs(1));
            }
        })
    };
    // Kept to put back if the settings cannot be saved after a good join.
    let previous = host.read_conf().ok().flatten();
    let joined = wifi::join(host, country, &accepted.ssid, &accepted.password);
    *watching.lock().unwrap() = false;
    let _ = watcher.join();
    let failure = match joined {
        Joined::Failed { reason } => Some(reason.detail().to_owned()),
        Joined::Connected => {
            // The key last, just before the marker, so a name or zone that
            // cannot be saved leaves the key there as it was (the reviewer on
            // #1120).
            let saved = match &accepted.name {
                Some(name) => save.name(name),
                None => Ok(()),
            }
            .and_then(|()| match &accepted.timezone {
                Some(zone) => save.timezone(zone),
                None => Ok(()),
            })
            .map(|()| {
                // Best effort (Third on #1401): the lamp works without the
                // link, and only its place in the app waits for a later setup.
                if let Some(link) = &accepted.backend
                    && let Err(why) = save.backend(link)
                {
                    eprintln!(
                        "lamp-setup: the backend's link could not be saved ({why}); setup goes on without it"
                    );
                }
            })
            .and_then(|()| match &accepted.credential {
                Some(key) => save.credential(key),
                None => Ok(()),
            })
            .and_then(|()| save.marker());
            match saved {
                Ok(()) => None,
                Err(_) => {
                    // Joined but not finished: no marker, the previous Wi-Fi
                    // file back, and the hotspot, so the person can try again.
                    // What was saved before the failure stays; the next run
                    // writes it again.
                    let _ = host.write_conf(previous.as_deref());
                    let _ = host.hotspot();
                    Some("the lamp joined the Wi-Fi but could not save its settings".into())
                }
            }
        }
    };
    let address = failure.is_none().then(|| station_ip()).flatten();
    let mut status = status.lock().unwrap();
    match failure {
        None => {
            // The address once more, now that the join is done: the watcher
            // may not have caught it before the join returned.
            if let Some(address) = address {
                status.lan_ip = address;
            }
            status.phase = Phase::Connected;
            status.set_up_completed = true;
            // V1's page shows "Preparing your robot's voice…" while this is
            // `preparing`, and reloads into its own settings at `ready`, which
            // need V1's server. So it stays `preparing`: the lamp's greeting,
            // as in V1, is how the person learns it is done.
            status.runtime_phase = "preparing".into();
        }
        Some(error) => {
            status.phase = Phase::Failed;
            status.error = error;
        }
    }
}

/// V1's reply wrapper: `{"status":1,"data":…,"message":null}`.
pub fn success(data: Value) -> Value {
    json!({"status": 1, "data": data, "message": null})
}

pub fn failure(message: &str) -> Value {
    json!({"status": 0, "data": null, "message": message})
}

/// The station address the home network gave `interface`, if any: not the
/// hotspot's own.
pub fn station_ip(interface: &str, hotspot: &str) -> Option<String> {
    let output = std::process::Command::new("ip")
        .args(["-4", "-o", "addr", "show", "dev", interface])
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .skip_while(|word| *word != "inet")
        .nth(1)
        .and_then(|cidr| cidr.split('/').next())
        .filter(|address| *address != hotspot)
        .map(String::from)
}
