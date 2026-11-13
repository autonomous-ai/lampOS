//! Monitor mode (`LAMP_PEEK=monitor`, #5883's first piece): the same page
//! for the developer who owns the lamp, from a computer or phone on the
//! same Wi-Fi, without a static token. The way in is a one-time code that
//! lamp-cloud asks for on the admin socket (root and the lamp's user only),
//! good once and for five minutes, given in the link's fragment and posted
//! once to `/in`, which answers with a session cookie kept in memory only:
//! a restart ends every session. Only a peer on the local network is
//! answered, and only as the lamp's own name or address in `Host`.
use crate::serve::same;
use axum::serve::IncomingStream;
use std::{
    fs,
    io::{self, Read, Write},
    net::{IpAddr, SocketAddr},
    os::unix::{fs::PermissionsExt, net::UnixListener},
    path::Path,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

/// How long a code is good for, once.
pub const CODE_LASTS: Duration = Duration::from_secs(5 * 60);
/// How long a session lasts, unless peek stops first.
pub const SESSION_LASTS: Duration = Duration::from_secs(12 * 60 * 60);
/// Browsers open at once; the next is refused.
pub const MOST_SESSIONS: usize = 4;
/// Codes waiting at once: the oldest goes for a new one.
const MOST_CODES: usize = 8;
/// The cookie that carries a session.
pub const SESSION_COOKIE: &str = "peek_session";
/// Where lamp-cloud asks for a code.
pub const ADMIN_SOCKET: &str = "LAMP_PEEK_ADMIN_SOCKET";
pub const DEFAULT_ADMIN_SOCKET: &str = "/run/lampos-peek/admin.sock";
/// Where monitor mode answers unless `LAMP_PEEK_LISTEN` says otherwise:
/// `http://lamp-xxxx.local/`.
pub const DEFAULT_LISTEN: &str = "0.0.0.0:80";

/// The one-time codes and the sessions they opened, in memory only.
#[derive(Default)]
pub struct Gate {
    codes: Vec<(String, Instant)>,
    sessions: Vec<(String, Instant)>,
}

/// What a code posted to `/in` gets.
#[derive(Debug, PartialEq, Eq)]
pub enum Redeemed {
    /// A new session, for the cookie.
    Session(String),
    /// Used, out of date, or never made.
    Refused,
    /// [`MOST_SESSIONS`] are open; the code is kept for when one ends.
    Full,
}

impl Gate {
    /// A new code, good once until [`CODE_LASTS`] after `now`.
    pub fn issue(&mut self, now: Instant) -> io::Result<String> {
        self.prune(now);
        if self.codes.len() >= MOST_CODES {
            self.codes.remove(0);
        }
        let code = random_hex()?;
        self.codes.push((code.clone(), now));
        Ok(code)
    }

    /// `code` spent for a session, if it is good.
    pub fn redeem(&mut self, code: &str, now: Instant) -> io::Result<Redeemed> {
        self.prune(now);
        let Some(at) = self.codes.iter().position(|(kept, _)| same(kept, code)) else {
            return Ok(Redeemed::Refused);
        };
        if self.sessions.len() >= MOST_SESSIONS {
            return Ok(Redeemed::Full);
        }
        self.codes.remove(at);
        let session = random_hex()?;
        self.sessions.push((session.clone(), now));
        Ok(Redeemed::Session(session))
    }

    /// Whether `session` is one this run opened and still lasts.
    pub fn admits(&mut self, session: &str, now: Instant) -> bool {
        self.lasts_until(session, now).is_some()
    }

    /// When `session` ends, if it is one this run opened and still lasts.
    pub fn lasts_until(&mut self, session: &str, now: Instant) -> Option<Instant> {
        self.prune(now);
        self.sessions
            .iter()
            .find(|(kept, _)| same(kept, session))
            .map(|(_, at)| *at + SESSION_LASTS)
    }

    fn prune(&mut self, now: Instant) {
        self.codes
            .retain(|(_, at)| now.saturating_duration_since(*at) < CODE_LASTS);
        self.sessions
            .retain(|(_, at)| now.saturating_duration_since(*at) < SESSION_LASTS);
    }
}

/// 32 random bytes as 64 hex digits.
fn random_hex() -> io::Result<String> {
    let mut bytes = [0u8; 32];
    fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// What monitor mode holds: the gate and the lamp's own names.
pub struct Monitor {
    pub gate: Mutex<Gate>,
    /// The names `Host` may give, lower case: the lamp's host name and its
    /// `.local` name ([`own_names`]).
    pub names: Vec<String>,
}

impl Monitor {
    pub fn new(names: Vec<String>) -> Self {
        Self {
            gate: Mutex::default(),
            names,
        }
    }

    pub fn gate(&self) -> std::sync::MutexGuard<'_, Gate> {
        self.gate.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The lamp's host name and its `.local` name, as avahi gives it.
pub fn own_names() -> Vec<String> {
    let name = ["/proc/sys/kernel/hostname", "/etc/hostname"]
        .iter()
        .find_map(|path| fs::read_to_string(path).ok())
        .map(|name| name.trim().to_ascii_lowercase())
        .unwrap_or_default();
    if name.is_empty() {
        return Vec::new();
    }
    vec![format!("{name}.local"), name]
}

/// A connection's two ends: who asks, and the lamp's address it asked on.
#[derive(Clone, Copy, Debug)]
pub struct Conn {
    pub peer: SocketAddr,
    pub local: SocketAddr,
}

impl axum::extract::connect_info::Connected<IncomingStream<'_, tokio::net::TcpListener>> for Conn {
    fn connect_info(stream: IncomingStream<'_, tokio::net::TcpListener>) -> Self {
        Self {
            peer: *stream.remote_addr(),
            // Unknown, no address matches it: only the names do.
            local: stream
                .io()
                .local_addr()
                .unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], 0))),
        }
    }
}

/// Whether `peer` is on the local network: 10/8, 172.16/12, 192.168/16,
/// 169.254/16, fe80::/10, fc00::/7 or this machine.
pub fn lan_peer(peer: IpAddr) -> bool {
    match peer.to_canonical() {
        IpAddr::V4(v4) => v4.is_private() || v4.is_link_local() || v4.is_loopback(),
        IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            v6.is_loopback() || first & 0xffc0 == 0xfe80 || first & 0xfe00 == 0xfc00
        }
    }
}

/// Whether `host` (a `Host` header) is one of `names` or the address the
/// request came to, with or without a port: no other name, so that a page
/// elsewhere cannot reach the lamp under a name of its own (DNS rebinding).
pub fn host_allowed(host: &str, names: &[String], local: IpAddr) -> bool {
    let host = host.to_ascii_lowercase();
    let address = match host.strip_prefix('[') {
        Some(rest) => match rest.split_once(']') {
            Some((inner, _)) => inner.to_owned(),
            None => return false,
        },
        None => host.split(':').next().unwrap_or_default().to_owned(),
    };
    if let Ok(ip) = address.parse::<IpAddr>() {
        return ip.to_canonical() == local.to_canonical() && !local.is_unspecified();
    }
    let name = address.trim_end_matches('.');
    !name.is_empty() && names.iter().any(|kept| kept == name)
}

/// The admin socket, owner only (root passes anyway): any stale one is
/// replaced.
pub fn bind_admin(path: &Path) -> io::Result<UnixListener> {
    let _ = fs::remove_file(path);
    let listener = UnixListener::bind(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

/// Each connection to the admin socket gets one new code and a newline.
pub fn admin(listener: &UnixListener, monitor: &Monitor) {
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        match monitor.gate().issue(Instant::now()) {
            Ok(code) => {
                let _ = writeln!(stream, "{code}");
            }
            Err(error) => eprintln!("lamp-peek: no code made: {error}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(redeemed: Redeemed) -> String {
        match redeemed {
            Redeemed::Session(session) => session,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_code_works_once() {
        let (mut gate, now) = (Gate::default(), Instant::now());
        let code = gate.issue(now).unwrap();
        assert_eq!(code.len(), 64);
        let opened = session(gate.redeem(&code, now).unwrap());
        assert!(gate.admits(&opened, now));
        assert_eq!(gate.redeem(&code, now).unwrap(), Redeemed::Refused);
        assert_eq!(gate.redeem("", now).unwrap(), Redeemed::Refused);
        assert!(!gate.admits(&code, now), "a code is not a session");
    }

    /// On a fake clock: `now` is only what the test says it is.
    #[test]
    fn a_code_expires_after_five_minutes() {
        let (mut gate, start) = (Gate::default(), Instant::now());
        let late = gate.issue(start).unwrap();
        let in_time = gate.issue(start).unwrap();
        let just = start + CODE_LASTS - Duration::from_secs(1);
        session(gate.redeem(&in_time, just).unwrap());
        assert_eq!(
            gate.redeem(&late, start + CODE_LASTS).unwrap(),
            Redeemed::Refused
        );
    }

    #[test]
    fn a_fifth_browser_is_refused_and_sessions_end_after_twelve_hours() {
        let (mut gate, start) = (Gate::default(), Instant::now());
        let opened: Vec<String> = (0..MOST_SESSIONS)
            .map(|_| {
                let code = gate.issue(start).unwrap();
                session(gate.redeem(&code, start).unwrap())
            })
            .collect();
        let fifth = gate.issue(start).unwrap();
        assert_eq!(gate.redeem(&fifth, start).unwrap(), Redeemed::Full);
        let just = start + SESSION_LASTS - Duration::from_secs(1);
        assert!(opened.iter().all(|kept| gate.admits(kept, just)));
        let later = start + SESSION_LASTS;
        assert!(opened.iter().all(|kept| !gate.admits(kept, later)));
    }

    #[test]
    fn a_session_is_not_kept_across_a_restart() {
        let (mut gate, now) = (Gate::default(), Instant::now());
        let code = gate.issue(now).unwrap();
        let opened = session(gate.redeem(&code, now).unwrap());
        // A restart is a new gate: nothing is written anywhere.
        let mut restarted = Gate::default();
        assert!(!restarted.admits(&opened, now));
        assert_eq!(restarted.redeem(&code, now).unwrap(), Redeemed::Refused);
    }

    #[test]
    fn only_a_peer_on_the_local_network_is_answered() {
        for lan in [
            "10.1.2.3",
            "172.16.0.9",
            "172.31.255.1",
            "192.168.1.20",
            "169.254.3.4",
            "127.0.0.1",
            "::1",
            "fe80::1",
            "fd12:3456::1",
            "::ffff:192.168.1.20",
        ] {
            assert!(lan_peer(lan.parse().unwrap()), "{lan}");
        }
        for far in [
            "8.8.8.8",
            "172.32.0.1",
            "100.64.0.1",
            "203.0.113.9",
            "2001:db8::1",
            "::ffff:8.8.8.8",
        ] {
            assert!(!lan_peer(far.parse().unwrap()), "{far}");
        }
    }

    #[test]
    fn host_must_be_the_lamps_own_name_or_address() {
        let names = vec!["lamp-test.local".to_owned(), "lamp-test".to_owned()];
        let local: IpAddr = "192.168.1.5".parse().unwrap();
        for good in [
            "lamp-test.local",
            "LAMP-TEST.local:80",
            "lamp-test.local.",
            "lamp-test",
            "192.168.1.5",
            "192.168.1.5:80",
        ] {
            assert!(host_allowed(good, &names, local), "{good}");
        }
        for bad in [
            "rebound.example",
            "lamp-test.local.example",
            "192.168.1.6",
            "localhost",
            "",
            "[::1]",
            "[",
        ] {
            assert!(!host_allowed(bad, &names, local), "{bad}");
        }
        let v6: IpAddr = "fe80::5".parse().unwrap();
        assert!(host_allowed("[fe80::5]:80", &names, v6));
        let unknown: IpAddr = "0.0.0.0".parse().unwrap();
        assert!(!host_allowed("0.0.0.0", &names, unknown));
    }
}
