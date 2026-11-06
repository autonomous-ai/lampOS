//! Harness's own updates held, and its health read with no credential
//! (#5229; docs/tasks.md, "Harness held at its pinned version"). Off unless
//! `LAMP_HARNESS_HOLD_UPDATES=on` in lamp-tasks' environment.
//!
//! From Harness's source (openharness main 577954332): its daemon reads
//! `ADAPTER_UPDATE_DISABLE` from its own environment (`config/env.ts`) and
//! holds its self-update when it is `true` (`masterProcess.ts:278`), except
//! on a Team account, which ignores it on purpose. `harness start` either
//! spawns the daemon with the caller's environment, or rewrites the user
//! unit `harness service install` wrote from the caller's values of a fixed
//! list (`lib/platformDaemon.ts:42`) that has both `ADAPTER_UPDATE_DISABLE`
//! and `HARNESS_ENV_FILE`; a start from a shell without them drops them.
//! The one file Harness loads is `HARNESS_ENV_FILE` (dotenv, not
//! overriding). So with the switch on, lamp-tasks writes [`FILE`] in
//! Harness's home with `ADAPTER_UPDATE_DISABLE=true` and gives every Harness
//! command both variables ([`hold`]).
//!
//! The daemon answers `GET /api/health` on 127.0.0.1 with no credential,
//! `{"ok":true,"version":"x.y.z"}` once its server is up, signed in or not
//! (`hookServer.ts:527`), on `PORT`, 18473 unless set (`config/env.ts:124`).
//! After each `harness start` that succeeded, [`watch_health`] asks it, in a
//! thread of its own so nothing waits on it, and journals one fixed line.
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    io::{self, Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpStream},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

/// The switch, in lamp-tasks' own unit: `on` exactly.
pub const SWITCH: &str = "LAMP_HARNESS_HOLD_UPDATES";
/// The variable Harness's daemon holds its updates by.
pub const DISABLE: &str = "ADAPTER_UPDATE_DISABLE";
/// The file Harness loads, by this variable.
pub const ENV_FILE: &str = "HARNESS_ENV_FILE";
/// The file's name, in Harness's home (`HOME`), 0600.
pub const FILE: &str = "harness-hold.env";
/// What the file says.
pub const TEXT: &str = "ADAPTER_UPDATE_DISABLE=true\n";
/// Harness's port unless `PORT` is set.
pub const DEFAULT_PORT: u16 = 18473;
/// How long after a start the health is asked for, at most.
pub const HEALTH_WITHIN: Duration = Duration::from_secs(20);
/// Between two asks.
const HEALTH_EVERY: Duration = Duration::from_millis(500);
/// One ask's bound, to connect, write and read.
const ASK_WITHIN: Duration = Duration::from_secs(2);
/// The longest answer read.
const MOST_ANSWER: u64 = 16 * 1024;

/// `on` exactly.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// With `environment`'s `HOME`, [`FILE`] written there (0600, whole) and
/// both [`ENV_FILE`] and [`DISABLE`] added to `environment`. What was
/// written is returned; an error when there is no absolute `HOME` or the
/// file cannot be written, with `environment` unchanged.
pub fn hold(environment: &mut BTreeMap<String, OsString>) -> Result<PathBuf, String> {
    let home = environment
        .get("HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
        .ok_or("no absolute HOME")?;
    let path = home.join(FILE);
    write_whole(&path, TEXT.as_bytes()).map_err(|error| format!("{}: {error}", path.display()))?;
    environment.insert(ENV_FILE.into(), path.clone().into());
    environment.insert(DISABLE.into(), "true".into());
    Ok(path)
}

fn write_whole(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let next = path.with_extension("env.next");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&next)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&next, path)
}

/// The port Harness listens on, from `environment`'s `PORT`.
pub fn port(environment: &BTreeMap<String, OsString>) -> u16 {
    environment
        .get("PORT")
        .and_then(|port| port.to_str())
        .and_then(|port| port.parse::<u16>().ok())
        .filter(|port| *port != 0)
        .unwrap_or(DEFAULT_PORT)
}

/// What one ask of the health said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Health {
    /// 200 with `ok: true`, and the version it said.
    Ok(Option<String>),
    /// An answer that isn't that: its fixed reason.
    Bad(&'static str),
}

/// One `GET /api/health` on 127.0.0.1:`port`, no credential, within
/// [`ASK_WITHIN`]. `Err`: nothing listening or nothing answered.
pub fn ask(port: u16) -> io::Result<Health> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&address, ASK_WITHIN)?;
    stream.set_read_timeout(Some(ASK_WITHIN))?;
    stream.set_write_timeout(Some(ASK_WITHIN))?;
    write!(
        stream,
        "GET /api/health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )?;
    let mut answer = Vec::new();
    stream.take(MOST_ANSWER).read_to_end(&mut answer)?;
    Ok(read(&answer))
}

/// An http answer read as [`Health`].
fn read(answer: &[u8]) -> Health {
    let Some(split) = answer.windows(4).position(|end| end == b"\r\n\r\n") else {
        return Health::Bad("no_answer");
    };
    let (head, body) = (&answer[..split], &answer[split + 4..]);
    let status = head.split(|&b| b == b'\r').next().unwrap_or_default();
    if !(status.starts_with(b"HTTP/1.1 200 ") || status.starts_with(b"HTTP/1.0 200 ")) {
        return Health::Bad("not_200");
    }
    let chunked = head.split(|&b| b == b'\n').any(|line| {
        let line = String::from_utf8_lossy(line).to_ascii_lowercase();
        line.trim()
            .strip_prefix("transfer-encoding:")
            .is_some_and(|value| value.trim() == "chunked")
    });
    let whole;
    let body = if chunked {
        let Some(joined) = dechunk(body) else {
            return Health::Bad("not_json");
        };
        whole = joined;
        &whole[..]
    } else {
        body
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return Health::Bad("not_json");
    };
    if value.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
        return Health::Bad("not_ok");
    }
    let version = value
        .get("version")
        .and_then(serde_json::Value::as_str)
        .filter(|version| {
            version.len() <= 32
                && version
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-+".contains(&b))
        })
        .map(str::to_owned);
    Health::Ok(version)
}

/// A chunked body joined, up to its last (empty) chunk; `None` when a size
/// isn't hex or a chunk is cut short.
fn dechunk(mut body: &[u8]) -> Option<Vec<u8>> {
    let mut whole = Vec::new();
    loop {
        let end = body.windows(2).position(|crlf| crlf == b"\r\n")?;
        let size = std::str::from_utf8(&body[..end]).ok()?;
        let size = size.split(';').next()?.trim();
        let size = usize::from_str_radix(size, 16).ok()?;
        body = &body[end + 2..];
        if size == 0 {
            return Some(whole);
        }
        if body.len() < size + 2 || &body[size..size + 2] != b"\r\n" {
            return None;
        }
        whole.extend_from_slice(&body[..size]);
        body = &body[size + 2..];
    }
}

/// The journal line for what the health said within `within`, asking every
/// [`HEALTH_EVERY`] until it answers.
pub fn health_line(port: u16, within: Duration) -> String {
    let started = Instant::now();
    loop {
        let asked = ask(port);
        let waited = started.elapsed();
        match asked {
            Ok(Health::Ok(version)) => {
                return format!(
                    "tasks: Harness health ok on 127.0.0.1:{port}, version {}, {} ms after its start",
                    version.as_deref().unwrap_or("unsaid"),
                    waited.as_millis()
                );
            }
            Ok(Health::Bad(reason)) if waited >= within => {
                return format!("tasks: Harness health failed on 127.0.0.1:{port}: {reason}");
            }
            Err(_) if waited >= within => {
                return format!(
                    "tasks: Harness health failed on 127.0.0.1:{port}: no_answer within {} s",
                    within.as_secs()
                );
            }
            _ => thread::sleep(HEALTH_EVERY),
        }
    }
}

/// [`health_line`] in a thread of its own, its line journaled.
pub fn watch_health(port: u16) {
    let _ = thread::Builder::new()
        .name("harness-health".into())
        .spawn(move || eprintln!("{}", health_line(port, HEALTH_WITHIN)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{net::TcpListener, os::unix::fs::PermissionsExt};

    #[test]
    fn the_hold_writes_its_file_and_gives_harness_both_variables() {
        let home = tempfile::tempdir().unwrap();
        let mut environment = BTreeMap::new();
        environment.insert("HOME".to_owned(), OsString::from(home.path()));
        let path = hold(&mut environment).unwrap();
        assert_eq!(path, home.path().join(FILE));
        assert_eq!(fs::read_to_string(&path).unwrap(), TEXT);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(environment[DISABLE], OsString::from("true"));
        assert_eq!(environment[ENV_FILE], OsString::from(&path));
        // Again: the same file, whole.
        fs::write(&path, "ADAPTER_UPDATE_DISABLE=false\n").unwrap();
        hold(&mut environment).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), TEXT);
    }

    #[test]
    fn no_home_holds_nothing() {
        let mut environment = BTreeMap::new();
        assert!(hold(&mut environment).is_err());
        environment.insert("HOME".to_owned(), OsString::from("relative"));
        assert!(hold(&mut environment).is_err());
        assert!(!environment.contains_key(DISABLE));
    }

    #[test]
    fn only_on_exactly_is_on() {
        assert!(on(Some("on")));
        for value in [None, Some("off"), Some("ON"), Some("true"), Some("")] {
            assert!(!on(value), "{value:?}");
        }
    }

    #[test]
    fn the_port_is_harnesss_own_unless_set() {
        let mut environment = BTreeMap::new();
        assert_eq!(port(&environment), DEFAULT_PORT);
        environment.insert("PORT".to_owned(), OsString::from("18500"));
        assert_eq!(port(&environment), 18500);
        environment.insert("PORT".to_owned(), OsString::from("0"));
        assert_eq!(port(&environment), DEFAULT_PORT);
    }

    #[test]
    fn an_answer_is_read_as_harness_gives_it() {
        let ok = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"ok\":true,\"version\":\"0.5.39\"}";
        assert_eq!(read(ok), Health::Ok(Some("0.5.39".into())));
        assert_eq!(
            read(b"HTTP/1.1 200 OK\r\n\r\n{\"ok\":true}"),
            Health::Ok(None)
        );
        assert_eq!(
            read(b"HTTP/1.1 401 Unauthorized\r\n\r\n{}"),
            Health::Bad("not_200")
        );
        assert_eq!(
            read(b"HTTP/1.1 200 OK\r\n\r\n{\"ok\":false}"),
            Health::Bad("not_ok")
        );
        assert_eq!(
            read(b"HTTP/1.1 200 OK\r\n\r\n<html>"),
            Health::Bad("not_json")
        );
        assert_eq!(read(b"HTTP/1.1 200"), Health::Bad("no_answer"));
    }

    #[test]
    fn a_chunked_answer_is_read_whole() {
        // An HTTP/1.1 server may answer in chunks: on lamp-c0a4
        // (#5379) the line said not_json though the daemon was up.
        let chunked = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n10\r\n{\"ok\":true,\"vers\r\nE\r\nion\":\"0.5.39\"}\r\n0\r\n\r\n";
        assert_eq!(read(chunked), Health::Ok(Some("0.5.39".into())));
        let upper =
            b"HTTP/1.1 200 OK\r\ntransfer-encoding: Chunked\r\n\r\nB\r\n{\"ok\":true}\r\n0\r\n\r\n";
        assert_eq!(read(upper), Health::Ok(None));
        // A chunk cut short or a size that isn't hex is still not json.
        let cut = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n40\r\n{\"ok\":true}";
        assert_eq!(read(cut), Health::Bad("not_json"));
        let bad = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n{}\r\n0\r\n\r\n";
        assert_eq!(read(bad), Health::Bad("not_json"));
    }

    /// A one-answer server on loopback, the request it was sent kept.
    fn server(answer: &'static [u8]) -> (u16, thread::JoinHandle<String>) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let thread = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut asked = Vec::new();
            let mut chunk = [0u8; 512];
            while !asked.windows(4).any(|end| end == b"\r\n\r\n") {
                let read = stream.read(&mut chunk).unwrap();
                if read == 0 {
                    break;
                }
                asked.extend_from_slice(&chunk[..read]);
            }
            stream.write_all(answer).unwrap();
            String::from_utf8(asked).unwrap()
        });
        (port, thread)
    }

    #[test]
    fn the_health_is_asked_with_no_credential_and_journaled() {
        let (port, asked) = server(b"HTTP/1.1 200 OK\r\n\r\n{\"ok\":true,\"version\":\"0.5.39\"}");
        let line = health_line(port, Duration::from_secs(5));
        let asked = asked.join().unwrap();
        assert!(asked.starts_with("GET /api/health HTTP/1.1\r\n"), "{asked}");
        assert!(
            !asked.to_ascii_lowercase().contains("authorization"),
            "{asked}"
        );
        assert!(!asked.to_ascii_lowercase().contains("cookie"), "{asked}");
        assert!(
            line.starts_with(&format!(
                "tasks: Harness health ok on 127.0.0.1:{port}, version 0.5.39, "
            )),
            "{line}"
        );
    }

    #[test]
    fn nothing_listening_is_said_once_its_time_is_up() {
        let port = {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            listener.local_addr().unwrap().port()
        };
        let line = health_line(port, Duration::ZERO);
        assert_eq!(
            line,
            format!("tasks: Harness health failed on 127.0.0.1:{port}: no_answer within 0 s")
        );
    }
}
