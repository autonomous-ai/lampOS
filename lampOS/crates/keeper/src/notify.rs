//! systemd's notify protocol, as `sd_notify(3)` describes it: one datagram
//! of `KEY=value` lines to the socket named in `$NOTIFY_SOCKET`.
//!
//! A part says `ready()` once it can do its work, `alive()` from its main
//! loop, and `stopping()` on its way out. systemd then knows a part that is
//! started from one that is ready, and with `WatchdogSec=` restarts one whose
//! loop has stopped turning, not only one whose process has died.
//!
//! Without `$NOTIFY_SOCKET` (a part run by hand, or a unit that is not
//! `Type=notify`) every call is a silent no-op, and a datagram that cannot be
//! sent is let go: telling systemd must never stop a part.
use std::{
    cell::Cell,
    env,
    os::unix::net::UnixDatagram,
    time::{Duration, Instant},
};

/// Where to tell systemd, and how often it wants to hear `alive`.
#[derive(Debug, Default)]
pub struct Notify {
    /// The socket, if there is one: a path, or `@name` for an abstract one.
    socket: Option<String>,
    /// Half the watchdog's interval: `alive` is sent at most this often, so
    /// twice in each interval, as `sd_watchdog_enabled(3)` advises.
    alive_every: Option<Duration>,
    /// When `alive` was last sent.
    alive_sent: Cell<Option<Instant>>,
}

impl Notify {
    /// As systemd started this process: `$NOTIFY_SOCKET`, and the watchdog
    /// from `$WATCHDOG_USEC` when `$WATCHDOG_PID` is unset or is this
    /// process (a child started by a part is not the one watched).
    pub fn from_environment() -> Self {
        let mine =
            env::var("WATCHDOG_PID").map_or(true, |pid| pid.parse() == Ok(std::process::id()));
        let watchdog = env::var("WATCHDOG_USEC")
            .ok()
            .and_then(|usec| usec.parse::<u64>().ok())
            .filter(|usec| mine && *usec > 0)
            .map(Duration::from_micros);
        Self::new(env::var("NOTIFY_SOCKET").ok(), watchdog)
    }

    /// To `socket`, with a watchdog of `watchdog` if there is one.
    pub fn new(socket: Option<String>, watchdog: Option<Duration>) -> Self {
        Self {
            socket: socket.filter(|socket| !socket.is_empty()),
            alive_every: watchdog.map(|interval| interval / 2),
            alive_sent: Cell::new(None),
        }
    }

    /// The part can do its work.
    pub fn ready(&self) {
        self.send("READY=1");
    }

    /// The part's loop turned. Cheap to call every step: it is sent at most
    /// twice in each watchdog interval, and never without a watchdog.
    pub fn alive(&self) {
        self.alive_at(Instant::now());
    }

    fn alive_at(&self, now: Instant) {
        let Some(every) = self.alive_every else {
            return;
        };
        if self
            .alive_sent
            .get()
            .is_some_and(|sent| now.saturating_duration_since(sent) < every)
        {
            return;
        }
        self.alive_sent.set(Some(now));
        self.send("WATCHDOG=1");
    }

    /// The part is still starting and needs `by` more before it says
    /// ready: systemd's start timeout is pushed out to at least `by` from
    /// now (`EXTEND_TIMEOUT_USEC`).
    pub fn extend_timeout(&self, by: Duration) {
        self.send(&format!("EXTEND_TIMEOUT_USEC={}", by.as_micros()));
    }

    /// One line for `systemctl status` to show.
    pub fn status(&self, text: &str) {
        // A newline would start another assignment of the protocol.
        self.send(&format!("STATUS={}", text.replace('\n', " ")));
    }

    /// The part is on its way out.
    pub fn stopping(&self) {
        self.send("STOPPING=1");
    }

    fn send(&self, message: &str) {
        let Some(socket) = self.socket.as_deref() else {
            return;
        };
        let Ok(out) = UnixDatagram::unbound() else {
            return;
        };
        let _ = match socket.strip_prefix('@') {
            Some(name) => send_abstract(&out, name, message),
            None => out.send_to(message.as_bytes(), socket),
        };
    }
}

#[cfg(target_os = "linux")]
fn send_abstract(out: &UnixDatagram, name: &str, message: &str) -> std::io::Result<usize> {
    use std::os::linux::net::SocketAddrExt;
    let address = std::os::unix::net::SocketAddr::from_abstract_name(name)?;
    out.send_to_addr(message.as_bytes(), &address)
}

/// Abstract sockets are Linux's alone; systemd never names one elsewhere.
#[cfg(not(target_os = "linux"))]
fn send_abstract(_: &UnixDatagram, _: &str, _: &str) -> std::io::Result<usize> {
    Err(std::io::ErrorKind::Unsupported.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A real socket in a directory of its own, and what it was sent.
    struct Ear {
        socket: UnixDatagram,
        path: PathBuf,
    }

    impl Ear {
        fn new(name: &str) -> Self {
            let directory = env::temp_dir().join(format!("keeper-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&directory);
            std::fs::create_dir_all(&directory).unwrap();
            let path = directory.join("notify");
            let socket = UnixDatagram::bind(&path).unwrap();
            socket.set_nonblocking(true).unwrap();
            Self { socket, path }
        }

        fn notify(&self, watchdog: Option<Duration>) -> Notify {
            Notify::new(Some(self.path.to_str().unwrap().to_owned()), watchdog)
        }

        fn heard(&self) -> Vec<Vec<u8>> {
            let mut heard = Vec::new();
            let mut buffer = [0u8; 256];
            while let Ok(length) = self.socket.recv(&mut buffer) {
                heard.push(buffer[..length].to_vec());
            }
            heard
        }
    }

    impl Drop for Ear {
        fn drop(&mut self) {
            if let Some(directory) = self.path.parent() {
                let _ = std::fs::remove_dir_all(directory);
            }
        }
    }

    #[test]
    fn each_word_is_one_datagram_byte_for_byte() {
        let ear = Ear::new("words");
        let notify = ear.notify(Some(Duration::from_secs(10)));
        notify.ready();
        notify.alive();
        notify.status("holding\nstill");
        notify.extend_timeout(Duration::from_millis(41_500));
        notify.stopping();
        assert_eq!(
            ear.heard(),
            [
                b"READY=1".to_vec(),
                b"WATCHDOG=1".to_vec(),
                b"STATUS=holding still".to_vec(),
                b"EXTEND_TIMEOUT_USEC=41500000".to_vec(),
                b"STOPPING=1".to_vec(),
            ]
        );
    }

    #[test]
    fn alive_every_step_is_sent_at_most_twice_an_interval() {
        let ear = Ear::new("alive");
        let notify = ear.notify(Some(Duration::from_secs(1)));
        let start = Instant::now();
        // Fifty steps a second for two seconds.
        for step in 0..100u32 {
            notify.alive_at(start + Duration::from_millis(20) * step);
        }
        assert_eq!(ear.heard(), vec![b"WATCHDOG=1".to_vec(); 4]);
    }

    #[test]
    fn without_a_watchdog_alive_says_nothing() {
        let ear = Ear::new("nowatchdog");
        let notify = ear.notify(None);
        notify.alive();
        notify.ready();
        assert_eq!(ear.heard(), [b"READY=1".to_vec()]);
    }

    #[test]
    fn without_a_socket_everything_is_a_no_op() {
        let notify = Notify::new(None, Some(Duration::from_secs(1)));
        notify.ready();
        notify.alive();
        notify.status("x");
        notify.stopping();
        // And a socket that is not there is let go, not a fault.
        let gone = Notify::new(Some("/nonexistent/notify".into()), None);
        gone.ready();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn an_abstract_socket_is_reached_by_its_at_name() {
        use std::os::linux::net::SocketAddrExt;
        let name = format!("lamp-keeper-test-{}", std::process::id());
        let address = std::os::unix::net::SocketAddr::from_abstract_name(&name).unwrap();
        let ear = UnixDatagram::bind_addr(&address).unwrap();
        ear.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
        Notify::new(Some(format!("@{name}")), None).ready();
        let mut buffer = [0u8; 32];
        let length = ear.recv(&mut buffer).unwrap();
        assert_eq!(&buffer[..length], b"READY=1");
    }
}
