//! Names and the time zone changed from the app after setup, without a reset
//! (#1959, `docs/cloud.md`, "Changed after setup"). Off unless
//! `LAMP_SETTINGS_CHANGE=on`, with `LAMP_CLOUD=on`; without it each kind is
//! answered as any this lamp lacks.
//!
//! `lamp-cloud` writes nothing itself: it hands one line to root's helper,
//! `lamp-setup change`, on the socket systemd holds for it, and the helper
//! checks the request again and writes with setup's own writers. Each kind
//! is answered `starting` at once, as V1's `timezone.set`, and `success` or
//! `failure` once the helper has answered, on a thread of its own so the
//! broker's loop never waits on it.
//!
//! The Wi-Fi (`wifi.set`, #1959's second piece) has a switch and a helper of
//! its own, `LAMP_SETTINGS_WIFI=on` beside the one above and
//! `lamp-setup wifi` on [`WIFI_SOCKET`]: it is the one change that can take
//! the lamp off the network the app reaches it on. Its result may be ready
//! only once the lamp is back online, so results are taken only while
//! connected and kept until the broker has acknowledged them
//! (`crate::online`). The password goes into the helper's line and nowhere
//! else: never a reply, an error or a journal line.
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// `on` takes the kinds below, with `LAMP_CLOUD=on`.
pub const ENVIRONMENT: &str = "LAMP_SETTINGS_CHANGE";

/// The kinds, each answered `starting` and then its result.
pub const KINDS: [&str; 3] = ["timezone.set", "person.name.set", "device.rename"];

/// `on`, beside [`ENVIRONMENT`] and `LAMP_CLOUD`, takes [`WIFI_KIND`].
pub const WIFI_ENVIRONMENT: &str = "LAMP_SETTINGS_WIFI";

/// The Wi-Fi changed from the app: `{"ssid", "password"[, "request_id"]}`.
pub const WIFI_KIND: &str = "wifi.set";

/// The Wi-Fi helper's socket (`deploy/product/lamp-setup-wifi.socket`).
pub const WIFI_SOCKET: &str = "/run/lampos-setup-wifi/wifi.sock";

/// How long the Wi-Fi helper is waited on: 60 s for the new network, 60 s
/// for the old one, and the radio's restarts and the broker's checks.
pub const WIFI_WAIT: Duration = Duration::from_secs(180);

/// The longest `request_id` echoed back, in bytes.
const REQUEST_ID_MOST: usize = 64;

/// The helper's socket (`deploy/product/lamp-setup-change.socket`).
pub const SOCKET: &str = "/run/lampos-setup-change/change.sock";

/// The lamp's name in the app, as the helper writes it.
pub const DISPLAY_NAME: &str = "/etc/lampOS/setup/display-name";

/// How long the helper is waited on: it writes a few small files and
/// answers before anything it restarts later.
pub const WAIT: Duration = Duration::from_secs(10);

/// The longest text taken for a name or a zone, in bytes: the helper's own
/// bound (64 characters) fits in it.
const TEXT_MOST: usize = 512;

/// Whether the switch is on: both `LAMP_CLOUD` and `LAMP_SETTINGS_CHANGE` are `on`.
pub fn gates(cloud: Option<&str>, change: Option<&str>) -> bool {
    cloud == Some("on") && change == Some("on")
}

/// [`gates`] as the environment says.
pub fn on_from_environment() -> bool {
    gates(
        std::env::var("LAMP_CLOUD").ok().as_deref(),
        std::env::var(ENVIRONMENT).ok().as_deref(),
    )
}

/// Whether `wifi.set` is taken: [`gates`] and `LAMP_SETTINGS_WIFI=on` too.
pub fn wifi_gates(cloud: Option<&str>, change: Option<&str>, wifi: Option<&str>) -> bool {
    gates(cloud, change) && wifi == Some("on")
}

/// [`wifi_gates`] as the environment says.
pub fn wifi_on_from_environment() -> bool {
    wifi_gates(
        std::env::var("LAMP_CLOUD").ok().as_deref(),
        std::env::var(ENVIRONMENT).ok().as_deref(),
        std::env::var(WIFI_ENVIRONMENT).ok().as_deref(),
    )
}

/// The Wi-Fi helper's request line for the app's `data`, with the broker
/// the lamp is on (`host:port`), and the app's `request_id` if it gave one;
/// or why not. The helper checks the name and the password itself, with
/// setup's own words; here only their shape. No error holds a value.
pub fn wifi_line(data: &Value, broker: &str) -> Result<(String, Option<String>), String> {
    let ssid = data
        .get("ssid")
        .and_then(Value::as_str)
        .filter(|ssid| ssid.len() <= TEXT_MOST)
        .ok_or("ssid must be text")?;
    let password = data
        .get("password")
        .and_then(Value::as_str)
        .filter(|password| password.len() <= TEXT_MOST)
        .ok_or("password must be text")?;
    let request_id = match data.get("request_id") {
        None | Some(Value::Null) => None,
        Some(Value::String(id))
            if !id.is_empty()
                && id.len() <= REQUEST_ID_MOST
                && id.bytes().all(|b| b.is_ascii_graphic()) =>
        {
            Some(id.clone())
        }
        Some(_) => return Err("request_id must be 1 to 64 printable characters".into()),
    };
    if broker.is_empty() {
        return Err("the lamp is not on its broker".into());
    }
    let line = json!({"wifi": {"ssid": ssid, "password": password, "broker": broker}});
    Ok((line.to_string(), request_id))
}

/// The helper's request line for `kind` with the app's `data`, or why not.
/// The helper checks the text itself; here only its shape.
pub fn line(kind: &str, data: &Value) -> Result<String, String> {
    let (field, key, why) = match kind {
        "timezone.set" => ("timezone", "timezone", "timezone must be text"),
        "person.name.set" => ("name", "person_name", "name must be text"),
        "device.rename" => ("name", "display_name", "name must be text"),
        _ => return Err(crate::wire::not_here(kind)),
    };
    let text = data
        .get(field)
        .and_then(Value::as_str)
        .filter(|text| text.len() <= TEXT_MOST)
        .ok_or(why)?;
    Ok(json!({ key: text }).to_string())
}

/// What the helper's reply line says: its `data`, or why not.
pub fn result(reply: &str) -> Result<Option<Value>, String> {
    let reply: Value =
        serde_json::from_str(reply.trim()).map_err(|_| "the lamp's settings did not answer")?;
    if let Some(data) = reply.get("done") {
        return Ok(Some(data.clone()));
    }
    Err(reply
        .get("refused")
        .and_then(Value::as_str)
        .unwrap_or("the lamp's settings did not answer")
        .to_owned())
}

/// The helper over its socket: one line out, one line back, within `wait`.
pub fn ask(socket: &std::path::Path, line: &str, wait: Duration) -> Result<Option<Value>, String> {
    let unavailable = |_| "the lamp's settings are not running".to_owned();
    let mut stream = UnixStream::connect(socket).map_err(unavailable)?;
    stream
        .set_read_timeout(Some(wait))
        .and_then(|()| stream.set_write_timeout(Some(wait)))
        .map_err(unavailable)?;
    stream
        .write_all(format!("{line}\n").as_bytes())
        .map_err(unavailable)?;
    let mut reply = String::new();
    BufReader::new(stream.take(4_096))
        .read_line(&mut reply)
        .map_err(|_| "the lamp's settings did not answer in time".to_owned())?;
    result(&reply)
}

/// A change's result, waiting to be said.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub kind: String,
    pub result: Result<Option<Value>, String>,
    /// The app's own id for the request, said back in `data.request_id`.
    pub request_id: Option<String>,
}

impl Outcome {
    /// The reply for it, as `identity` says it: V1's `success` or `failure`,
    /// with the request's id in `data` if it had one.
    pub fn reply(&self, identity: &crate::wire::Identity, now: &str) -> Value {
        let mut reply = identity.data(now, &self.kind, &self.result);
        if let Some(id) = &self.request_id {
            let data = reply
                .as_object_mut()
                .map(|reply| reply.entry("data").or_insert_with(|| json!({})));
            if let Some(Value::Object(data)) = data {
                data.insert("request_id".into(), json!(id));
            }
        }
        reply
    }
}

/// The changes asked and their results, waiting to be said.
#[derive(Debug)]
pub struct Changes {
    pub socket: PathBuf,
    pub wait: Duration,
    /// The Wi-Fi helper's socket and wait, with `LAMP_SETTINGS_WIFI=on`.
    pub wifi: Option<(PathBuf, Duration)>,
    /// The broker the lamp is on now, `host:port`, for the Wi-Fi helper.
    broker: Mutex<String>,
    /// A Wi-Fi change is under way: a second is refused at once.
    wifi_busy: Arc<AtomicBool>,
    done: Arc<Mutex<Vec<Outcome>>>,
}

impl Changes {
    pub fn new(socket: impl Into<PathBuf>, wait: Duration) -> Arc<Self> {
        Self::with_wifi(socket, wait, None)
    }

    /// With the Wi-Fi helper's socket and wait, or none.
    pub fn with_wifi(
        socket: impl Into<PathBuf>,
        wait: Duration,
        wifi: Option<(PathBuf, Duration)>,
    ) -> Arc<Self> {
        Arc::new(Self {
            socket: socket.into(),
            wait,
            wifi,
            broker: Mutex::default(),
            wifi_busy: Arc::default(),
            done: Arc::default(),
        })
    }

    /// The product's: the helpers' sockets, the Wi-Fi's only when `wifi`.
    pub fn product(wifi: bool) -> Arc<Self> {
        Self::with_wifi(
            SOCKET,
            WAIT,
            wifi.then(|| (PathBuf::from(WIFI_SOCKET), WIFI_WAIT)),
        )
    }

    /// Whether `wifi.set` is taken.
    pub fn wifi_on(&self) -> bool {
        self.wifi.is_some()
    }

    /// The broker the lamp is on now, said at each connection.
    pub fn set_broker(&self, endpoint: &str, port: u16) {
        let host = endpoint.trim();
        let address = if host.contains(':') {
            format!("[{host}]:{port}")
        } else {
            format!("{host}:{port}")
        };
        if let Ok(mut broker) = self.broker.lock() {
            *broker = address;
        }
    }

    /// Asks the Wi-Fi helper for the app's `data` on a thread of its own;
    /// or why not, at once. One change at a time.
    pub fn start_wifi(&self, data: &Value) -> Result<(), String> {
        let (socket, wait) = self
            .wifi
            .clone()
            .ok_or_else(|| crate::wire::not_here(WIFI_KIND))?;
        let broker = self.broker.lock().map(|b| b.clone()).unwrap_or_default();
        let (line, request_id) = wifi_line(data, &broker)?;
        if self.wifi_busy.swap(true, Ordering::AcqRel) {
            return Err("a Wi-Fi change is already under way".into());
        }
        let (done, busy) = (self.done.clone(), self.wifi_busy.clone());
        std::thread::spawn(move || {
            let result = ask(&socket, &line, wait);
            busy.store(false, Ordering::Release);
            if let Ok(mut done) = done.lock() {
                done.push(Outcome {
                    kind: WIFI_KIND.into(),
                    result,
                    request_id,
                });
            }
        });
        Ok(())
    }

    /// Results handed to the broker and lost with the connection before it
    /// acknowledged them: said again, first, on the next one.
    pub fn owe(&self, again: Vec<Outcome>) {
        if again.is_empty() {
            return;
        }
        if let Ok(mut done) = self.done.lock() {
            let later = std::mem::take(&mut *done);
            done.extend(again);
            done.extend(later);
        }
    }

    /// Asks the helper for `line` on a thread of its own; its result is
    /// kept for [`Self::take`].
    pub fn start(&self, kind: &str, line: String) {
        let (socket, wait, done) = (self.socket.clone(), self.wait, self.done.clone());
        let kind = kind.to_owned();
        std::thread::spawn(move || {
            let result = ask(&socket, &line, wait);
            if let Ok(mut done) = done.lock() {
                done.push(Outcome {
                    kind,
                    result,
                    request_id: None,
                });
            }
        });
    }

    /// The results come since the last call, oldest first.
    pub fn take(&self) -> Vec<Outcome> {
        self.done
            .lock()
            .map(|mut done| std::mem::take(&mut *done))
            .unwrap_or_default()
    }
}

/// The lamp's name in the app from `text` (the helper's file), if it holds one.
pub fn display_name(text: &str) -> Option<String> {
    let name = text.lines().next()?.trim();
    (!name.is_empty() && name.chars().count() <= 64 && !name.chars().any(char::is_control))
        .then(|| name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::answer::{answer, tests::Fake};
    use crate::online::reply;
    use crate::wire::Identity;
    use std::os::unix::net::UnixListener;
    use std::time::Instant;

    fn identity() -> Identity {
        Identity {
            device: "lamp".into(),
            version: "0.1.0+abc".into(),
            id: "d-42".into(),
            mac: "lamp-7f72".into(),
            timezone: String::new(),
            local_ip: String::new(),
        }
    }

    fn socket(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("lcs-{}-{name}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }

    /// A helper on `path` that takes one line, waits `after`, and answers
    /// `answer`; it hands back the line it took.
    fn helper(
        path: &std::path::Path,
        after: Duration,
        answer: &'static str,
    ) -> std::thread::JoinHandle<String> {
        let listener = UnixListener::bind(path).unwrap();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            std::thread::sleep(after);
            let mut stream = stream;
            stream.write_all(answer.as_bytes()).unwrap();
            line
        })
    }

    /// Waits up to 5 s for the first result.
    fn first(changes: &Changes) -> (String, Result<Option<Value>, String>) {
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(done) = changes.take().into_iter().next() {
                return (done.kind, done.result);
            }
            assert!(Instant::now() < until, "no result");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn the_switch_needs_both_words() {
        assert!(gates(Some("on"), Some("on")));
        for (cloud, change) in [
            (None, Some("on")),
            (Some("on"), None),
            (Some("on"), Some("1")),
            (Some("off"), Some("on")),
        ] {
            assert!(!gates(cloud, change), "{cloud:?} {change:?}");
        }
    }

    /// Off, nothing is asked of the helper and each kind is one this lamp
    /// lacks, as before: `device.rename` too.
    #[test]
    fn off_each_kind_is_refused_as_one_this_lamp_lacks() {
        let mut lamp = Fake::default();
        for (kind, data) in [
            ("timezone.set", json!({"timezone": "Europe/Paris"})),
            ("person.name.set", json!({"name": "Lan"})),
            ("device.rename", json!({"name": "Desk lamp"})),
        ] {
            assert_eq!(
                answer(&mut lamp, kind, &data),
                Err(format!("not on this lamp: {kind}"))
            );
            let sent = format!(r#"{{"cmd":"data","kind":"{kind}","data":{data}}}"#);
            let replied: Value =
                serde_json::from_str(&reply(&identity(), &mut lamp, sent.as_bytes(), "t").unwrap())
                    .unwrap();
            assert_eq!(replied["status"], "failure");
        }
    }

    #[test]
    fn each_kind_becomes_one_line_for_the_helper() {
        assert_eq!(
            line("timezone.set", &json!({"timezone": "Europe/Paris"})),
            Ok(r#"{"timezone":"Europe/Paris"}"#.to_owned())
        );
        assert_eq!(
            line("person.name.set", &json!({"name": ""})),
            Ok(r#"{"person_name":""}"#.to_owned())
        );
        assert_eq!(
            line("device.rename", &json!({"name": "Desk lamp", "other": 1})),
            Ok(r#"{"display_name":"Desk lamp"}"#.to_owned())
        );
        assert_eq!(
            line("timezone.set", &json!({"zone": "Europe/Paris"})),
            Err("timezone must be text".to_owned())
        );
        assert_eq!(
            line("person.name.set", &json!({"name": 7})),
            Err("name must be text".to_owned())
        );
        assert_eq!(
            line("device.rename", &json!({"name": "x".repeat(513)})),
            Err("name must be text".to_owned())
        );
    }

    /// The ack at once, before the helper has answered (rule 7: the app
    /// gets an ack at once and the result after), then the result.
    #[test]
    fn a_change_is_acked_at_once_and_its_result_follows() {
        let path = socket("ack");
        let serving = helper(
            &path,
            Duration::from_millis(300),
            "{\"done\":{\"timezone\":\"Europe/Paris\"}}\n",
        );
        let changes = Changes::new(&path, Duration::from_secs(5));
        let mut lamp = Fake {
            changes: Some(changes.clone()),
            ..Fake::default()
        };
        let asked = Instant::now();
        let sent = br#"{"cmd":"data","kind":"timezone.set","data":{"timezone":"Europe/Paris"}}"#;
        let acked: Value =
            serde_json::from_str(&reply(&identity(), &mut lamp, sent, "t").unwrap()).unwrap();
        let ack_took = asked.elapsed();
        assert!(ack_took < Duration::from_millis(100), "{ack_took:?}");
        assert_eq!(
            (&acked["type"], &acked["kind"], &acked["status"]),
            (&json!("data"), &json!("timezone.set"), &json!("starting"))
        );
        assert!(acked.get("data").is_none());
        let (kind, result) = first(&changes);
        assert!(asked.elapsed() >= Duration::from_millis(300));
        assert_eq!(kind, "timezone.set");
        assert_eq!(result, Ok(Some(json!({"timezone": "Europe/Paris"}))));
        assert_eq!(serving.join().unwrap(), "{\"timezone\":\"Europe/Paris\"}\n");
        // As the loop says it: V1's success, with the zone.
        let said = identity().data("t", &kind, &result);
        assert_eq!(
            (&said["status"], &said["data"]["timezone"]),
            (&json!("success"), &json!("Europe/Paris"))
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_refusal_a_helper_not_running_and_one_that_never_answers_are_failures() {
        let path = socket("refused");
        let serving = helper(
            &path,
            Duration::ZERO,
            "{\"refused\":\"timezone must be a zone the lamp knows, such as Asia/Ho_Chi_Minh\"}\n",
        );
        let changes = Changes::new(&path, Duration::from_secs(5));
        let mut lamp = Fake {
            changes: Some(changes.clone()),
            ..Fake::default()
        };
        assert_eq!(
            answer(
                &mut lamp,
                "timezone.set",
                &json!({"timezone": "Mars/Olympus"})
            ),
            Ok(None)
        );
        let (_, result) = first(&changes);
        assert_eq!(
            result,
            Err("timezone must be a zone the lamp knows, such as Asia/Ho_Chi_Minh".to_owned())
        );
        serving.join().unwrap();
        let _ = std::fs::remove_file(&path);
        // A bad shape is refused at once, and nothing is asked.
        assert_eq!(
            answer(&mut lamp, "device.rename", &json!({})),
            Err("name must be text".to_owned())
        );
        // No helper.
        let gone = socket("gone");
        assert_eq!(
            ask(&gone, "{}", Duration::from_secs(1)),
            Err("the lamp's settings are not running".to_owned())
        );
        // A helper that takes the line and says nothing.
        let silent = socket("silent");
        let _listener = UnixListener::bind(&silent).unwrap();
        let started = Instant::now();
        assert_eq!(
            ask(&silent, "{}", Duration::from_millis(200)),
            Err("the lamp's settings did not answer in time".to_owned())
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        let _ = std::fs::remove_file(&silent);
        assert_eq!(
            result_of("nonsense"),
            Err("the lamp's settings did not answer".to_owned())
        );
    }

    fn result_of(reply: &str) -> Result<Option<Value>, String> {
        result(reply)
    }

    /// An obvious fake, never a real password.
    const FAKE_PASSWORD: &str = "fake-pass-NEW-1234";

    #[test]
    fn the_wifi_needs_its_own_switch_beside_the_names() {
        assert!(wifi_gates(Some("on"), Some("on"), Some("on")));
        for (cloud, change, wifi) in [
            (Some("on"), Some("on"), None),
            (Some("on"), None, Some("on")),
            (None, Some("on"), Some("on")),
            (Some("on"), Some("on"), Some("1")),
        ] {
            assert!(
                !wifi_gates(cloud, change, wifi),
                "{cloud:?} {change:?} {wifi:?}"
            );
        }
    }

    /// Off, `wifi.set` is a kind this lamp lacks, with the names' switch on
    /// or off, and nothing is asked of any helper.
    #[test]
    fn off_the_wifi_is_refused_as_a_kind_this_lamp_lacks() {
        let data = json!({"ssid": "New Home", "password": FAKE_PASSWORD});
        for changes in [None, Some(Changes::new(socket("never"), WAIT))] {
            let mut lamp = Fake {
                changes,
                ..Fake::default()
            };
            assert_eq!(
                answer(&mut lamp, WIFI_KIND, &data),
                Err("not on this lamp: wifi.set".to_owned())
            );
        }
    }

    #[test]
    fn the_wifi_line_carries_the_broker_and_no_error_holds_a_value() {
        let (line, id) = wifi_line(
            &json!({"ssid": "New Home", "password": FAKE_PASSWORD, "request_id": "r-7"}),
            "broker.test:1883",
        )
        .unwrap();
        let line: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(
            line,
            json!({"wifi": {"ssid": "New Home", "password": FAKE_PASSWORD, "broker": "broker.test:1883"}})
        );
        assert_eq!(id.as_deref(), Some("r-7"));
        for (data, broker, why) in [
            (
                json!({"password": FAKE_PASSWORD}),
                "b:1",
                "ssid must be text",
            ),
            (
                json!({"ssid": "New Home", "password": 7}),
                "b:1",
                "password must be text",
            ),
            (
                json!({"ssid": "New Home", "password": FAKE_PASSWORD, "request_id": 3}),
                "b:1",
                "request_id must be 1 to 64 printable characters",
            ),
            (
                json!({"ssid": "New Home", "password": FAKE_PASSWORD}),
                "",
                "the lamp is not on its broker",
            ),
        ] {
            let error = wifi_line(&data, broker).unwrap_err();
            assert_eq!(error, why);
            assert!(!error.contains(FAKE_PASSWORD));
        }
    }

    /// One change at a time; the result carries the app's id back, as a
    /// success and as a failure, and never the password.
    #[test]
    fn a_wifi_change_is_one_at_a_time_and_its_result_carries_the_request_id() {
        let path = socket("wifi");
        let serving = helper(
            &path,
            Duration::from_millis(300),
            "{\"done\":{\"ssid\":\"New Home\"}}\n",
        );
        let changes = Changes::with_wifi(socket("names"), WAIT, Some((path.clone(), WIFI_WAIT)));
        let mut lamp = Fake {
            changes: Some(changes.clone()),
            ..Fake::default()
        };
        let data = json!({"ssid": "New Home", "password": FAKE_PASSWORD, "request_id": "r-7"});
        // Not yet on a broker: refused at once.
        assert_eq!(
            answer(&mut lamp, WIFI_KIND, &data),
            Err("the lamp is not on its broker".to_owned())
        );
        changes.set_broker("broker.test", 1883);
        assert_eq!(answer(&mut lamp, WIFI_KIND, &data), Ok(None));
        assert_eq!(
            answer(&mut lamp, WIFI_KIND, &data),
            Err("a Wi-Fi change is already under way".to_owned())
        );
        let until = Instant::now() + Duration::from_secs(5);
        let done = loop {
            if let Some(done) = changes.take().into_iter().next() {
                break done;
            }
            assert!(Instant::now() < until, "no result");
            std::thread::sleep(Duration::from_millis(10));
        };
        let line: Value = serde_json::from_str(&serving.join().unwrap()).unwrap();
        assert_eq!(line["wifi"]["broker"], "broker.test:1883");
        assert_eq!(line["wifi"]["password"], FAKE_PASSWORD);
        let said = done.reply(&identity(), "t");
        assert_eq!(
            (&said["kind"], &said["status"], &said["data"]),
            (
                &json!("wifi.set"),
                &json!("success"),
                &json!({"ssid": "New Home", "request_id": "r-7"})
            )
        );
        let failed = Outcome {
            kind: WIFI_KIND.into(),
            result: Err("wrong WiFi password: the router rejected the key; the lamp is back on its old Wi-Fi".into()),
            request_id: Some("r-7".into()),
        }
        .reply(&identity(), "t");
        assert_eq!(failed["status"], "failure");
        assert_eq!(failed["data"], json!({"request_id": "r-7"}));
        for reply in [said, failed] {
            assert!(!reply.to_string().contains(FAKE_PASSWORD));
        }
        // Free again once answered.
        assert!(!changes.wifi_busy.load(Ordering::Acquire));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_broker_on_an_address_with_colons_is_bracketed() {
        let changes = Changes::with_wifi(socket("v6"), WAIT, None);
        changes.set_broker("fd00::1", 1883);
        assert_eq!(*changes.broker.lock().unwrap(), "[fd00::1]:1883");
    }

    #[test]
    fn the_lamps_name_in_the_app_is_read_from_the_helpers_file() {
        assert_eq!(display_name("Desk lamp\n"), Some("Desk lamp".into()));
        assert_eq!(display_name("  \n"), None);
        assert_eq!(display_name(""), None);
        assert_eq!(display_name(&"x".repeat(65)), None);
    }
}
