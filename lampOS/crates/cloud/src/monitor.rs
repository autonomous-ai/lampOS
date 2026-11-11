//! The owner's dev monitor from the app (#5883, `docs/cloud.md`, "The dev
//! monitor"): lamp-peek's monitor mode (`docs/peek.md`) switched on and off,
//! read, and opened by a one-time link. Off unless `LAMP_APP_MONITOR=on`,
//! with `LAMP_CLOUD=on` and `LAMP_SETTINGS_CHANGE=on` (its helper); without
//! them the kinds are answered as any this lamp lacks.
//!
//! - `monitor.set` (`{"on": bool}`) goes to root's helper, `lamp-setup
//!   change`, as `control.set` does: answered `starting`, then its result.
//!   On writes lamp-peek's drop-in and starts it; off stops it before the
//!   result, so no page stays open once the app says off.
//! - `monitor.get` (`{}`) reads the drop-in where the helper writes it:
//!   `{"on": bool}`. Nothing is asked of anyone.
//! - `monitor.link` (`{"seal_to", "request_id"}`) asks lamp-peek's admin
//!   socket for a one-time code, good once for 5 minutes, and answers the
//!   link sealed to the app's one-time X25519 key, bound to this lamp's
//!   device id and the app's `request_id`, as `control.code` is
//!   (`crate::control`): `{"sealed": {"v", "enc", "ct"}, "until_unix_ms"}`,
//!   `ct` opening to `{"url", "code", "until_unix_ms"}`, the url
//!   `http://<host>.local/in#<code>`. The code rides only in the fragment, so
//!   it is never in a request line; neither it nor the url is in a journal
//!   line, an error or the clear part of the reply.
use crate::control::{MUST_SEAL, NO_REQUEST_ID, NOT_SEALED};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// `on` takes [`KINDS`], beside `LAMP_CLOUD` and `LAMP_SETTINGS_CHANGE`.
pub const ENVIRONMENT: &str = "LAMP_APP_MONITOR";

/// The kinds, in `can`'s order.
pub const KINDS: [&str; 3] = [SET_KIND, "monitor.get", LINK_KIND];

/// The dev monitor on or off: `{"on": bool}`, to root's helper.
pub const SET_KIND: &str = "monitor.set";

/// A one-time link to the page, sealed.
pub const LINK_KIND: &str = "monitor.link";

/// lamp-peek's drop-in while the dev monitor is on, as root's helper writes
/// it (`lamp_setup::change::MONITOR_DROPIN`).
pub const DROPIN: &str = "/etc/systemd/system/lamp-peek.service.d/monitor.conf";

/// What it holds, exactly (`lamp_setup::change::MONITOR_DROPIN_TEXT`).
pub const DROPIN_TEXT: &str = "[Service]\nEnvironment=LAMP_PEEK=monitor\n";

/// lamp-peek's admin socket (`lamp_peek::monitor::DEFAULT_ADMIN_SOCKET`).
pub const ADMIN_SOCKET: &str = "/run/lampos-peek/admin.sock";

/// The HPKE `info` every link is sealed under.
pub const INFO: &[u8] = b"lampos/v1/monitor.link";

/// The sealed answer's format, so a later one can be told apart.
pub const SEALED_V: u64 = 1;

/// lamp-peek answers its admin socket at once; the broker's loop waits on
/// it, so it is bounded short.
pub const WAIT: Duration = Duration::from_secs(2);

/// A code lasts this long on lamp-peek (`lamp_peek::monitor::CODE_LASTS`).
pub const CODE_LASTS_MS: u64 = 5 * 60 * 1_000;

/// The app's refusals, shown as they are.
pub const OFF: &str = "the dev monitor is off on this lamp: turn it on first";
pub const UNREACHED: &str = "the dev monitor did not answer on the lamp now";

/// Where the switch is read, where a code is asked, and the device id each
/// link is bound to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub dropin: PathBuf,
    pub admin: PathBuf,
    /// The backend's device id, as every reply's `id` says it.
    pub device_id: String,
    pub wait: Duration,
}

/// Whether the kinds are taken: `LAMP_CLOUD`, `LAMP_SETTINGS_CHANGE` and
/// `LAMP_APP_MONITOR` each `on` exactly.
pub fn gates(cloud: Option<&str>, change: Option<&str>, monitor: Option<&str>) -> bool {
    crate::settings::gates(cloud, change) && monitor == Some("on")
}

impl Place {
    /// The product's, as the environment says: none when off.
    pub fn from_environment(device_id: &str) -> Option<Self> {
        let var = |name: &str| std::env::var(name).ok();
        gates(
            var("LAMP_CLOUD").as_deref(),
            var(crate::settings::ENVIRONMENT).as_deref(),
            var(ENVIRONMENT).as_deref(),
        )
        .then(|| Self {
            dropin: DROPIN.into(),
            admin: ADMIN_SOCKET.into(),
            device_id: device_id.to_owned(),
            wait: WAIT,
        })
    }

    /// Whether the dev monitor is on: its drop-in holds exactly what the
    /// helper writes.
    pub fn on(&self) -> bool {
        let mut text = String::new();
        std::fs::File::open(&self.dropin)
            .and_then(|file| file.take(1_024).read_to_string(&mut text))
            .is_ok()
            && text == DROPIN_TEXT
    }
}

/// `monitor.set`'s helper line: exactly one field, `on`, true or false.
pub fn line(data: &Value) -> Result<String, String> {
    let why = || "on must be true or false, and the only field".to_owned();
    let on = data
        .as_object()
        .filter(|object| object.len() == 1)
        .and_then(|object| object.get("on"))
        .and_then(Value::as_bool)
        .ok_or_else(why)?;
    Ok(json!({ "monitor": on }).to_string())
}

/// `monitor.get`: what the drop-in says, writing nothing.
pub fn get(place: &Place) -> Value {
    json!({"on": place.on()})
}

/// `monitor.link` for the lamp named `host`: its reply's `data`, or the
/// refusal the app shows. What it did goes to the journal.
pub fn link(place: &Place, host: &str, data: &Value) -> Result<Value, String> {
    link_saying(place, host, data, crate::chat::now_ms(), &mut |line| {
        eprintln!("lamp-cloud: {line}")
    })
}

/// [`link`] at `now_ms`, its journal lines handed to `say`. Everything the
/// app sent is checked before lamp-peek is asked, so no code is made for a
/// request that can't take it.
pub fn link_saying(
    place: &Place,
    host: &str,
    data: &Value,
    now_ms: u64,
    say: &mut dyn FnMut(&str),
) -> Result<Value, String> {
    let seal_to = data
        .get("seal_to")
        .and_then(Value::as_str)
        .and_then(crate::control::key_of)
        .ok_or(MUST_SEAL)?;
    let request_id = data
        .get("request_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_graphic()))
        .ok_or(NO_REQUEST_ID)?;
    if !place.on() {
        return Err(OFF.to_owned());
    }
    let code = ask(&place.admin, place.wait).map_err(|why| {
        say(&format!("{LINK_KIND} from the app: lamp-peek {why}"));
        UNREACHED.to_owned()
    })?;
    let until_unix_ms = now_ms.saturating_add(CODE_LASTS_MS);
    let url = format!("http://{host}.local/in#{code}");
    let opened = json!({"url": url, "code": code, "until_unix_ms": until_unix_ms});
    let aad = format!("{}\n{request_id}", place.device_id);
    let sealed = crate::seal::seal(
        &seal_to,
        INFO,
        aad.as_bytes(),
        opened.to_string().as_bytes(),
    )
    .map_err(|refused| match refused {
        crate::seal::Refused::BadKey => MUST_SEAL.to_owned(),
        crate::seal::Refused::Failed => NOT_SEALED.to_owned(),
    })?;
    say(&format!(
        "{LINK_KIND} from the app: a one-time link, sealed"
    ));
    Ok(json!({
        "sealed": {
            "v": SEALED_V,
            "enc": URL_SAFE_NO_PAD.encode(sealed.enc),
            "ct": URL_SAFE_NO_PAD.encode(&sealed.ct),
        },
        "until_unix_ms": until_unix_ms,
    }))
}

/// One code from lamp-peek's admin socket: 64 hex digits on one line.
fn ask(socket: &Path, wait: Duration) -> Result<String, String> {
    let stream = UnixStream::connect(socket).map_err(|error| format!("not reached: {error}"))?;
    stream
        .set_read_timeout(Some(wait))
        .map_err(|error| format!("not reached: {error}"))?;
    let mut line = String::new();
    BufReader::new(stream.take(128))
        .read_line(&mut line)
        .map_err(|error| format!("did not answer: {error}"))?;
    let code = line.trim();
    if code.len() != 64 || !code.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("gave no code".into());
    }
    Ok(code.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::net::UnixListener;

    const APP_KEY: [u8; 32] = [9; 32];
    const CODE: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    /// A short folder of its own: a socket's whole path fits in 103 bytes.
    fn folder(tag: &str) -> PathBuf {
        let dir = Path::new("/tmp").join(format!("cm-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A place in `dir`, and a stand-in for lamp-peek's admin socket that
    /// writes `answer` to each connection and counts them.
    fn place(dir: &Path, answer: &'static str) -> (Place, std::sync::mpsc::Receiver<()>) {
        let admin = dir.join("admin.sock");
        let listener = UnixListener::bind(&admin).unwrap();
        let (asked, connections) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let _ = asked.send(());
                let _ = writeln!(stream, "{answer}");
            }
        });
        let place = Place {
            dropin: dir.join("monitor.conf"),
            admin,
            device_id: "d-42".into(),
            wait: WAIT,
        };
        (place, connections)
    }

    fn asked(request_id: &str) -> Value {
        let key = crate::seal::public_of(&APP_KEY);
        json!({"seal_to": URL_SAFE_NO_PAD.encode(key), "request_id": request_id})
    }

    #[test]
    fn the_switch_is_on_exactly_and_needs_the_cloud_and_the_helper() {
        assert!(gates(Some("on"), Some("on"), Some("on")));
        for (cloud, change, monitor) in [
            (Some("on"), Some("on"), None),
            (Some("on"), Some("on"), Some("ON")),
            (Some("on"), Some("on"), Some("monitor")),
            (Some("on"), None, Some("on")),
            (None, Some("on"), Some("on")),
        ] {
            assert!(!gates(cloud, change, monitor), "{monitor:?}");
        }
    }

    #[test]
    fn a_set_is_exactly_one_boolean_on() {
        assert_eq!(line(&json!({"on": true})).unwrap(), r#"{"monitor":true}"#);
        assert_eq!(line(&json!({"on": false})).unwrap(), r#"{"monitor":false}"#);
        for data in [
            json!({}),
            json!(null),
            json!({"on": "yes"}),
            json!({"on": 1}),
            json!({"on": true, "frames": true}),
        ] {
            assert!(line(&data).is_err(), "{data}");
        }
    }

    #[test]
    fn a_get_reads_the_drop_in_as_the_helper_writes_it() {
        let dir = folder("get");
        let (place, _) = place(&dir, CODE);
        assert_eq!(get(&place), json!({"on": false}));
        std::fs::write(&place.dropin, DROPIN_TEXT).unwrap();
        assert_eq!(get(&place), json!({"on": true}));
        std::fs::write(&place.dropin, "[Service]\nEnvironment=LAMP_PEEK=on\n").unwrap();
        assert_eq!(get(&place), json!({"on": false}));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The link opens only with the app's key, for this lamp and this
    /// request; the code is never in the clear part or a journal line, and
    /// a request that can't take a code never makes one.
    #[test]
    fn a_link_is_sealed_to_the_app_and_bound_to_the_lamp_and_the_request() {
        let dir = folder("link");
        let (place, connections) = place(&dir, CODE);
        let mut lines = Vec::new();
        let mut say = |line: &str| lines.push(line.to_owned());
        // Off: refused before lamp-peek is asked.
        assert_eq!(
            link_saying(&place, "lamp-ab12", &asked("r-7"), 1_000, &mut say),
            Err(OFF.to_owned())
        );
        std::fs::write(&place.dropin, DROPIN_TEXT).unwrap();
        // No key, a key that isn't one, no request id: refused, no code made.
        for data in [
            json!({"request_id": "r-7"}),
            json!({"seal_to": "short", "request_id": "r-7"}),
            json!({"seal_to": asked("x")["seal_to"]}),
            json!({"seal_to": asked("x")["seal_to"], "request_id": ""}),
        ] {
            assert!(link_saying(&place, "lamp-ab12", &data, 1_000, &mut say).is_err());
        }
        assert!(connections.try_recv().is_err());
        let answered = link_saying(&place, "lamp-ab12", &asked("r-7"), 1_000, &mut say).unwrap();
        assert!(connections.try_recv().is_ok());
        assert_eq!(answered["sealed"]["v"], json!(1));
        assert_eq!(answered["until_unix_ms"], json!(301_000));
        let sealed = crate::seal::Sealed {
            enc: URL_SAFE_NO_PAD
                .decode(answered["sealed"]["enc"].as_str().unwrap())
                .unwrap()
                .try_into()
                .unwrap(),
            ct: URL_SAFE_NO_PAD
                .decode(answered["sealed"]["ct"].as_str().unwrap())
                .unwrap(),
        };
        let opened = crate::seal::open(&APP_KEY, &sealed, INFO, b"d-42\nr-7").unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&opened).unwrap(),
            json!({
                "url": format!("http://lamp-ab12.local/in#{CODE}"),
                "code": CODE,
                "until_unix_ms": 301_000
            })
        );
        assert!(crate::seal::open(&APP_KEY, &sealed, INFO, b"d-42\nr-8").is_err());
        assert!(crate::seal::open(&APP_KEY, &sealed, INFO, b"d-43\nr-7").is_err());
        let reply = answered.to_string();
        for said in lines.iter().chain([&reply]) {
            assert!(!said.contains(&CODE[..16]), "{said}");
        }
        assert_eq!(
            lines,
            ["monitor.link from the app: a one-time link, sealed"]
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn no_code_from_lamp_peek_is_refused_plainly() {
        let dir = folder("bad");
        let (place, _) = place(&dir, "not a code");
        std::fs::write(&place.dropin, DROPIN_TEXT).unwrap();
        let mut lines = Vec::new();
        let refused = link_saying(&place, "lamp-ab12", &asked("r-1"), 0, &mut |line| {
            lines.push(line.to_owned())
        });
        assert_eq!(refused, Err(UNREACHED.to_owned()));
        assert_eq!(lines, ["monitor.link from the app: lamp-peek gave no code"]);
        // lamp-peek not running: its socket is gone.
        std::fs::remove_file(&place.admin).unwrap();
        assert_eq!(
            link_saying(&place, "lamp-ab12", &asked("r-1"), 0, &mut |_| {}),
            Err(UNREACHED.to_owned())
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
