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
//!   `{"on": bool}`, with since when and until when while on, or when it
//!   last turned itself off ([`get`]). Nothing is asked of anyone.
//! - `monitor.link` (`{"seal_to", "request_id"}`) asks lamp-peek's admin
//!   socket for a one-time code, good once for 5 minutes, and answers the
//!   link sealed to the app's one-time X25519 key, bound to this lamp's
//!   device id and the app's `request_id`, as `control.code` is
//!   (`crate::control`): `{"sealed": {"v", "enc", "ct"}, "until_unix_ms"}`,
//!   `ct` opening to `{"url", "code", "until_unix_ms"}`, the url
//!   `http://<host>.local/in#<code>`. The code rides only in the fragment, so
//!   it is never in a request line; neither it nor the url is in a journal
//!   line, an error or the clear part of the reply.
//!
//! While it is on, [`keep`] says so in the lamp's health (`dev monitor`,
//! "on since 2 h 5 min ago, off by itself in 21 h 55 min"), and turns it off
//! through the same helper once it has been on for [`DAY`] (#7432): the
//! words anyone says to the lamp are not shown for longer than the owner
//! is likely to remember. `monitor.get` says since when, and when it last
//! turned itself off.
use crate::control::{MUST_SEAL, NO_REQUEST_ID, NOT_SEALED};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};

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

/// `monitor.set` answers on once lamp-peek is started, a moment before it
/// binds its admin socket (#6401: a link asked 43 ms too early on
/// lamp-0c89). A socket that is not there yet, or not yet taking
/// connections, is tried again every [`RETRY_EVERY`] for this long before
/// the link is refused.
pub const STARTING: Duration = Duration::from_secs(3);

/// How often a socket not there yet is tried again within [`STARTING`].
pub const RETRY_EVERY: Duration = Duration::from_millis(100);

/// A code lasts this long on lamp-peek (`lamp_peek::monitor::CODE_LASTS`).
pub const CODE_LASTS_MS: u64 = 5 * 60 * 1_000;

/// The app's refusals, shown as they are.
pub const OFF: &str = "the dev monitor is off on this lamp: turn it on first";
pub const UNREACHED: &str = "the dev monitor did not answer on the lamp now";

/// The dev monitor turns itself off this long after it was turned on
/// (#7432), counted from its drop-in's time.
pub const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// A shorter day, for a lamp row only: seconds, from 60 to a day.
pub const DAY_ENVIRONMENT: &str = "LAMP_APP_MONITOR_DAY_S";

/// Its item in the lamp's health (`docs/health.md`).
pub const HEALTH_NAME: &str = "dev monitor";

/// How often [`keep`] looks and says it: within lamp-health's 5 s stale
/// time and the bus's 3 s lease.
pub const LOOK_EVERY: Duration = Duration::from_secs(2);

/// A turn-off the helper did not take is asked again after this.
pub const TRY_AGAIN: Duration = Duration::from_secs(60);

/// In lamp-cloud's state folder: when the dev monitor last turned itself
/// off, in unix ms, until it is on again.
pub const OFF_FILE: &str = "monitor-off";

/// Where the switch is read, where a code is asked, and the device id each
/// link is bound to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub dropin: PathBuf,
    pub admin: PathBuf,
    /// The backend's device id, as every reply's `id` says it.
    pub device_id: String,
    pub wait: Duration,
    /// How long a socket not there yet is waited for ([`STARTING`]).
    pub starting: Duration,
    /// How long it stays on: [`DAY`], or [`DAY_ENVIRONMENT`]'s.
    pub day: Duration,
    /// [`OFF_FILE`] in lamp-cloud's state folder, if it has one.
    pub went_off: Option<PathBuf>,
}

/// [`DAY_ENVIRONMENT`]'s value: whole seconds from 60 to a day, else a day.
pub fn day_from(value: Option<&str>) -> Duration {
    value
        .and_then(|seconds| seconds.parse::<u64>().ok())
        .filter(|seconds| (60..=DAY.as_secs()).contains(seconds))
        .map_or(DAY, Duration::from_secs)
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
            starting: STARTING,
            day: day_from(var(DAY_ENVIRONMENT).as_deref()),
            went_off: std::env::var_os("STATE_DIRECTORY")
                .map(|dir| PathBuf::from(dir).join(OFF_FILE)),
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

    /// Since when it is on, in unix ms: its drop-in's time, while it is on.
    pub fn since_ms(&self) -> Option<u64> {
        if !self.on() {
            return None;
        }
        let at = std::fs::metadata(&self.dropin).and_then(|meta| meta.modified());
        Some(at.ok()?.duration_since(UNIX_EPOCH).ok()?.as_millis() as u64)
    }

    /// When it last turned itself off, in unix ms, if it has not been on
    /// since.
    pub fn went_off_ms(&self) -> Option<u64> {
        std::fs::read_to_string(self.went_off.as_ref()?)
            .ok()?
            .trim()
            .parse()
            .ok()
    }
}

/// `monitor.set`'s helper line and the app's `request_id`: `on`, true or
/// false, and an optional `request_id` (as `wifi.set`'s), nothing else.
pub fn line(data: &Value) -> Result<(String, Option<String>), String> {
    let why = || "on must be true or false, and the only field besides request_id".to_owned();
    let object = data
        .as_object()
        .filter(|object| object.keys().all(|key| key == "on" || key == "request_id"))
        .ok_or_else(why)?;
    let on = object.get("on").and_then(Value::as_bool).ok_or_else(why)?;
    let request_id = crate::settings::request_id(data)?;
    Ok((json!({ "monitor": on }).to_string(), request_id))
}

/// `monitor.get`: what the drop-in says, writing nothing. On, since when
/// and when it turns itself off; off, when it last turned itself off, if
/// it did and has not been on since.
pub fn get(place: &Place) -> Value {
    match (place.since_ms(), place.went_off_ms()) {
        (Some(since), _) => json!({
            "on": true,
            "since_unix_ms": since,
            "off_at_unix_ms": since.saturating_add(place.day.as_millis() as u64),
        }),
        (None, Some(at)) => json!({"on": false, "went_off_by_itself_unix_ms": at}),
        (None, None) => json!({"on": false}),
    }
}

/// What [`keep`] finds at one look.
#[derive(Debug, PartialEq, Eq)]
pub enum Look {
    Off,
    /// On, what the lamp's health says, and whether its day is over.
    On {
        said: String,
        due: bool,
    },
}

/// The dev monitor on since `since_ms` (or off), at `now_ms`, with `day`.
/// A clock set back before it was turned on counts as just on.
pub fn look(since_ms: Option<u64>, day: Duration, now_ms: u64) -> Look {
    let Some(since) = since_ms else {
        return Look::Off;
    };
    let on_for = now_ms.saturating_sub(since);
    let left = (day.as_millis() as u64).saturating_sub(on_for);
    if left == 0 {
        return Look::On {
            said: format!("on since {} ago, turning itself off now", span(on_for)),
            due: true,
        };
    }
    Look::On {
        said: format!(
            "on since {} ago, off by itself in {}",
            span(on_for),
            span(left)
        ),
        due: false,
    }
}

/// "3 h 12 min", "4 min": whole minutes, not rounded up.
fn span(ms: u64) -> String {
    let minutes = ms / 60_000;
    if minutes < 60 {
        format!("{minutes} min")
    } else {
        format!("{} h {} min", minutes / 60, minutes % 60)
    }
}

/// Watches the dev monitor on a thread of its own for as long as lamp-cloud
/// runs: while it is on, says so in the lamp's health every
/// [`LOOK_EVERY`]; once it is off, says it stopped on purpose, so the item
/// goes; once its day is over, asks root's helper at `helper` to turn it
/// off, as the app's off does, and keeps when in [`OFF_FILE`].
pub fn keep(place: Place, bus: Option<lamp_bus::Bus>, helper: PathBuf) {
    use lamp_msgs::Diagnostic;
    std::thread::spawn(move || {
        let mut health = bus.and_then(|bus| {
            bus.publisher_declared(
                Diagnostic::TOPIC,
                &Diagnostic::declaration(),
                crate::lamp::SOURCE,
            )
            .ok()
        });
        let mut keeping = Keeping::default();
        loop {
            let report = keeping.step(
                &place,
                &helper,
                crate::chat::now_ms(),
                Instant::now(),
                &mut |line| eprintln!("lamp-cloud: {line}"),
            );
            if let (Some(publisher), Some(report)) = (health.as_mut(), report) {
                let _ = publisher.publish_json(lamp_ipc::monotonic_us(), &report);
            }
            std::thread::sleep(LOOK_EVERY);
        }
    });
}

/// What [`keep`] carries from one look to the next.
#[derive(Debug, Default)]
struct Keeping {
    was_on: bool,
    /// When the helper was last asked to turn it off, while its day is over.
    tried: Option<Instant>,
}

impl Keeping {
    /// One look at `now_ms` (`now` for the re-ask): what the lamp's health
    /// is told, if anything; the day over, the off asked of root's helper.
    fn step(
        &mut self,
        place: &Place,
        helper: &Path,
        now_ms: u64,
        now: Instant,
        say: &mut dyn FnMut(&str),
    ) -> Option<lamp_msgs::Diagnostic> {
        use lamp_msgs::{Diagnostic, Level};
        match look(place.since_ms(), place.day, now_ms) {
            Look::On { said, due } => {
                // On again since it turned itself off: that is no longer
                // the news.
                if !self.was_on
                    && let Some(file) = &place.went_off
                {
                    let _ = std::fs::remove_file(file);
                }
                self.was_on = true;
                if due
                    && self
                        .tried
                        .is_none_or(|at| now.duration_since(at) >= TRY_AGAIN)
                {
                    self.tried = Some(now);
                    turn_off(place, helper, now_ms, say);
                }
                Some(Diagnostic::new(Level::Ok, HEALTH_NAME, &said))
            }
            Look::Off => {
                self.tried = None;
                std::mem::take(&mut self.was_on).then(|| Diagnostic::stopping(HEALTH_NAME))
            }
        }
    }
}

/// Its day over: off through root's helper at `helper`, as the app's
/// `monitor.set {"on": false}`, and when kept for `monitor.get`.
pub fn turn_off(place: &Place, helper: &Path, now_ms: u64, say: &mut dyn FnMut(&str)) {
    let line = json!({"monitor": false}).to_string();
    match crate::settings::ask(helper, &line, crate::settings::WAIT) {
        Ok(done) => {
            if let Some(file) = &place.went_off {
                let _ = std::fs::write(file, now_ms.to_string());
            }
            say(&format!(
                "the dev monitor was on for its day: turned off ({})",
                done.unwrap_or_default()
            ));
        }
        Err(why) => say(&format!(
            "the dev monitor's day is over, but it was not turned off: {why}; asked again in a minute"
        )),
    }
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
    let code = ask(&place.admin, place.wait, place.starting).map_err(|why| {
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

/// One code from lamp-peek's admin socket: 64 hex digits on one line. A
/// socket missing or refusing is tried again for `starting`, as lamp-peek
/// just switched on binds it a moment later; any other failure, at once.
fn ask(socket: &Path, wait: Duration, starting: Duration) -> Result<String, String> {
    let until = Instant::now() + starting;
    let stream = loop {
        match UnixStream::connect(socket) {
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
                ) && Instant::now() < until =>
            {
                std::thread::sleep(RETRY_EVERY)
            }
            connected => break connected.map_err(|error| format!("not reached: {error}"))?,
        }
    };
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
            starting: STARTING,
            day: DAY,
            went_off: Some(dir.join(OFF_FILE)),
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
    fn a_set_is_one_boolean_on_and_an_optional_request_id() {
        let line_of = |data: Value| line(&data).unwrap();
        assert_eq!(
            line_of(json!({"on": true})),
            (r#"{"monitor":true}"#.into(), None)
        );
        assert_eq!(
            line_of(json!({"on": false})),
            (r#"{"monitor":false}"#.into(), None)
        );
        assert_eq!(
            line_of(json!({"on": true, "request_id": "r-7"})),
            (r#"{"monitor":true}"#.into(), Some("r-7".into()))
        );
        for data in [
            json!({}),
            json!(null),
            json!({"request_id": "r-7"}),
            json!({"on": "yes"}),
            json!({"on": 1}),
            json!({"on": true, "frames": true}),
            json!({"on": true, "request_id": "r-7", "frames": true}),
            json!({"on": true, "request_id": ""}),
            json!({"on": true, "request_id": 7}),
            json!({"on": true, "request_id": "r 7"}),
            json!({"on": true, "request_id": "r".repeat(65)}),
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
        let got = get(&place);
        assert_eq!(got["on"], json!(true));
        let since = got["since_unix_ms"].as_u64().unwrap();
        assert!(since.abs_diff(crate::chat::now_ms()) < 60_000, "{got}");
        assert_eq!(got["off_at_unix_ms"], json!(since + 86_400_000));
        std::fs::write(&place.dropin, "[Service]\nEnvironment=LAMP_PEEK=on\n").unwrap();
        assert_eq!(get(&place), json!({"on": false}));
        // Turned itself off: said until it is on again.
        std::fs::write(dir.join(OFF_FILE), "1700000000000").unwrap();
        assert_eq!(
            get(&place),
            json!({"on": false, "went_off_by_itself_unix_ms": 1_700_000_000_000u64})
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_day_is_a_day_unless_a_row_asks_for_a_shorter_one() {
        assert_eq!(day_from(None), DAY);
        assert_eq!(day_from(Some("300")), Duration::from_secs(300));
        assert_eq!(day_from(Some("60")), Duration::from_secs(60));
        for value in ["59", "86401", "0", "-5", "5m", ""] {
            assert_eq!(day_from(Some(value)), DAY, "{value}");
        }
    }

    #[test]
    fn health_says_since_when_and_when_it_goes_off_by_itself() {
        let hour = 3_600_000;
        assert_eq!(look(None, DAY, 5 * hour), Look::Off);
        assert_eq!(
            look(Some(hour), DAY, hour + 3 * hour + 12 * 60_000),
            Look::On {
                said: "on since 3 h 12 min ago, off by itself in 20 h 48 min".into(),
                due: false
            }
        );
        assert_eq!(
            look(Some(hour), DAY, hour + 59_999),
            Look::On {
                said: "on since 0 min ago, off by itself in 23 h 59 min".into(),
                due: false
            }
        );
        assert_eq!(
            look(Some(hour), DAY, hour + 24 * hour),
            Look::On {
                said: "on since 24 h 0 min ago, turning itself off now".into(),
                due: true
            }
        );
        // A clock set back counts as just on, never as over.
        assert!(matches!(
            look(Some(5 * hour), DAY, hour),
            Look::On { due: false, .. }
        ));
        assert!(matches!(
            look(Some(0), Duration::from_secs(300), 300_000),
            Look::On { due: true, .. }
        ));
    }

    /// Its day over, the off goes to root's helper as the app's does, and
    /// when is kept for `monitor.get`; a helper that refuses keeps nothing.
    #[test]
    fn a_day_over_turns_it_off_through_the_helper_and_keeps_when() {
        let dir = folder("off");
        let (place, _) = place(&dir, CODE);
        let helper = dir.join("change.sock");
        let listener = UnixListener::bind(&helper).unwrap();
        let (heard, lines_in) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for (n, stream) in listener.incoming().flatten().enumerate() {
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                let _ = heard.send(line);
                let reply = if n == 0 {
                    r#"{"refused":"busy"}"#
                } else {
                    r#"{"done":{"on":false,"closed":true}}"#
                };
                let _ = writeln!(&stream, "{reply}");
            }
        });
        let mut said = Vec::new();
        turn_off(&place, &helper, 42, &mut |line| said.push(line.to_owned()));
        assert_eq!(place.went_off_ms(), None);
        turn_off(&place, &helper, 42, &mut |line| said.push(line.to_owned()));
        assert_eq!(place.went_off_ms(), Some(42));
        assert_eq!(lines_in.recv().unwrap(), "{\"monitor\":false}\n");
        assert_eq!(lines_in.recv().unwrap(), "{\"monitor\":false}\n");
        assert_eq!(
            said,
            [
                "the dev monitor's day is over, but it was not turned off: busy; asked again in a minute",
                r#"the dev monitor was on for its day: turned off ({"closed":true,"on":false})"#,
            ]
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// `keep`'s looks, one at a time: on again, `monitor-off` goes; the day
    /// over, the off is asked of the helper, and again only a minute
    /// later; off, the item goes, also for a lamp-cloud that starts with
    /// the dev monitor already off.
    #[test]
    fn keep_asks_the_off_again_a_minute_later_and_lets_the_item_go() {
        let dir = folder("keep");
        let (place, _) = place(&dir, CODE);
        let helper = dir.join("change.sock");
        let listener = UnixListener::bind(&helper).unwrap();
        let (heard, lines_in) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut line = String::new();
                BufReader::new(&stream).read_line(&mut line).unwrap();
                let _ = heard.send(line);
                let _ = writeln!(&stream, r#"{{"refused":"busy"}}"#);
            }
        });
        std::fs::write(place.went_off.as_ref().unwrap(), "7").unwrap();
        std::fs::write(&place.dropin, DROPIN_TEXT).unwrap();
        let since = place.since_ms().unwrap();
        let over = since + DAY.as_millis() as u64;
        let at = Instant::now();
        let step = |keeping: &mut Keeping, now_ms: u64, now: Instant| {
            keeping.step(&place, &helper, now_ms, now, &mut |_| {})
        };
        let mut keeping = Keeping::default();
        assert!(!step(&mut keeping, since + 1_000, at).unwrap().is_stopping());
        assert_eq!(place.went_off_ms(), None);
        assert!(lines_in.try_recv().is_err());
        assert!(step(&mut keeping, over, at).is_some());
        assert_eq!(lines_in.try_recv().unwrap(), "{\"monitor\":false}\n");
        step(&mut keeping, over, at + TRY_AGAIN - Duration::from_secs(1));
        assert!(lines_in.try_recv().is_err());
        step(&mut keeping, over, at + TRY_AGAIN);
        assert_eq!(lines_in.try_recv().unwrap(), "{\"monitor\":false}\n");
        std::fs::remove_file(&place.dropin).unwrap();
        assert!(step(&mut keeping, over, at).unwrap().is_stopping());
        assert!(step(&mut keeping, over, at).is_none());
        // A new lamp-cloud, the dev monitor turned off while it was down:
        // the item it left is let go once, not left stale.
        let mut keeping = Keeping::default();
        assert!(step(&mut keeping, over, at).unwrap().is_stopping());
        assert!(step(&mut keeping, over, at).is_none());
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

    /// The app asks for a link the moment `monitor.set` said on, before
    /// lamp-peek has bound its admin socket (#6401: 43 ms early on
    /// lamp-0c89): the socket appears 300 ms later and the link still comes.
    #[test]
    fn a_link_asked_as_lamp_peek_starts_waits_for_its_socket() {
        let dir = folder("late");
        let admin = dir.join("admin.sock");
        let place = Place {
            dropin: dir.join("monitor.conf"),
            admin: admin.clone(),
            device_id: "d-42".into(),
            wait: WAIT,
            starting: STARTING,
            day: DAY,
            went_off: None,
        };
        std::fs::write(&place.dropin, DROPIN_TEXT).unwrap();
        let late = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            let listener = UnixListener::bind(&admin).unwrap();
            let (mut stream, _) = listener.accept().unwrap();
            writeln!(stream, "{CODE}").unwrap();
        });
        let mut lines = Vec::new();
        let answered = link_saying(&place, "lamp-ab12", &asked("r-2"), 0, &mut |line| {
            lines.push(line.to_owned())
        });
        assert!(answered.is_ok(), "{answered:?} {lines:?}");
        assert_eq!(
            lines,
            ["monitor.link from the app: a one-time link, sealed"]
        );
        late.join().unwrap();
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
        let _ = std::fs::remove_dir_all(dir);
    }

    /// lamp-peek that never comes up, its socket gone or left behind by
    /// one that stopped: refused as before once `starting` has passed, the
    /// reason in the journal.
    #[test]
    fn lamp_peek_not_running_is_refused_once_the_start_wait_passes() {
        let dir = folder("gone");
        let (mut place, _) = place(&dir, CODE);
        place.starting = Duration::from_millis(300);
        std::fs::write(&place.dropin, DROPIN_TEXT).unwrap();
        // Gone: no socket at all.
        std::fs::remove_file(&place.admin).unwrap();
        // Left behind: a socket file nobody listens on.
        let stale = dir.join("stale.sock");
        drop(UnixListener::bind(&stale).unwrap());
        for (admin, kind) in [
            (place.admin.clone(), std::io::ErrorKind::NotFound),
            (stale, std::io::ErrorKind::ConnectionRefused),
        ] {
            let error = UnixStream::connect(&admin).unwrap_err();
            assert_eq!(error.kind(), kind);
            place.admin = admin;
            let mut lines = Vec::new();
            let began = Instant::now();
            let refused = link_saying(&place, "lamp-ab12", &asked("r-1"), 0, &mut |line| {
                lines.push(line.to_owned())
            });
            assert!(began.elapsed() >= place.starting);
            assert_eq!(refused, Err(UNREACHED.to_owned()));
            assert_eq!(
                lines,
                [format!(
                    "monitor.link from the app: lamp-peek not reached: {error}"
                )]
            );
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
