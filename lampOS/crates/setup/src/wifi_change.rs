//! The home's Wi-Fi changed from the app after setup, without a reset
//! (#1959, `docs/setup.md`, "The Wi-Fi changed after setup").
//!
//! `lamp-cloud` hands one line to root's helper, `lamp-setup wifi`, on a
//! socket of its own (`lamp-setup-wifi.socket`): the names and the zone's
//! helper has no network, and this one must have it. The change never
//! strands the lamp:
//! 1. the old network's file is kept beside it ([`Rejoin::keep_previous`])
//!    until the new one is confirmed;
//! 2. the new file is written with setup's own writer ([`wifi::wpa_conf`])
//!    and the radio is switched with V1's script, as setup's join does;
//! 3. the new network is proven only when it is the one joined, with a
//!    station address, and the broker `lamp-cloud` is using takes a TCP
//!    connection, within [`JOIN_WAIT`]; and it is kept only once the app
//!    confirms it, within [`CONFIRM_WAIT`] (#1959's P1);
//! 4. otherwise the old file goes back, `wpa_cli reconfigure` first so the
//!    new network is never left loaded, then the radio again, each bounded,
//!    and the old network is waited for, [`BACK_WAIT`] a try, [`BACK_TRIES`]
//!    tries. A lamp whose old network does not come back either stays on it
//!    and keeps trying it: after setup there is no setup page behind the
//!    hotspot, so the hotspot would strand it;
//! 5. a helper cut before it ends (killed, `RuntimeMaxSec`, a power cut)
//!    leaves `.lamp-previous`, which [`restore`] puts back: the unit's
//!    `ExecStopPost=` and `lamp-setup-wifi-restore.service` at boot.
//!
//! The password is in the request line and in the network file, nowhere
//! else: never in a reply, an error or a journal line, and [`Asked`]'s
//! `Debug` hides it.
use crate::wifi::{self, Host, Observation, WRONG_KEY_STOP, classify, wrong_key_failures};
use serde_json::{Value, json};
use std::{
    fmt, io,
    net::{SocketAddr, TcpStream, ToSocketAddrs},
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};

/// The switch on `lamp-cloud`, beside `LAMP_SETTINGS_CHANGE=on`: `on` takes
/// `wifi.set`. Its own, since this is the one change that can take the lamp
/// off the network the app reaches it on.
pub const ENVIRONMENT: &str = "LAMP_SETTINGS_WIFI";

/// The longest request line read, in bytes.
pub const LINE_MOST: usize = 1_024;
/// How long the new network has to be joined and reach the broker: setup's
/// own bound (V1's `setupInternetWait`).
pub const JOIN_WAIT: Duration = wifi::JOIN_WAIT;
/// How long the old network has to come back after a failed change, a try.
pub const BACK_WAIT: Duration = Duration::from_secs(60);
/// How many times the radio is restarted on the way back (#1959's P1).
pub const BACK_TRIES: u32 = 3;
/// How long a proven new network waits for the app's confirmation; without
/// it the lamp goes back as if the join had failed (#1959's P1: the app
/// closed, the phone gone).
pub const CONFIRM_WAIT: Duration = Duration::from_secs(120);
/// How long a proven new network waits for `lamp-cloud`'s link to be up
/// again over it, before its [`CONFIRM_WAIT`] starts (the reviewer-lead's
/// P2 on #2617): the window is the app's, from when it can reach the lamp.
pub const ONLINE_WAIT: Duration = Duration::from_secs(60);
/// How late after the window a confirmation `lamp-cloud` took within it may
/// reach the helper: the line crossing the local socket. `lamp-cloud` takes
/// none after the window's end, so a confirmation is answered once.
pub const CONFIRM_SLACK: Duration = Duration::from_millis(500);
/// The broker's TCP connection, name lookup included.
pub const BROKER_TIMEOUT: Duration = Duration::from_secs(3);

/// One change asked, checked. The password is never shown.
#[derive(Clone, PartialEq, Eq)]
pub struct Asked {
    pub ssid: String,
    pub password: String,
    /// `host:port`, the broker `lamp-cloud` is on now.
    pub broker: String,
}

impl fmt::Debug for Asked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Asked")
            .field("ssid", &self.ssid)
            .field("password", &"<hidden>")
            .field("broker", &self.broker)
            .finish()
    }
}

/// A broker's `host:port`, or why not: a host name or an address, and a
/// port other than 0.
fn broker_ok(address: &str) -> bool {
    let Some((host, port)) = address.rsplit_once(':') else {
        return false;
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    !host.is_empty()
        && host.len() <= 253
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':'))
        && port.parse::<u16>().is_ok_and(|port| port != 0)
}

/// The one line `lamp-cloud` sends, checked: exactly
/// `{"wifi": {"ssid": s, "password": s, "broker": "host:port"}}`. A refusal
/// is setup's page's words, never a value asked.
pub fn parse(line: &str, country: &str) -> Result<Asked, String> {
    if line.len() > LINE_MOST {
        return Err("the request is too long".into());
    }
    let value: Value = serde_json::from_str(line).map_err(|_| "the request is not JSON")?;
    let wifi = value
        .as_object()
        .filter(|object| object.len() == 1)
        .and_then(|object| object.get("wifi"))
        .and_then(Value::as_object)
        .ok_or("the request names no change this lamp makes")?;
    let text = |key: &str| wifi.get(key).and_then(Value::as_str);
    let ssid = text("ssid").ok_or("choose a Wi-Fi network")?;
    let password = text("password").ok_or("the Wi-Fi password must be text")?;
    let broker = text("broker").filter(|broker| broker_ok(broker));
    let broker = broker.ok_or("the lamp is not on its broker")?;
    wifi::wpa_conf(country, ssid, password).map_err(crate::serve::refused_words)?;
    Ok(Asked {
        ssid: ssid.trim().to_owned(),
        password: password.to_owned(),
        broker: broker.to_owned(),
    })
}

/// What a change needs beyond setup's join. Injected for tests.
pub trait Rejoin: Host {
    /// A TCP connection to `address` (`host:port`), made and closed, within
    /// [`BROKER_TIMEOUT`].
    fn broker(&mut self, address: &str) -> bool;
    /// Keep the old network's file beside the live one; `None` removes it.
    fn keep_previous(&mut self, bytes: Option<&[u8]>) -> io::Result<()>;
    /// The old network's file kept beside the live one, if there is one.
    fn read_previous(&mut self) -> io::Result<Option<Vec<u8>>>;
    /// `wpa_cli reconfigure`, bounded: wpa_supplicant reads its file again.
    fn reload(&mut self) -> io::Result<()>;
    /// Tell the asker the new network `ssid` is proven, and wait at most
    /// `most` for the app's confirmation through it.
    fn app_confirms(&mut self, ssid: &str, most: Duration) -> bool;
    /// wpa_supplicant runs `conf`'s network from its memory, its file left
    /// as it is and never saved over: the way back when the old file cannot
    /// be written (the first builder's P3 2 on #2617). On an error it may
    /// hold no network (its networks are removed first): the caller then
    /// [`reload`](Rejoin::reload)s the live file (#2686's P2).
    fn hold(&mut self, conf: &[u8]) -> io::Result<()>;
}

/// The stop-time restore's bound, all its tries (the first builder's
/// follow-up 4 on #2617): inside the unit's `TimeoutStopSec=2min` with a
/// poll's queries and the file's writes to spare, so a shutdown is held at
/// most that. A try starts only while its radio steps fit. The boot's
/// restore is the backstop: the kept file stays while the old network is
/// not back.
pub const RESTORE_MOST: Duration = Duration::from_secs(90);

/// The network's name in a wpa_supplicant file: its first `ssid=` line, as
/// hex (setup's writer) or between quotes (V1's `connect-wifi`, `P"…"` with
/// wpa_cli's escapes). `None` when there is none to read.
pub fn conf_ssid(conf: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(conf);
    let value = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("ssid="))?
        .trim();
    if let Some(quoted) = value.strip_prefix("P\"").and_then(|v| v.strip_suffix('"')) {
        return Some(wifi::decode_wpa(quoted));
    }
    if let Some(quoted) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
        return Some(quoted.to_owned());
    }
    if value.is_empty() || value.len() % 2 != 0 {
        return None;
    }
    let bytes = (0..value.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(value.get(at..at + 2)?, 16).ok())
        .collect::<Option<Vec<u8>>>()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// The `key=value` lines of a wpa_supplicant file's first network, as
/// `SET_NETWORK` takes them (it parses a value as the file does). `None`
/// when it has none.
pub fn network_lines(conf: &[u8]) -> Option<Vec<(String, String)>> {
    let text = String::from_utf8_lossy(conf);
    let mut lines = text.lines().map(str::trim);
    lines.find(|line| line.replace(' ', "") == "network={")?;
    let mut fields = Vec::new();
    for line in lines {
        if line == "}" {
            return (!fields.is_empty()).then_some(fields);
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line.split_once('=')?;
        fields.push((key.trim().to_owned(), value.trim().to_owned()));
    }
    None
}

/// How a change ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Changed {
    /// On the new network, with the broker reached: the old file is gone.
    Joined,
    /// Not taken, and back on the old network with the broker reached.
    Back { why: String },
    /// Not taken, the old file back, and the old network not back within
    /// [`BACK_WAIT`]: the lamp stays on it and keeps trying it.
    NotBack { why: String },
    /// Nothing was changed.
    Refused(String),
}

/// Why the new network was not taken, in the page's words where setup has
/// them. A network joined with an address that cannot reach the broker is
/// said as that, not as "no internet".
fn why_not(observation: &Observation) -> String {
    let reason = classify(observation);
    if reason == wifi::Reason::NoInternet {
        return "connected to the new WiFi, but the lamp's server could not be reached".into();
    }
    reason.detail().into()
}

/// Waits until `confirmed` holds, at most `most`; checks about once a second.
fn wait_for<H: Rejoin>(
    host: &mut H,
    most: Duration,
    mut confirmed: impl FnMut(&mut H) -> bool,
) -> bool {
    let deadline = host.now_ms() + most.as_millis() as u64;
    loop {
        if confirmed(host) {
            return true;
        }
        if host.now_ms() >= deadline {
            return false;
        }
        host.sleep(Duration::from_secs(1));
    }
}

/// Change to `asked`'s network, or come back to the old one.
pub fn change(host: &mut impl Rejoin, country: &str, asked: &Asked) -> Changed {
    let Ok(conf) = wifi::wpa_conf(country, &asked.ssid, &asked.password) else {
        return Changed::Refused("the Wi-Fi could not be written".into());
    };
    let previous = match host.read_conf() {
        Ok(Some(previous)) => previous,
        Ok(None) => return Changed::Refused("the lamp has no Wi-Fi to come back to".into()),
        Err(_) => return Changed::Refused("the lamp's Wi-Fi could not be read".into()),
    };
    if host.keep_previous(Some(&previous)).is_err() {
        return Changed::Refused("the lamp's Wi-Fi could not be kept to come back to".into());
    }
    let ssid = asked.ssid.as_str();
    let mut why = None;
    if host.write_conf(Some(conf.as_bytes())).is_err() || host.station().is_err() {
        why = Some(wifi::Reason::WifiConnectError.detail().to_owned());
    }
    if why.is_none() {
        let deadline = host.now_ms() + JOIN_WAIT.as_millis() as u64;
        let mut attempt = 0u32;
        while host.now_ms() < deadline {
            if host.current_ssid().as_deref() == Some(ssid)
                && host.has_station_ip()
                && host.broker(&asked.broker)
            {
                // Proven; kept only once the app confirms it through it.
                if host.app_confirms(ssid, CONFIRM_WAIT) {
                    forget_previous(host, conf.as_bytes());
                    return Changed::Joined;
                }
                why = Some(format!(
                    "the app did not confirm the new Wi-Fi within {} s",
                    CONFIRM_WAIT.as_secs()
                ));
                break;
            }
            if attempt % 3 == 2 && wrong_key_failures(&host.wpa_events()) >= WRONG_KEY_STOP {
                break;
            }
            attempt += 1;
            host.sleep(Duration::from_secs(1));
        }
        if why.is_none() {
            let observation = Observation {
                events: host.wpa_events(),
                wpa_state: host.wpa_state(),
                has_ip: host.has_station_ip(),
                ssid_matched: host.current_ssid().as_deref() == Some(ssid),
            };
            why = Some(why_not(&observation));
        }
    }
    let why = why.unwrap_or_default();
    // Back: the old file, wpa_supplicant told to read it, the radio, and the
    // old network waited for; each step bounded, the radio tried again. A
    // write that fails is tried again at once (the first builder's P3 2 on
    // #2617), not left to the stop's restore.
    let old_ssid = conf_ssid(&previous);
    if !put_back(host, &previous) {
        // The old file stays kept beside the new one, for `restore`; the
        // radio runs the old network from memory meanwhile (P3 2).
        if held_on_old(host, &previous, &asked.broker, old_ssid.as_deref()) {
            return Changed::Back {
                why: format!(
                    "{why}; the lamp is back on its old Wi-Fi, run from memory since its file could not be written"
                ),
            };
        }
        return Changed::NotBack {
            why: format!("{why}; the old Wi-Fi could not be put back"),
        };
    }
    let back = back_on_old(host, Some(&asked.broker), old_ssid.as_deref(), None);
    // The live file is the old one now: nothing is left to restore.
    let _ = host.keep_previous(None);
    if back {
        Changed::Back {
            why: format!("{why}; the lamp is back on its old Wi-Fi"),
        }
    } else {
        Changed::NotBack {
            why: format!(
                "{why}; the old Wi-Fi did not come back after {BACK_TRIES} tries, and the lamp keeps trying it"
            ),
        }
    }
}

/// The old file written as the live one, [`BACK_TRIES`] tries a second
/// apart. Whether it was.
fn put_back<H: Rejoin>(host: &mut H, previous: &[u8]) -> bool {
    for try_ in 0..BACK_TRIES {
        if host.write_conf(Some(previous)).is_ok() {
            return true;
        }
        if try_ + 1 < BACK_TRIES {
            host.sleep(Duration::from_secs(1));
        }
    }
    false
}

/// After a confirmation, `.lamp-previous` goes, so no later restore undoes
/// the change (the first builder's P3 3 on #2617): its removal is tried
/// [`BACK_TRIES`] times, and if it never goes, it is made the new network's
/// own file, which a restore then puts back unchanged.
fn forget_previous<H: Rejoin>(host: &mut H, conf: &[u8]) {
    for try_ in 0..BACK_TRIES {
        if host.keep_previous(None).is_ok() {
            return;
        }
        if try_ + 1 < BACK_TRIES {
            host.sleep(Duration::from_secs(1));
        }
    }
    if host.keep_previous(Some(conf)).is_err() {
        eprintln!(
            "lamp-setup: wifi: the old Wi-Fi's kept file could not be removed or replaced; a restore would undo this change"
        );
    }
}

/// Whether the old network is back: `ssid` (when the old file names one)
/// the one joined, not merely an address (the first builder's follow-up 1
/// on #2617: one left from the new network, or another's), and the broker
/// reached when one is known.
fn on_old<H: Rejoin>(host: &mut H, broker: Option<&str>, ssid: Option<&str>) -> bool {
    host.has_station_ip()
        && ssid.is_none_or(|ssid| host.current_ssid().as_deref() == Some(ssid))
        && broker.is_none_or(|broker| host.broker(broker))
}

/// The old file is live: `wpa_cli reconfigure` first, so the new network is
/// never left loaded even when the radio's script fails or hangs, then the
/// radio's restart; each try waits [`BACK_WAIT`] for the old network
/// ([`on_old`]), [`BACK_TRIES`] tries. With `most`, all of it ends within
/// that: a try starts only while its two commands' bounds fit, and its wait
/// ends at the bound. Whether the old network is back.
fn back_on_old<H: Rejoin>(
    host: &mut H,
    broker: Option<&str>,
    ssid: Option<&str>,
    most: Option<Duration>,
) -> bool {
    let started = host.now_ms();
    let steps = (wifi::RADIO_TIMEOUT + wifi::QUERY_TIMEOUT).as_millis() as u64;
    for _ in 0..BACK_TRIES {
        let left = |host: &mut H| {
            most.map(|most| (most.as_millis() as u64).saturating_sub(host.now_ms() - started))
        };
        if left(host).is_some_and(|left| left < steps) {
            return false;
        }
        let _ = host.reload();
        let _ = host.station();
        let wait = left(host).map_or(BACK_WAIT, |left| BACK_WAIT.min(Duration::from_millis(left)));
        if wait_for(host, wait, |host| on_old(host, broker, ssid)) {
            return true;
        }
    }
    false
}

/// The old file could not be written: wpa_supplicant runs the old network
/// from memory ([`Rejoin::hold`]), with no radio restart (it would read the
/// new file again), each try waiting [`BACK_WAIT`] for it, [`BACK_TRIES`]
/// tries a second apart. A hold that fails may have removed wpa_supplicant's
/// networks before its failing step (#2686's P2), so it is told to read the
/// live file again at once; and when the old network never came back, it
/// reads it once more, so the lamp ends on the new network that worked, not
/// on none. The kept file stays for the stop's restore, which writes it.
/// Whether the old network is back.
fn held_on_old<H: Rejoin>(host: &mut H, previous: &[u8], broker: &str, ssid: Option<&str>) -> bool {
    for try_ in 0..BACK_TRIES {
        if try_ > 0 {
            host.sleep(Duration::from_secs(1));
        }
        if host.hold(previous).is_err() {
            let _ = host.reload();
            continue;
        }
        if wait_for(host, BACK_WAIT, |host| on_old(host, Some(broker), ssid)) {
            return true;
        }
    }
    let _ = host.reload();
    false
}

/// What [`restore`] found and did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Restored {
    /// No old file was kept: no change was cut.
    Nothing,
    /// The old file is live again (and, when live, the old network is back
    /// with an address); the kept file is gone.
    Put,
    /// The old file is live again, but the old network did not come back
    /// after [`BACK_TRIES`] tries: the kept file stays, for the boot's
    /// restore (the first builder's P3 1 on #2617).
    NotBack,
    /// The kept file could not be read, or the old file not written; it
    /// stays kept (P3 5).
    Failed,
}

/// A change cut before it ended (killed, `RuntimeMaxSec`, a power cut):
/// the old network's file kept beside the live one goes back. `live` after
/// the helper's own stop (`ExecStopPost=`), with wpa_supplicant running: it
/// re-reads the file and the radio restarts, within [`RESTORE_MOST`] (in
/// practice one try: a second's 55 s no longer fits), the old network judged back by its name
/// and an address (no broker is known at the stop). The kept file goes only once that has worked (the first
/// builder's P3 1 on #2617). At boot (`lamp-setup-wifi-restore.service`,
/// before wpa_supplicant and dhcpcd) only the file.
pub fn restore(host: &mut impl Rejoin, live: bool) -> Restored {
    let previous = match host.read_previous() {
        Ok(Some(previous)) => previous,
        Ok(None) => return Restored::Nothing,
        Err(_) => return Restored::Failed,
    };
    if !put_back(host, &previous) {
        return Restored::Failed;
    }
    if live
        && !back_on_old(
            host,
            None,
            conf_ssid(&previous).as_deref(),
            Some(RESTORE_MOST),
        )
    {
        return Restored::NotBack;
    }
    let _ = host.keep_previous(None);
    Restored::Put
}

/// What one request came to: the helper's reply line, its journal line, and
/// what follows the reply. Neither line holds the password, and the journal
/// line holds no network name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answered {
    /// `{"done": {"ssid": s}}` or `{"refused": why}`.
    pub reply: Value,
    pub said: String,
    /// How the change ended, and the broker to judge the old network back
    /// by; `None` when nothing was changed.
    pub changed: Option<(Changed, String)>,
}

/// The helper's answer to one request line.
pub fn answer(line: &str, country: &str, enabled: bool, host: &mut impl Rejoin) -> Answered {
    let refused = |why: &str| Answered {
        reply: json!({"refused": why}),
        said: format!("refused: {why}"),
        changed: None,
    };
    if !enabled {
        return refused("Wi-Fi changes after setup are off");
    }
    let asked = match parse(line.trim_end_matches(['\n', '\r']), country) {
        Ok(asked) => asked,
        Err(why) => return refused(&why),
    };
    // A network's name is the person's: the journal says only its length.
    let name = format!("a name of {} bytes", asked.ssid.len());
    let changed = change(host, country, &asked);
    let (reply, said) = match &changed {
        Changed::Joined => (
            json!({"done": {"ssid": asked.ssid}}),
            format!("the Wi-Fi was changed, to {name}, and the broker reached"),
        ),
        Changed::Back { why } | Changed::NotBack { why } => (
            json!({"refused": why}),
            format!("the Wi-Fi change to {name} was not taken: {why}"),
        ),
        Changed::Refused(why) => return refused(why),
    };
    Answered {
        reply,
        said,
        changed: Some((changed, asked.broker)),
    }
}

/// What the lamp says aloud once on the new network: never its name.
pub const SAY_JOINED: &str = "I'm on the new Wi-Fi now.";
/// What the lamp says aloud once back on the old one.
pub const SAY_BACK: &str = "I couldn't join that one, so I've gone back to the old Wi-Fi.";
/// What the lamp says while neither network is there, at most once a minute.
/// With no network the voice's model cannot be reached, so this line is
/// never asked of it: it is [`RECORDING_LOST`], recorded at build time in
/// the lamp's own voice and played by the voice (`docs/setup.md`). This is
/// its transcript, and the one place its words are kept in code (#3743): it
/// names the reset, not the main button (whose 5 s hold only mutes), and
/// where it is: the pinhole labelled RESET on the back of the base, beside
/// the round POWER button, pushed with a paperclip or SIM pin and held 5 s
/// (the hardware lead on #3743). It says the hold erases, and asks for it
/// only when the Wi-Fi's name or password has really changed.
///
/// It is first said only when the old network has not come back in
/// [`BACK_TRIES`] waits of [`BACK_WAIT`], 3 minutes at least after the
/// lamp left it, so a router's restart (2-3 minutes) never hears it.
pub const SAY_LOST: &str = "I can't reach any Wi-Fi. If you changed your Wi-Fi's name or password, push a paperclip into the hole marked reset on the back of my base and hold it for five seconds. That erases me and starts setup again.";
/// The voice's recording of [`SAY_LOST`], by the name the say service takes
/// (`recording`), the failure notice `wifi_lost` in
/// `/opt/lampOS/voice/notices`, placed by install (`docs/failure-notices.md`,
/// #1844).
pub const RECORDING_LOST: &str = "wifi_lost";
/// [`SAY_LOST`] at most this often.
pub const LOST_EVERY: Duration = Duration::from_secs(60);
/// How long [`SAY_LOST`] is kept up for, while no network comes back.
pub const LOST_MOST: Duration = Duration::from_secs(10 * 60);

/// The line said after `changed`, the same for the reply's every reader.
pub fn line_for(changed: &Changed) -> Option<&'static str> {
    match changed {
        Changed::Joined => Some(SAY_JOINED),
        Changed::Back { .. } => Some(SAY_BACK),
        Changed::NotBack { .. } => Some(SAY_LOST),
        Changed::Refused(_) => None,
    }
}

/// Where the lamp's lines go: the voice's say service. Injected for tests.
pub trait Mouth {
    /// Words for the voice's model to say: only while there is a network.
    fn say(&mut self, text: &str);
    /// A recording the voice holds, by name: no model, no network. Whether
    /// the voice took it (begun, or waiting to be).
    fn play(&mut self, recording: &str) -> bool;
}

/// The lost line: the recording, or, when the voice does not have it (the
/// clip is not rendered yet, `tools/render-wifi-line.sh`), the same words in
/// plain text, for a voice that can still speak them (#1959's P1).
fn say_lost(mouth: &mut impl Mouth) {
    if !mouth.play(RECORDING_LOST) {
        mouth.say(SAY_LOST);
    }
}

/// After `changed`, the line once; and with neither network back, the lost
/// line again at most once a minute until one is, [`LOST_MOST`] at most.
/// The lost line is always the recording ([`RECORDING_LOST`]), never words
/// for the model. Returns how many lines were asked for.
pub fn say_after(
    host: &mut impl Rejoin,
    mouth: &mut impl Mouth,
    changed: &Changed,
    broker: &str,
) -> u32 {
    let Some(line) = line_for(changed) else {
        return 0;
    };
    if !matches!(changed, Changed::NotBack { .. }) {
        mouth.say(line);
        return 1;
    }
    say_lost(mouth);
    let started = host.now_ms();
    let mut last = started;
    let mut said = 1;
    loop {
        host.sleep(Duration::from_secs(5));
        let now = host.now_ms();
        if now.saturating_sub(started) >= LOST_MOST.as_millis() as u64 {
            return said;
        }
        if host.has_station_ip() && host.broker(broker) {
            return said;
        }
        if now.saturating_sub(last) >= LOST_EVERY.as_millis() as u64 {
            say_lost(mouth);
            last = now;
            said += 1;
        }
    }
}

/// The voice's say service (`docs/speaking-first.md`, "the say service"):
/// one line as `text`, or one recording by name, from the skill name
/// `wifi`, kept a minute. With the say service off, nothing is said.
pub struct SayService {
    pub socket: PathBuf,
}

/// How long a recording's ask waits for the voice's first answer: a
/// recording that is not there is refused at once, and one the lamp cannot
/// play now (speaking, listening, muted) at its next pass.
pub const PLAY_ANSWER_WAIT: Duration = Duration::from_secs(2);

impl SayService {
    pub fn product() -> Self {
        Self {
            socket: std::env::var_os("LAMP_SAY_SOCKET")
                .map_or_else(|| "/run/lampos-say/say.sock".into(), PathBuf::from),
        }
    }
}

impl Mouth for SayService {
    fn say(&mut self, text: &str) {
        self.say_as("wifi", text);
    }

    fn play(&mut self, recording: &str) -> bool {
        match self.play_recording(recording) {
            Some(said) => {
                eprintln!("lamp-setup: wifi: {said}");
                false
            }
            None => true,
        }
    }
}

impl SayService {
    /// Words for the voice's model to say, from the skill name `skill`, kept
    /// a minute: only while there is a network, and with the say service off
    /// nothing is said. The outcome is not waited for.
    pub fn say_as(&self, skill: &str, text: &str) {
        use std::io::Write;
        let ask = json!({"skill": skill, "text": text, "priority": 7, "expires_in_ms": 60_000});
        let Ok(mut stream) = std::os::unix::net::UnixStream::connect(&self.socket) else {
            eprintln!("lamp-setup: {skill}: the voice's say service is not there; nothing said");
            return;
        };
        // A client gone before the outcome changes nothing for the say
        // (`say::tests::a_ticket_dropped_unanswered_is_dropped_and_a_client_gone_is_nothing`),
        // so the outcome is not waited for.
        let _ = stream.write_all(format!("{ask}\n").as_bytes());
    }

    /// Ask the voice to play `recording`, and wait at most
    /// [`PLAY_ANSWER_WAIT`] for its first answer: the journal line when it
    /// was not played (missing, refused, no voice), `None` once begun or
    /// still waiting to be. A refusal after the wait is read by nobody and
    /// not journalled (`docs/setup.md`). Never the line's words.
    pub fn play_recording(&self, recording: &str) -> Option<String> {
        use std::io::{BufRead, BufReader, Write};
        let ask = json!({"skill": "wifi", "recording": recording, "priority": 7,
            "expires_in_ms": 60_000});
        let Ok(mut stream) = std::os::unix::net::UnixStream::connect(&self.socket) else {
            return Some(format!(
                "the voice's say service is not there; the recording {recording} was not played"
            ));
        };
        if stream.write_all(format!("{ask}\n").as_bytes()).is_err()
            || stream.set_read_timeout(Some(PLAY_ANSWER_WAIT)).is_err()
        {
            return Some(format!(
                "the voice's say service did not take the ask; the recording {recording} was not played"
            ));
        }
        let mut first = String::new();
        if BufReader::new(stream).read_line(&mut first).is_err() || first.is_empty() {
            // No answer yet: it waits in the voice, or the voice is gone.
            return None;
        }
        let answer: Value = serde_json::from_str(first.trim_end()).unwrap_or(Value::Null);
        match answer["outcome"].as_str() {
            None if answer["event"] == "begun" => None,
            Some("spoken") => None,
            Some(outcome) => Some(format!(
                "the recording {recording} was not played: {outcome}{}",
                answer["why"]
                    .as_str()
                    .map(|why| format!(", {why}"))
                    .unwrap_or_default()
            )),
            None => Some(format!(
                "the voice's say service answered what it cannot be; the recording {recording} was not played"
            )),
        }
    }
}

/// The lamp as setup's join finds it ([`wifi::ImageHost`]), with the old
/// file kept beside the live one and the broker reached over TCP.
pub struct ImageRejoin {
    pub image: wifi::ImageHost,
    /// Beside the live file: `wpa_supplicant-wlan0.conf.lamp-previous`.
    pub previous: PathBuf,
}

impl ImageRejoin {
    pub fn new() -> Self {
        let image = wifi::ImageHost::new();
        let previous = image.conf.with_extension("conf.lamp-previous");
        Self { image, previous }
    }
}

/// The asker on the helper's socket (standard input and output), as
/// [`confirmed_on`].
pub fn confirmed_on_stdio(ssid: &str, most: Duration) -> bool {
    confirmed_on(
        io::BufReader::new(io::stdin()),
        &mut io::stdout(),
        ssid,
        ONLINE_WAIT,
        most,
    )
}

/// The asker's side of a proven new network (#1959's P1, the reviewer-lead's
/// P2 and P3 and the first builder's P2-1 on #2617). The helper writes
/// `{"joined": {"ssid", "online_within_ms", "confirm_within_ms"}}`;
/// `lamp-cloud` drops its link, makes it again over the new network, says
/// `wifi.joined` to the app and, once the broker acks it, starts its copy
/// of the window and says `{"online": true}` within `online_most`. Only
/// then does the helper's window of `most` start, so `lamp-cloud`'s never
/// ends after it; a confirmation `lamp-cloud` took within its own may take
/// [`CONFIRM_SLACK`] more to cross the socket. Anything else, the socket
/// closed, or nothing in time, is no confirmation.
pub fn confirmed_on<R: io::BufRead + Send + 'static>(
    reader: R,
    out: &mut impl io::Write,
    ssid: &str,
    online_most: Duration,
    most: Duration,
) -> bool {
    use std::io::BufRead;
    let mut say = |line: Value| writeln!(out, "{line}").and_then(|()| out.flush()).is_ok();
    // Two lines at most, each bounded, on a thread of their own: the
    // waits below bound the change, not the reads.
    let (send, heard) = mpsc::channel::<Value>();
    let reading = std::thread::Builder::new()
        .name("lamp-setup-confirm".into())
        .spawn(move || {
            let mut reader = reader;
            for _ in 0..2 {
                let mut line = String::new();
                let read = io::Read::take(&mut reader, LINE_MOST as u64).read_line(&mut line);
                if read.is_err() || line.is_empty() {
                    return;
                }
                let value = serde_json::from_str(line.trim()).unwrap_or(Value::Null);
                if send.send(value).is_err() {
                    return;
                }
            }
        });
    if reading.is_err() {
        return false;
    }
    let joined = json!({"joined": {
        "ssid": ssid,
        "online_within_ms": online_most.as_millis() as u64,
        "confirm_within_ms": most.as_millis() as u64,
    }});
    if !say(joined) {
        return false;
    }
    if heard.recv_timeout(online_most).ok() != Some(json!({"online": true})) {
        return false;
    }
    let deadline = Instant::now() + most + CONFIRM_SLACK;
    heard
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .ok()
        == Some(json!({"confirm": true}))
}

impl Default for ImageRejoin {
    fn default() -> Self {
        Self::new()
    }
}

impl Host for ImageRejoin {
    fn read_conf(&mut self) -> io::Result<Option<Vec<u8>>> {
        self.image.read_conf()
    }
    fn write_conf(&mut self, bytes: Option<&[u8]>) -> io::Result<()> {
        self.image.write_conf(bytes)
    }
    fn station(&mut self) -> io::Result<()> {
        self.image.station()
    }
    fn hotspot(&mut self) -> io::Result<()> {
        self.image.hotspot()
    }
    fn internet(&mut self) -> bool {
        self.image.internet()
    }
    fn current_ssid(&mut self) -> Option<String> {
        self.image.current_ssid()
    }
    fn has_station_ip(&mut self) -> bool {
        self.image.has_station_ip()
    }
    fn wpa_state(&mut self) -> String {
        self.image.wpa_state()
    }
    fn wpa_events(&mut self) -> Vec<String> {
        self.image.wpa_events()
    }
    fn now_ms(&mut self) -> u64 {
        self.image.now_ms()
    }
    fn sleep(&mut self, duration: Duration) {
        self.image.sleep(duration);
    }
}

impl Rejoin for ImageRejoin {
    fn broker(&mut self, address: &str) -> bool {
        let until = Instant::now() + BROKER_TIMEOUT;
        // The system's lookup takes no time limit: it runs on a thread of its
        // own and is waited for only until the deadline.
        let (send, found) = mpsc::channel::<Vec<SocketAddr>>();
        let asked = address.to_owned();
        let looked = std::thread::Builder::new()
            .name("lamp-setup-broker".into())
            .spawn(move || {
                let _ = send.send(
                    asked
                        .to_socket_addrs()
                        .map(Iterator::collect)
                        .unwrap_or_default(),
                );
            });
        if looked.is_err() {
            return false;
        }
        let Ok(addresses) = found.recv_timeout(until.saturating_duration_since(Instant::now()))
        else {
            return false;
        };
        addresses.iter().any(|address| {
            let left = until.saturating_duration_since(Instant::now());
            !left.is_zero() && TcpStream::connect_timeout(address, left).is_ok()
        })
    }

    fn keep_previous(&mut self, bytes: Option<&[u8]>) -> io::Result<()> {
        match bytes {
            // Written, synced and its folder synced.
            Some(bytes) => crate::serve::write_private(&self.previous, bytes),
            None => {
                match std::fs::remove_file(&self.previous) {
                    Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
                    _ => {}
                }
                // The removal on disk before the change is said kept (P3 3).
                match self.previous.parent() {
                    Some(folder) => std::fs::File::open(folder)?.sync_all(),
                    None => Ok(()),
                }
            }
        }
    }

    fn read_previous(&mut self) -> io::Result<Option<Vec<u8>>> {
        match std::fs::read(&self.previous) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn reload(&mut self) -> io::Result<()> {
        self.image.reconfigure()
    }

    fn app_confirms(&mut self, ssid: &str, most: Duration) -> bool {
        confirmed_on_stdio(ssid, most)
    }

    fn hold(&mut self, conf: &[u8]) -> io::Result<()> {
        let fields = network_lines(conf)
            .ok_or_else(|| io::Error::other("the old Wi-Fi's file names no network"))?;
        self.image.hold(&fields)
    }
}
