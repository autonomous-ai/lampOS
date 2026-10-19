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
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// `on` takes the kinds below, with `LAMP_CLOUD=on`.
pub const ENVIRONMENT: &str = "LAMP_SETTINGS_CHANGE";

/// The kinds, each answered `starting` and then its result.
pub const KINDS: [&str; 3] = ["timezone.set", "person.name.set", "device.rename"];

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

/// A change's kind and its result.
pub type Outcome = (String, Result<Option<Value>, String>);

/// The changes asked and their results, waiting to be said.
#[derive(Debug)]
pub struct Changes {
    pub socket: PathBuf,
    pub wait: Duration,
    done: Arc<Mutex<Vec<Outcome>>>,
}

impl Changes {
    pub fn new(socket: impl Into<PathBuf>, wait: Duration) -> Arc<Self> {
        Arc::new(Self {
            socket: socket.into(),
            wait,
            done: Arc::default(),
        })
    }

    /// The product's: the helper's socket.
    pub fn product() -> Arc<Self> {
        Self::new(SOCKET, WAIT)
    }

    /// Asks the helper for `line` on a thread of its own; its result is
    /// kept for [`Self::take`].
    pub fn start(&self, kind: &str, line: String) {
        let (socket, wait, done) = (self.socket.clone(), self.wait, self.done.clone());
        let kind = kind.to_owned();
        std::thread::spawn(move || {
            let result = ask(&socket, &line, wait);
            if let Ok(mut done) = done.lock() {
                done.push((kind, result));
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
                return done;
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

    #[test]
    fn the_lamps_name_in_the_app_is_read_from_the_helpers_file() {
        assert_eq!(display_name("Desk lamp\n"), Some("Desk lamp".into()));
        assert_eq!(display_name("  \n"), None);
        assert_eq!(display_name(""), None);
        assert_eq!(display_name(&"x".repeat(65)), None);
    }
}
