//! Joining the home's Wi-Fi during setup, as V1 does it (`docs/setup.md`):
//! the same failure reasons, the same 60 s bound, the same early stop after
//! two rejected keys, and the hotspot back on failure. V1's own image scripts
//! switch the radio (`device-sta-mode`, `device-ap-mode`); the one change is
//! that the password is written to the private configuration file here, never
//! put on a command line, and a failed join puts the previous file back.

use serde::Serialize;
use std::{
    io,
    net::{SocketAddr, TcpStream},
    sync::{Arc, Mutex, OnceLock, PoisonError, mpsc},
    time::{Duration, Instant},
};

/// V1's `setupInternetWait`: the join, the address and the Internet.
pub const JOIN_WAIT: Duration = Duration::from_secs(60);
/// V1's `wifiFailFastThreshold`: a key rejected this often will not pass.
pub const WRONG_KEY_STOP: u32 = 2;

/// Why a join failed. The names are V1's `setup_failure_reason` values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    WrongPassword,
    AssociationRejected,
    SsidNotFound,
    NoDhcp,
    NoInternet,
    SsidTooLong,
    WifiConnectError,
    Unknown,
}

impl Reason {
    /// V1's text for the setup screen.
    pub fn detail(self) -> &'static str {
        match self {
            Self::WrongPassword => "wrong WiFi password: the router rejected the key",
            Self::AssociationRejected => "the router refused the connection",
            Self::SsidNotFound => {
                "WiFi network not found: out of range or on a channel the device cannot use"
            }
            Self::NoDhcp => "connected to WiFi but the router gave no IP address",
            Self::NoInternet => {
                "connected to WiFi but there is no internet (captive portal or firewall?)"
            }
            Self::SsidTooLong => "ssid too long: max 32 bytes (802.11 limit)",
            Self::WifiConnectError => "the WiFi could not be started",
            Self::Unknown => "no internet or SSID did not match within 60s",
        }
    }
}

/// What the join last saw (V1's `wifiObservation`).
#[derive(Clone, Debug, Default)]
pub struct Observation {
    /// wpa_supplicant's log lines since this attempt began.
    pub events: Vec<String>,
    /// `wpa_cli status`'s `wpa_state`.
    pub wpa_state: String,
    /// A station address, not the hotspot's.
    pub has_ip: bool,
    pub ssid_matched: bool,
}

/// The highest `auth_failures` wpa_supplicant reported with `WRONG_KEY`, or
/// the number of failed 4-way handshakes, whichever is more.
pub fn wrong_key_failures(events: &[String]) -> u32 {
    let mut reported = 0;
    let mut handshakes = 0;
    for event in events {
        if event.contains("reason=WRONG_KEY")
            && let Some(at) = event.find("auth_failures=")
        {
            let digits: String = event[at + "auth_failures=".len()..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            if let Ok(count) = digits.parse::<u32>() {
                reported = reported.max(count);
            }
        }
        if event.contains("4-Way Handshake failed") {
            handshakes += 1;
        }
    }
    reported.max(handshakes)
}

/// V1's `classifyWiFiFailure`: key evidence wins over state, since after a
/// wrong key wpa_supplicant rescans and its state reads like "not found".
pub fn classify(observation: &Observation) -> Reason {
    let joined = observation.events.join("\n");
    if wrong_key_failures(&observation.events) > 0 {
        Reason::WrongPassword
    } else if joined.contains("CTRL-EVENT-ASSOC-REJECT")
        || joined.contains("reason=AUTH_FAILED")
        || joined.contains("reason=CONN_FAILED")
    {
        Reason::AssociationRejected
    } else if observation.has_ip && observation.ssid_matched {
        Reason::NoInternet
    } else if observation.wpa_state == "COMPLETED" && !observation.has_ip {
        Reason::NoDhcp
    } else if joined.contains("CTRL-EVENT-NETWORK-NOT-FOUND")
        || (!observation.events.is_empty() && !joined.contains("Trying to associate"))
    {
        Reason::SsidNotFound
    } else {
        Reason::Unknown
    }
}

/// Why a network name, password or country is refused before anything is
/// written. A line break in either would add lines to wpa_supplicant's file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    SsidEmpty,
    SsidTooLong,
    SsidControl,
    PasswordLength,
    PasswordCharacters,
    Country,
}

/// wpa_supplicant's file for one network, as V1's `connect-wifi` writes it,
/// with one correction: wpa_supplicant takes everything between the first
/// and last quote as it is, so V1's `\"` and `\\` escapes made a name or a
/// password holding `"` or `\` never match. The name is written as hex, which
/// holds any bytes, and the password raw between quotes, which is safe since
/// line breaks and other control characters are refused. A password holding
/// `"` is written as its 64-hex PSK ([`psk`]) instead: wpa_supplicant 2.10
/// ends a line at a `#` after a closing quote, so `pa"ss#word` would be cut
/// (the first builder's P3 7 on #2617).
pub fn wpa_conf(country: &str, ssid: &str, password: &str) -> Result<String, Refused> {
    let ssid = ssid.trim();
    if ssid.is_empty() {
        return Err(Refused::SsidEmpty);
    }
    if ssid.len() > 32 {
        return Err(Refused::SsidTooLong);
    }
    if ssid.chars().any(char::is_control) {
        return Err(Refused::SsidControl);
    }
    if !password.is_empty() && !(8..=63).contains(&password.len()) {
        return Err(Refused::PasswordLength);
    }
    if password.chars().any(|c| !(' '..='~').contains(&c)) {
        return Err(Refused::PasswordCharacters);
    }
    if country.len() != 2 || !country.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err(Refused::Country);
    }
    let hex: String = ssid.bytes().map(|byte| format!("{byte:02x}")).collect();
    let key = if password.is_empty() {
        "\tkey_mgmt=NONE\n".to_owned()
    } else if password.contains('"') {
        format!("\tpsk={}\n", psk(ssid, password))
    } else {
        format!("\tpsk=\"{password}\"\n")
    };
    Ok(format!(
        "ctrl_interface=DIR=/run/wpa_supplicant\nupdate_config=1\ncountry={country}\n\
         fast_reauth=1\nap_scan=1\nnetwork={{\n\tssid={hex}\n{key}\tscan_ssid=1\n}}\n"
    ))
}

/// WPA's PSK for `password` on the network `ssid`, as `wpa_passphrase`
/// writes it: PBKDF2-HMAC-SHA1, 4096 rounds, the name as the salt, 32 bytes
/// in hex.
pub fn psk(ssid: &str, password: &str) -> String {
    let mut key = [0u8; 32];
    ring::pbkdf2::derive(
        ring::pbkdf2::PBKDF2_HMAC_SHA1,
        std::num::NonZeroU32::new(4_096).unwrap_or(std::num::NonZeroU32::MIN),
        ssid.as_bytes(),
        password.as_bytes(),
        &mut key,
    );
    key.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// `wpa_cli` prints a network name with C escapes (`\xNN` for any byte
/// outside printable ASCII, and `\\`, `\"`, `\e`, `\n`, `\r`, `\t`):
/// "Nhà Lan" comes back as `Nh\xc3\xa0 Lan`. Read it back to the name.
pub fn decode_wpa(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'\\' && at + 1 < bytes.len() {
            let next = bytes[at + 1];
            let simple = match next {
                b'\\' => Some(b'\\'),
                b'"' => Some(b'"'),
                b'e' => Some(0x1b),
                b'n' => Some(b'\n'),
                b'r' => Some(b'\r'),
                b't' => Some(b'\t'),
                _ => None,
            };
            if let Some(byte) = simple {
                out.push(byte);
                at += 2;
                continue;
            }
            if next == b'x'
                && at + 3 < bytes.len()
                && let Ok(byte) = u8::from_str_radix(&text[at + 2..at + 4], 16)
            {
                out.push(byte);
                at += 4;
                continue;
            }
        }
        out.push(bytes[at]);
        at += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// What a join needs from the lamp. Injected for tests.
pub trait Host {
    /// The current Wi-Fi file's bytes, if there is one.
    fn read_conf(&mut self) -> std::io::Result<Option<Vec<u8>>>;
    /// Replace the Wi-Fi file durably, private; `None` removes it.
    fn write_conf(&mut self, bytes: Option<&[u8]>) -> std::io::Result<()>;
    /// V1's `device-sta-mode` (the hotspot goes) and `device-ap-mode`.
    fn station(&mut self) -> std::io::Result<()>;
    fn hotspot(&mut self) -> std::io::Result<()>;
    /// V1's `CheckInternet`: ping, else a TLS handshake ([`Reach`]).
    fn internet(&mut self) -> bool;
    /// The network joined now, its station address, wpa_supplicant's state.
    fn current_ssid(&mut self) -> Option<String>;
    fn has_station_ip(&mut self) -> bool;
    fn wpa_state(&mut self) -> String;
    /// wpa_supplicant's log since this attempt began.
    fn wpa_events(&mut self) -> Vec<String>;
    /// Monotonic milliseconds, and a wait.
    fn now_ms(&mut self) -> u64;
    fn sleep(&mut self, duration: Duration);
}

/// The outcome of one join.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum Joined {
    Connected,
    Failed { reason: Reason },
}

/// Join `ssid` as V1 does, with the file written here. On failure the
/// previous file is put back, nothing of this attempt is kept, and the
/// hotspot comes back.
pub fn join(host: &mut impl Host, country: &str, ssid: &str, password: &str) -> Joined {
    let conf = match wpa_conf(country, ssid, password) {
        Ok(conf) => conf,
        Err(Refused::SsidTooLong) => {
            return Joined::Failed {
                reason: Reason::SsidTooLong,
            };
        }
        Err(_) => {
            return Joined::Failed {
                reason: Reason::WifiConnectError,
            };
        }
    };
    let ssid = ssid.trim();
    let previous = match host.read_conf() {
        Ok(previous) => previous,
        Err(_) => {
            return Joined::Failed {
                reason: Reason::WifiConnectError,
            };
        }
    };
    if host.write_conf(Some(conf.as_bytes())).is_err() || host.station().is_err() {
        return fail(host, previous.as_deref(), Reason::WifiConnectError);
    }
    let started = host.now_ms();
    let deadline = started + JOIN_WAIT.as_millis() as u64;
    let mut attempt = 0u32;
    while host.now_ms() < deadline {
        if host.internet() && host.current_ssid().as_deref() == Some(ssid) {
            return Joined::Connected;
        }
        if attempt % 3 == 2 && wrong_key_failures(&host.wpa_events()) >= WRONG_KEY_STOP {
            break;
        }
        attempt += 1;
        host.sleep(Duration::from_secs(1));
    }
    let observation = Observation {
        events: host.wpa_events(),
        wpa_state: host.wpa_state(),
        has_ip: host.has_station_ip(),
        ssid_matched: host.current_ssid().as_deref() == Some(ssid),
    };
    fail(host, previous.as_deref(), classify(&observation))
}

/// Nothing of a failed attempt is kept: the previous file goes back, and the
/// hotspot with it.
fn fail(host: &mut impl Host, previous: Option<&[u8]>, reason: Reason) -> Joined {
    let _ = host.write_conf(previous);
    let _ = host.hotspot();
    Joined::Failed { reason }
}

/// V1's `cloudProbeTimeout`: the name, the connection and the TLS handshake,
/// all of it, so a dead network still fails fast inside the 60 s join.
pub const REACH_TIMEOUT: Duration = Duration::from_secs(3);

/// Looks up a `host:port` name. Injected for tests.
pub type Resolve = fn(&str) -> io::Result<Vec<SocketAddr>>;

fn system_lookup(address: &str) -> io::Result<Vec<SocketAddr>> {
    std::net::ToSocketAddrs::to_socket_addrs(address).map(Iterator::collect)
}

type Lookup = mpsc::Receiver<io::Result<Vec<SocketAddr>>>;

/// Where the one name lookup that has not come back yet is kept, for the
/// address it is for. The system's lookup takes no time limit and cannot be
/// stopped, so one left behind is waited on by the next check, never started
/// again. Every check of the running service shares one ([`Self::process`]),
/// so a lookup left by one setup attempt is the next attempt's too (#845).
#[derive(Clone, Default)]
pub struct LookupSlot(Arc<Mutex<Option<(String, Lookup)>>>);

static PROCESS_SLOT: OnceLock<LookupSlot> = OnceLock::new();

impl LookupSlot {
    /// The process's own: at most one lookup outstanding in all of it.
    pub fn process() -> Self {
        PROCESS_SLOT.get_or_init(Self::default).clone()
    }
}

/// V1's `tlsReach`, the Internet check when ping is blocked: a TLS handshake
/// with the voice's service, completed and closed. A TCP connection alone is
/// not enough: anything that accepts and then stays silent, or answers in
/// plain text, would pass for the Internet.
pub struct Reach {
    /// `host:port`.
    pub address: String,
    pub timeout: Duration,
    resolve: Resolve,
    /// The lookup that has not come back yet, shared ([`LookupSlot`]). It
    /// runs on a thread of its own and is waited for only until the deadline.
    pending: LookupSlot,
    tls: Option<Arc<rustls::ClientConfig>>,
}

impl Reach {
    /// The system's lookup, sharing the process's one outstanding lookup.
    pub fn new(address: impl Into<String>, timeout: Duration) -> Self {
        Self::with_lookup_in(address, timeout, system_lookup, LookupSlot::process())
    }

    /// With `resolve` in place of the system's, and a slot of its own.
    pub fn with_lookup(address: impl Into<String>, timeout: Duration, resolve: Resolve) -> Self {
        Self::with_lookup_in(address, timeout, resolve, LookupSlot::default())
    }

    /// With `resolve`, sharing `slot`'s outstanding lookup.
    pub fn with_lookup_in(
        address: impl Into<String>,
        timeout: Duration,
        resolve: Resolve,
        slot: LookupSlot,
    ) -> Self {
        Self {
            address: address.into(),
            timeout,
            resolve,
            pending: slot,
            tls: None,
        }
    }

    /// True once a TLS handshake with the address is done, within `timeout`.
    pub fn check(&mut self) -> bool {
        let until = Instant::now() + self.timeout;
        let Some(tls) = self.tls() else {
            return false;
        };
        let Some((host, _)) = self.address.rsplit_once(':') else {
            return false;
        };
        let host = host.trim_start_matches('[').trim_end_matches(']');
        let Ok(name) = rustls::pki_types::ServerName::try_from(host.to_owned()) else {
            return false;
        };
        let Ok(addresses) = self.addresses(until) else {
            return false;
        };
        for address in addresses {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            let Ok(socket) = TcpStream::connect_timeout(&address, left) else {
                continue;
            };
            let mut wire = Timed { socket, until };
            let Ok(mut session) = rustls::ClientConnection::new(Arc::clone(&tls), name.clone())
            else {
                return false;
            };
            while session.is_handshaking() {
                if Instant::now() >= until || session.complete_io(&mut wire).is_err() {
                    return false;
                }
            }
            // Closed politely, as V1's `conn.Close()`; nothing else is sent.
            session.send_close_notify();
            let _ = session.write_tls(&mut wire);
            return true;
        }
        false
    }

    fn addresses(&mut self, until: Instant) -> io::Result<Vec<SocketAddr>> {
        // Held while waiting, at most until the deadline: no second check can
        // start a lookup meanwhile.
        let mut slot = self
            .pending
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let pending = match slot.take() {
            Some((address, pending)) if address == self.address => pending,
            Some(other) => {
                *slot = Some(other);
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "another name's lookup is still out",
                ));
            }
            None => {
                let (send, receive) = mpsc::channel();
                let (address, resolve) = (self.address.clone(), self.resolve);
                std::thread::Builder::new()
                    .name("lamp-setup-lookup".into())
                    .spawn(move || {
                        let _ = send.send(resolve(&address));
                    })?;
                receive
            }
        };
        match pending.recv_timeout(until.saturating_duration_since(Instant::now())) {
            Ok(found) => found,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                *slot = Some((self.address.clone(), pending));
                Err(io::ErrorKind::TimedOut.into())
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(io::Error::other("the lookup ended without an answer"))
            }
        }
    }

    fn tls(&mut self) -> Option<Arc<rustls::ClientConfig>> {
        if self.tls.is_none() {
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let algorithms = provider.signature_verification_algorithms;
            let config = rustls::ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .ok()?
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(AnyCertificate(algorithms)))
                .with_no_client_auth();
            self.tls = Some(Arc::new(config));
        }
        self.tls.clone()
    }
}

/// V1's `InsecureSkipVerify`, on purpose: this proves only that the service
/// answers, and sends it nothing. A lamp has no clock that runs with the power
/// off, so right after the join, before the time is set, every certificate
/// can look not yet valid. As in V1, the handshake's own signature is still
/// checked: the other end must hold the key of the certificate it sent.
#[derive(Debug)]
struct AnyCertificate(rustls::crypto::WebPkiSupportedAlgorithms);

impl rustls::client::danger::ServerCertVerifier for AnyCertificate {
    fn verify_server_cert(
        &self,
        _: &rustls::pki_types::CertificateDer<'_>,
        _: &[rustls::pki_types::CertificateDer<'_>],
        _: &rustls::pki_types::ServerName<'_>,
        _: &[u8],
        _: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        certificate: &rustls::pki_types::CertificateDer<'_>,
        signed: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, certificate, signed, &self.0)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        certificate: &rustls::pki_types::CertificateDer<'_>,
        signed: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, certificate, signed, &self.0)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.supported_schemes()
    }
}

/// A socket whose every read and write ends by one deadline, so a server that
/// sends a byte now and then cannot stretch the handshake past it.
struct Timed {
    socket: TcpStream,
    until: Instant,
}

impl Timed {
    fn left(&self) -> io::Result<Duration> {
        let left = self.until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            Err(io::ErrorKind::TimedOut.into())
        } else {
            Ok(left)
        }
    }
}

impl io::Read for Timed {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.socket.set_read_timeout(Some(self.left()?))?;
        io::Read::read(&mut self.socket, buffer)
    }
}

impl io::Write for Timed {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.socket.set_write_timeout(Some(self.left()?))?;
        io::Write::write(&mut self.socket, bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        io::Write::flush(&mut self.socket)
    }
}

/// The longest a radio script (`device-sta-mode`, `device-ap-mode`) may
/// take: its own sleeps are 7 s, and a `systemctl restart` that hangs must
/// not hold the join or the way back (#1959's P1).
pub const RADIO_TIMEOUT: Duration = Duration::from_secs(45);
/// The longest V1's `device-ap-mode` may take, setup's own hotspot (the
/// first builder's P3 6 on #2617): it restarts hostapd up to twice, each
/// up to systemd's 90 s start limit, then dnsmasq and nginx, with 6 s of
/// its own sleeps. Killed earlier, a slow hostapd would leave a lamp in
/// setup with no hotspot at all.
pub const HOTSPOT_TIMEOUT: Duration = Duration::from_secs(4 * 60);
/// The longest `wpa_cli` or `journalctl` may take.
pub const QUERY_TIMEOUT: Duration = Duration::from_secs(10);

/// How long a command's output is read after it ended, for what it left.
const OUTPUT_GRACE: Duration = Duration::from_secs(1);

/// Runs `command` with its output read, killed once `most` has passed: its
/// exit status and standard output, or a timeout error.
pub fn within(
    command: &mut std::process::Command,
    most: Duration,
) -> std::io::Result<(std::process::ExitStatus, Vec<u8>)> {
    use std::{io::Read, os::unix::process::CommandExt};
    // Its own process group, so a timeout kills the script's children too
    // (a `systemctl restart` that hangs).
    let mut child = command
        .process_group(0)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .spawn()?;
    let mut stdout = child.stdout.take();
    let (send, read) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(stdout) = stdout.as_mut() {
            let _ = stdout.read_to_end(&mut bytes);
        }
        let _ = send.send(bytes);
    });
    let group = child.id();
    let kill_group = || {
        let _ = std::process::Command::new("kill")
            .args(["-KILL", "--", &format!("-{group}")])
            .stderr(std::process::Stdio::null())
            .status();
    };
    let deadline = Instant::now() + most;
    loop {
        if let Some(status) = child.try_wait()? {
            // A child of its own may keep the output open after it ends
            // (the first builder's P3 6 on #2617): waited on only to the
            // bound and a second's grace, then its group is killed.
            let grace = deadline
                .saturating_duration_since(Instant::now())
                .min(OUTPUT_GRACE);
            let bytes = read.recv_timeout(grace).unwrap_or_else(|_| {
                kill_group();
                read.recv_timeout(OUTPUT_GRACE).unwrap_or_default()
            });
            return Ok((status, bytes));
        }
        if Instant::now() >= deadline {
            kill_group();
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!(
                    "{:?} took longer than {} s",
                    command.get_program(),
                    most.as_secs()
                ),
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The lamp as V1's image leaves it: wpa_supplicant on `wlan0`, its file in
/// `/etc/wpa_supplicant`, and the image's two radio scripts.
pub struct ImageHost {
    pub conf: std::path::PathBuf,
    pub station_script: std::path::PathBuf,
    pub hotspot_script: std::path::PathBuf,
    pub interface: String,
    /// The voice's service, reached over TLS when ping is blocked, as V1 does.
    pub reach: Reach,
    started: std::time::Instant,
    since_unix: u64,
}

impl ImageHost {
    pub fn new() -> Self {
        Self {
            conf: "/etc/wpa_supplicant/wpa_supplicant-wlan0.conf".into(),
            station_script: "/usr/local/bin/device-sta-mode".into(),
            hotspot_script: "/usr/local/bin/device-ap-mode".into(),
            interface: "wlan0".into(),
            reach: Reach::new("generativelanguage.googleapis.com:443", REACH_TIMEOUT),
            started: std::time::Instant::now(),
            since_unix: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_secs()),
        }
    }

    fn run(&self, program: &std::path::Path, most: Duration) -> std::io::Result<()> {
        let mut command = std::process::Command::new(program);
        let (status, _) = within(&mut command, most)?;
        if status.success() {
            Ok(())
        } else {
            Err(std::io::Error::other(format!(
                "{} ended with {status}",
                program.display()
            )))
        }
    }

    /// `wpa_cli status`, as its key=value lines.
    fn status(&self) -> Vec<(String, String)> {
        let mut command = std::process::Command::new("wpa_cli");
        command.args(["-i", &self.interface, "status"]);
        within(&mut command, QUERY_TIMEOUT)
            .map(|(_, stdout)| {
                String::from_utf8_lossy(&stdout)
                    .lines()
                    .filter_map(|line| line.split_once('='))
                    .map(|(key, value)| (key.trim().to_owned(), value.trim().to_owned()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// `wpa_cli reconfigure`: wpa_supplicant reads its file again, so the
    /// network it runs is the file's even when the radio's script fails
    /// (#1959's P1).
    pub fn reconfigure(&self) -> std::io::Result<()> {
        let mut command = std::process::Command::new("wpa_cli");
        command.args(["-i", &self.interface, "reconfigure"]);
        let (status, stdout) = within(&mut command, QUERY_TIMEOUT)?;
        if status.success() && String::from_utf8_lossy(&stdout).trim_end().ends_with("OK") {
            Ok(())
        } else {
            Err(std::io::Error::other("wpa_cli reconfigure was not taken"))
        }
    }

    /// wpa_supplicant runs one network from its memory, given as its file's
    /// `key=value` lines, its file never saved over (#2617's P3 2): the
    /// networks it has are removed, this one added, set and selected. Over
    /// its control socket itself, not `wpa_cli`, so the password is never
    /// in a command line; each request bounded by [`QUERY_TIMEOUT`].
    pub fn hold(&self, fields: &[(String, String)]) -> std::io::Result<()> {
        let control = std::path::Path::new("/run/wpa_supplicant").join(&self.interface);
        let mine = std::env::temp_dir().join(format!("lamp-setup-wpa-{}", std::process::id()));
        let _ = std::fs::remove_file(&mine);
        let socket = std::os::unix::net::UnixDatagram::bind(&mine)?;
        let asked = (|| {
            socket.connect(&control)?;
            socket.set_read_timeout(Some(QUERY_TIMEOUT))?;
            socket.set_write_timeout(Some(QUERY_TIMEOUT))?;
            let ask = |request: &str| -> std::io::Result<String> {
                socket.send(request.as_bytes())?;
                let mut reply = [0u8; 4_096];
                let read = socket.recv(&mut reply)?;
                Ok(String::from_utf8_lossy(&reply[..read]).trim().to_owned())
            };
            let ok = |request: &str, what: &str| match ask(request)?.as_str() {
                "OK" => Ok(()),
                _ => Err(std::io::Error::other(format!(
                    "wpa_supplicant refused {what}"
                ))),
            };
            ok("REMOVE_NETWORK all", "removing its networks")?;
            let id = ask("ADD_NETWORK")?;
            if id.parse::<u32>().is_err() {
                return Err(std::io::Error::other("wpa_supplicant refused a network"));
            }
            for (key, value) in fields {
                // The field's name only: its value may be the password.
                ok(&format!("SET_NETWORK {id} {key} {value}"), key)?;
            }
            ok(&format!("SELECT_NETWORK {id}"), "selecting the network")
        })();
        let _ = std::fs::remove_file(&mine);
        asked
    }

    fn field(&self, key: &str) -> Option<String> {
        self.status()
            .into_iter()
            .find_map(|(name, value)| (name == key).then_some(value))
    }
}

impl Default for ImageHost {
    fn default() -> Self {
        Self::new()
    }
}

impl Host for ImageHost {
    fn read_conf(&mut self) -> std::io::Result<Option<Vec<u8>>> {
        match std::fs::read(&self.conf) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn write_conf(&mut self, bytes: Option<&[u8]>) -> std::io::Result<()> {
        use std::{io::Write, os::unix::fs::OpenOptionsExt};
        let Some(bytes) = bytes else {
            match std::fs::remove_file(&self.conf) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(error) => return Err(error),
                Ok(()) => {}
            }
            // The removal synced too, as a write's rename is: the root is
            // mounted `commit=600`, so an unplug up to 10 minutes later could
            // bring the failed join's file back (lamp-06c1, 29 Oct).
            if let Some(parent) = self.conf.parent() {
                std::fs::File::open(parent)?.sync_all()?;
            }
            return Ok(());
        };
        let temporary = self.conf.with_extension("conf.lamp-setup");
        let _ = std::fs::remove_file(&temporary);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&temporary, &self.conf)?;
        if let Some(parent) = self.conf.parent() {
            std::fs::File::open(parent)?.sync_all()?;
        }
        Ok(())
    }

    fn station(&mut self) -> std::io::Result<()> {
        // wpa_supplicant restarts here, so its log from now on is this join's.
        self.since_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        self.run(&self.station_script.clone(), RADIO_TIMEOUT)
    }

    fn hotspot(&mut self) -> std::io::Result<()> {
        self.run(&self.hotspot_script.clone(), HOTSPOT_TIMEOUT)
    }

    fn internet(&mut self) -> bool {
        let ping = std::process::Command::new("ping")
            .args(["-c", "1", "-W", "2", "8.8.8.8"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        ping || self.reach.check()
    }

    fn current_ssid(&mut self) -> Option<String> {
        self.field("ssid").map(|ssid| decode_wpa(&ssid))
    }

    fn has_station_ip(&mut self) -> bool {
        self.field("ip_address")
            .is_some_and(|address| !address.is_empty() && address != "192.168.100.1")
    }

    fn wpa_state(&mut self) -> String {
        self.field("wpa_state").unwrap_or_default()
    }

    fn wpa_events(&mut self) -> Vec<String> {
        let mut command = std::process::Command::new("journalctl");
        command
            .args(["-t", "wpa_supplicant", "--no-pager", "-o", "cat", "--since"])
            .arg(format!("@{}", self.since_unix));
        within(&mut command, QUERY_TIMEOUT)
            .map(|(_, stdout)| {
                String::from_utf8_lossy(&stdout)
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default()
    }

    fn now_ms(&mut self) -> u64 {
        self.started.elapsed().as_millis() as u64
    }

    fn sleep(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
}
