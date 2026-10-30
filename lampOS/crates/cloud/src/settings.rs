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
//!
//! The owner's approximate place (`owner.place`, `owner.place.clear`,
//! #4333, `docs/owner-place.md`) goes to the same helper as the names, with
//! `LAMP_OWNER_PLACE=on` beside the switch above, rounded here first
//! ([`place_line`]). Its clear needs only the helper ([`PLACE_CLEAR_KIND`]).
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// `on` takes the kinds below, with `LAMP_CLOUD=on`.
pub const ENVIRONMENT: &str = "LAMP_SETTINGS_CHANGE";

/// The kinds, each answered `starting` and then its result.
pub const KINDS: [&str; 4] = [
    "timezone.set",
    "person.name.set",
    "device.rename",
    // The owner's language (#4473): `{"language": "en"|"vi"|"ja"|"zh"|"ko"}`,
    // checked by the helper, which restarts the voice once it rests.
    "language.set",
];

/// `on`, beside [`ENVIRONMENT`] and `LAMP_CLOUD`, takes [`WIFI_KIND`].
pub const WIFI_ENVIRONMENT: &str = "LAMP_SETTINGS_WIFI";

/// The Wi-Fi changed from the app: `{"ssid", "password"[, "request_id"]}`.
pub const WIFI_KIND: &str = "wifi.set";

/// The Wi-Fi helper's socket (`deploy/product/lamp-setup-wifi.socket`).
pub const WIFI_SOCKET: &str = "/run/lampos-setup-wifi/wifi.sock";

/// How long the Wi-Fi helper is waited on for each of its lines: 60 s for
/// the new network, 60 s for the link back over it and 120 s for the app's
/// confirmation, and up to 3 tries of the radio's restart (45 s) and 60 s
/// for the old one (#1959's P1, #2617's P2-1).
pub const WIFI_WAIT: Duration = Duration::from_secs(600);

/// The app's confirmation that the lamp reached it over the new network:
/// `{"request_id"?}`, answered at once: `success` when it came within the
/// window [`WIFI_JOINED_KIND`] gave, and again for a second one within it;
/// `failure` after the window, and the lamp goes back (#1959's P1, the
/// first builder's P2-2 on #2617).
pub const WIFI_CONFIRM_KIND: &str = "wifi.confirm";

/// Said between `wifi.set`'s `starting` and its result once the new network
/// is proven and the link is up again over it: "joined, confirm please",
/// `{"ssid", "confirm_within_s", "request_id"?}`. `wifi.set`'s `success`
/// comes only once the app's confirmation has kept the network. An app
/// that does not know this kind never confirms, and the lamp goes back.
pub const WIFI_JOINED_KIND: &str = "wifi.joined";

/// How much sooner than the helper's `online_within_ms` lamp-cloud stops
/// waiting for the broker's ack of [`WIFI_JOINED_KIND`] (the first
/// builder's follow-up 2 on #2617): the helper's wait starts before its
/// line is written, lamp-cloud's after it is read, so lamp-cloud's is the
/// helper's less this, and an ack lamp-cloud takes is one the helper still
/// waits for.
pub const WIFI_ONLINE_MARGIN: Duration = Duration::from_secs(1);

/// How long a Wi-Fi change's result said with no join waits for the
/// broker's ack before the link is made again (follow-up 3): the radio was
/// switched and back, so the old link may be dead, and the keepalive alone
/// would notice only after 60 to 120 s.
pub const WIFI_RESULT_ACK_WAIT: Duration = Duration::from_secs(5);

/// `on`, beside [`ENVIRONMENT`] and `LAMP_CLOUD`, takes [`KEYS_KINDS`]: a
/// person's own DeepSeek and Deepgram keys (#2094, `docs/own-keys.md`).
pub const KEYS_ENVIRONMENT: &str = "LAMP_OWN_KEYS";

/// A key checked and kept (`{"service", "key"[, "base_url"][, "request_id"]}`),
/// removed (`{"service"[, "request_id"]}`), or whose each service uses.
pub const KEYS_KINDS: [&str; 3] = ["keys.set", "keys.remove", "keys.get"];

/// `on`, beside [`ENVIRONMENT`] and `LAMP_CLOUD`, takes [`PLACE_KINDS`]: the
/// owner's approximate place, for "near me" (#4333, `docs/owner-place.md`).
pub const PLACE_ENVIRONMENT: &str = "LAMP_OWNER_PLACE";

/// The owner's place set (`owner.place`, `{"name", "lat", "lon", "set_at"}` with
/// `set_at` in ISO 8601 UTC, rounded to
/// about a kilometre here and again by the helper) and cleared (`{}`), each
/// answered `starting` and then its result, as the names are.
pub const PLACE_KINDS: [&str; 2] = ["owner.place", PLACE_CLEAR_KIND];

/// The owner's place cleared (`{}`): taken whenever root's helper is here,
/// [`PLACE_ENVIRONMENT`] on or not, so a place kept while it was on is never
/// left behind when it is turned off (#4356). Listed in `can` only with it.
pub const PLACE_CLEAR_KIND: &str = "owner.place.clear";

/// The keys helper's socket (`deploy/product/lamp-setup-keys.socket`).
pub const KEYS_SOCKET: &str = "/run/lampos-setup-keys/keys.sock";

/// How long the keys helper is waited on: its one check (5 s) and a write.
pub const KEYS_WAIT: Duration = Duration::from_secs(15);

/// The longest key passed on, in bytes: the helper's own bound.
const KEY_MOST: usize = 512;

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

/// Whether [`KEYS_KINDS`] are taken: [`gates`] and `LAMP_OWN_KEYS=on` too.
pub fn keys_gates(cloud: Option<&str>, change: Option<&str>, keys: Option<&str>) -> bool {
    gates(cloud, change) && keys == Some("on")
}

/// [`keys_gates`] as the environment says.
pub fn keys_on_from_environment() -> bool {
    keys_gates(
        std::env::var("LAMP_CLOUD").ok().as_deref(),
        std::env::var(ENVIRONMENT).ok().as_deref(),
        std::env::var(KEYS_ENVIRONMENT).ok().as_deref(),
    )
}

/// Whether [`PLACE_KINDS`] are taken: [`gates`] and `LAMP_OWNER_PLACE=on` too.
pub fn place_gates(cloud: Option<&str>, change: Option<&str>, place: Option<&str>) -> bool {
    gates(cloud, change) && place == Some("on")
}

/// [`place_gates`] as the environment says.
pub fn place_on_from_environment() -> bool {
    place_gates(
        std::env::var("LAMP_CLOUD").ok().as_deref(),
        std::env::var(ENVIRONMENT).ok().as_deref(),
        std::env::var(PLACE_ENVIRONMENT).ok().as_deref(),
    )
}

/// The helper's request line for one of [`PLACE_KINDS`] with the app's
/// `data`, or why not: the place checked and rounded
/// ([`lamp_msgs::owner_place::fit`]), so what leaves lamp-cloud is never
/// finer than the grid; `null` clears it. No error holds a value.
pub fn place_line(kind: &str, data: &Value) -> Result<String, String> {
    match kind {
        "owner.place" => {
            let place = lamp_msgs::owner_place::fit(data)?;
            Ok(json!({ "owner_place": place }).to_string())
        }
        "owner.place.clear" => Ok(json!({ "owner_place": null }).to_string()),
        _ => Err(crate::wire::not_here(kind)),
    }
}

/// The app's `request_id`, if it gave one: 1 to 64 printable characters.
fn request_id(data: &Value) -> Result<Option<String>, String> {
    match data.get("request_id") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(id))
            if !id.is_empty()
                && id.len() <= REQUEST_ID_MOST
                && id.bytes().all(|b| b.is_ascii_graphic()) =>
        {
            Ok(Some(id.clone()))
        }
        Some(_) => Err("request_id must be 1 to 64 printable characters".into()),
    }
}

/// The keys helper's request line for `kind` with the app's `data`, and the
/// app's `request_id`; or why not. The helper checks the key and the
/// endpoint itself; here only their shape. No error holds a value.
pub fn keys_line(kind: &str, data: &Value) -> Result<(String, Option<String>), String> {
    let request_id = request_id(data)?;
    let service = || {
        data.get("service")
            .and_then(Value::as_str)
            .filter(|service| matches!(*service, "deepseek" | "deepgram"))
            .ok_or("service must be deepseek or deepgram")
    };
    let line = match kind {
        "keys.set" => {
            let key = data
                .get("key")
                .and_then(Value::as_str)
                .filter(|key| key.len() <= KEY_MOST)
                .ok_or("key must be text")?;
            let base_url = match data.get("base_url") {
                None | Some(Value::Null) => None,
                Some(Value::String(url)) if url.len() <= 2_048 => Some(url.as_str()),
                Some(_) => return Err("base_url must be text".into()),
            };
            let mut line = json!({"set": service()?, "key": key});
            if let Some(url) = base_url {
                line["base_url"] = json!(url);
            }
            line
        }
        "keys.remove" => json!({"remove": service()?}),
        "keys.get" => json!({"which": true}),
        _ => return Err(crate::wire::not_here(kind)),
    };
    Ok((line.to_string(), request_id))
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
    let request_id = request_id(data)?;
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
        "language.set" => ("language", "language", "language must be text"),
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

/// A Wi-Fi change's hold on `lamp-cloud`'s link to the broker (the
/// reviewer-lead's P2 and the first builder's P2-1 on #2617, Mobile's lost
/// result on #1959): the network under the link changes, so the old link
/// is dropped and made again at once, and a change's result is said only
/// on the new one.
#[derive(Debug, Default)]
enum Relink {
    /// Nothing to do.
    #[default]
    Idle,
    /// The change wants the link dropped and made again.
    Wanted,
    /// Dropped; the next connection is over the network the lamp is on now.
    Making,
}

/// The app's confirmation, as the change's thread waits for it.
#[derive(Debug)]
enum Confirm {
    /// Handed to the change until `by`, the helper's window as lamp-cloud
    /// keeps it, started before the helper's own so never ending after it;
    /// with the change's `request_id`, if the app gave one.
    Waiting(std::sync::mpsc::Sender<Event>, Instant, Option<String>),
    /// Taken and handed on: a second one within the window is `success`
    /// again, never handed twice.
    Taken(Instant, Option<String>),
}

/// What a Wi-Fi change shares with the broker's loop and with the app's
/// `wifi.confirm`.
#[derive(Debug, Default)]
pub struct WifiLink {
    relink: Mutex<Relink>,
    /// Wakes the broker's loop, even in its backoff, when a relink is wanted.
    asked: tokio::sync::Notify,
    /// The change waits for the broker's ack of this kind's result: of
    /// [`WIFI_JOINED_KIND`], when the window starts; of [`WIFI_KIND`]'s
    /// failure with no join, else the link is made again.
    awaiting_ack: Mutex<Option<(&'static str, std::sync::mpsc::Sender<Event>)>>,
    confirm: Mutex<Option<Confirm>>,
}

impl WifiLink {
    fn want_relink(&self) {
        if let Ok(mut relink) = self.relink.lock() {
            *relink = Relink::Wanted;
        }
        self.asked.notify_one();
    }

    /// Waits for the broker's ack of `kind`'s result from now on.
    fn await_ack(&self, kind: &'static str, events: std::sync::mpsc::Sender<Event>) {
        if let Ok(mut waiting) = self.awaiting_ack.lock() {
            *waiting = Some((kind, events));
        }
    }

    fn stop_awaiting_ack(&self) {
        if let Ok(mut waiting) = self.awaiting_ack.lock() {
            *waiting = None;
        }
    }

    /// Whether a `wifi.joined` still waits for its ack: once its wait has
    /// ended, one not yet said is never said (follow-up 3).
    fn joined_wanted(&self) -> bool {
        self.awaiting_ack.lock().is_ok_and(|waiting| {
            waiting
                .as_ref()
                .is_some_and(|(kind, _)| *kind == WIFI_JOINED_KIND)
        })
    }

    fn settled(&self) -> bool {
        self.relink
            .lock()
            .map(|relink| matches!(*relink, Relink::Idle))
            .unwrap_or(true)
    }
}

/// The time left until `by`, at least none.
fn left(by: Instant) -> Duration {
    by.saturating_duration_since(Instant::now())
}

/// The Wi-Fi helper over its socket (#1959's P1, #2617's P2s). Its first
/// line is the result, or `{"joined": {"ssid", "online_within_ms",
/// "confirm_within_ms"}}` once the new network is proven. Then the link is
/// made again, [`WIFI_JOINED_KIND`] is said on it with `confirm_within_s`,
/// and once the broker acks that, within the helper's `online_within_ms`,
/// the window starts here and the helper is told `{"online": true}`, which
/// starts its own. The app's confirmation, if it comes within the window,
/// goes to the helper as `{"confirm": true}`. The helper's last line is the
/// result: `wifi.set`'s `success` (kept) or `failure` (the lamp went back,
/// said after the link is made again over the old network).
pub fn ask_wifi(
    socket: &std::path::Path,
    line: &str,
    wait: Duration,
    ack_wait: Duration,
    link: &WifiLink,
    request_id: Option<String>,
    mut say: impl FnMut(&str, Result<Option<Value>, String>),
) {
    let unavailable = || Err("the lamp's settings are not running".to_owned());
    let Ok(mut stream) = UnixStream::connect(socket) else {
        return say(WIFI_KIND, unavailable());
    };
    if stream
        .set_read_timeout(Some(wait))
        .and_then(|()| stream.set_write_timeout(Some(wait)))
        .and_then(|()| stream.write_all(format!("{line}\n").as_bytes()))
        .is_err()
    {
        return say(WIFI_KIND, unavailable());
    }
    let Ok(mut writer) = stream.try_clone() else {
        return say(WIFI_KIND, unavailable());
    };
    let mut lines = BufReader::new(stream.take(8_192));
    let Some(first) = read_one(&mut lines) else {
        return say(
            WIFI_KIND,
            Err("the lamp's settings did not answer in time".to_owned()),
        );
    };
    let joined = serde_json::from_str::<Value>(first.trim())
        .ok()
        .and_then(|first| first.get("joined").cloned());
    // Not joined: the lamp never left its old network for long, and the
    // old link survived it (Mobile on lamp-0c89, #1959 21:2x). Most often:
    // the radio was switched and back, so a failure the broker does not ack
    // within a few seconds makes the link again (follow-up 3).
    let Some(joined) = joined else {
        let result = result(&first);
        if result.is_ok() {
            return say(WIFI_KIND, result);
        }
        let (events, heard) = std::sync::mpsc::channel::<Event>();
        link.await_ack(WIFI_KIND, events);
        say(WIFI_KIND, result);
        let acked = matches!(heard.recv_timeout(ack_wait), Ok(Event::Acked));
        link.stop_awaiting_ack();
        if !acked {
            link.want_relink();
        }
        return;
    };
    // After a join, the end: a failure means the lamp went back, off the
    // network the link was made again over, so it is made again once more;
    // a network kept leaves the link as it is.
    let end = |last: Option<String>| {
        let result = last.map_or_else(
            || Err("the lamp's settings did not answer in time".to_owned()),
            |last| result(&last),
        );
        if result.is_err() {
            link.want_relink();
        }
        result
    };
    let ms = |field: &str| joined.get(field).and_then(Value::as_u64);
    let (Some(online_ms), Some(confirm_ms), Some(ssid)) = (
        ms("online_within_ms"),
        ms("confirm_within_ms"),
        joined.get("ssid").cloned(),
    ) else {
        // Not this helper's line: dropped, so the helper goes back.
        drop(writer);
        return say(WIFI_KIND, end(read_one(&mut lines)));
    };
    // From here the helper's last line may come at any time (a cut, a
    // helper that gives up), so it is read on a thread of its own, and
    // each wait below also ends on it.
    let (events, heard) = std::sync::mpsc::channel::<Event>();
    let last_line = events.clone();
    std::thread::spawn(move || {
        let _ = last_line.send(Event::Last(read_one(&mut lines)));
    });
    link.await_ack(WIFI_JOINED_KIND, events.clone());
    link.want_relink();
    say(
        WIFI_JOINED_KIND,
        Ok(Some(
            json!({"ssid": ssid, "confirm_within_s": confirm_ms / 1_000}),
        )),
    );
    let mut last = None;
    let mut ended = false;
    // Until the broker acks `wifi.joined`, within the helper's bound less
    // the margin, so it ends before the helper's (follow-up 2).
    let online_by =
        Instant::now() + Duration::from_millis(online_ms).saturating_sub(WIFI_ONLINE_MARGIN);
    let acked = loop {
        match heard.recv_timeout(left(online_by)) {
            Ok(Event::Acked) => break true,
            Ok(Event::Last(line)) => {
                (last, ended) = (line, true);
                break false;
            }
            Ok(Event::Confirm) => {}
            Err(_) => break false,
        }
    };
    // A `wifi.joined` not said by now never is (follow-up 3).
    link.stop_awaiting_ack();
    if acked {
        // The window starts here, then at the helper: this one never ends
        // after the helper's.
        let by = Instant::now() + Duration::from_millis(confirm_ms);
        if let Ok(mut confirm) = link.confirm.lock() {
            *confirm = Some(Confirm::Waiting(events.clone(), by, request_id.clone()));
        }
        if writer.write_all(b"{\"online\":true}\n").is_ok() {
            let mut confirmed = false;
            while !confirmed && !ended {
                match heard.recv_timeout(left(by)) {
                    Ok(Event::Confirm) => confirmed = true,
                    Ok(Event::Last(line)) => (last, ended) = (line, true),
                    Ok(Event::Acked) => {}
                    Err(_) => break,
                }
            }
            if let Ok(mut confirm) = link.confirm.lock() {
                // A confirmation is taken under this lock, or not at all.
                while let Ok(event) = heard.try_recv() {
                    match event {
                        Event::Confirm => confirmed = true,
                        Event::Last(line) => (last, ended) = (line, true),
                        Event::Acked => {}
                    }
                }
                confirmed &= !ended;
                *confirm = confirmed.then(|| Confirm::Taken(by, request_id.clone()));
            }
            if confirmed {
                let _ = writer.write_all(b"{\"confirm\":true}\n");
            }
        }
    }
    drop(writer);
    while !ended {
        match heard.recv_timeout(wait) {
            Ok(Event::Last(line)) => (last, ended) = (line, true),
            Ok(_) => {}
            Err(_) => break,
        }
    }
    if let Ok(mut confirm) = link.confirm.lock() {
        *confirm = None;
    }
    say(WIFI_KIND, end(last));
}

/// What a joined Wi-Fi change's thread waits on.
#[derive(Debug)]
enum Event {
    /// The broker acked `wifi.joined`.
    Acked,
    /// The app's confirmation, within the window.
    Confirm,
    /// The helper's last line, or none (closed, or not in time).
    Last(Option<String>),
}

/// One line from the helper, or none: closed, or not in time.
fn read_one(lines: &mut impl BufRead) -> Option<String> {
    let mut line = String::new();
    lines.read_line(&mut line).ok().filter(|n| *n > 0)?;
    Some(line)
}

/// The keys helper over its socket, as [`ask`], with the failure's code:
/// the helper's own (`refused`, `unreached`, `bad_field`, `not_allowed`),
/// or `unavailable` when it isn't there or doesn't answer.
pub fn ask_keys(
    socket: &std::path::Path,
    line: &str,
    wait: Duration,
) -> (Result<Option<Value>, String>, Option<String>) {
    let unavailable = |why: &str| (Err(why.to_owned()), Some("unavailable".to_owned()));
    let Ok(mut stream) = UnixStream::connect(socket) else {
        return unavailable("the lamp's keys are not running");
    };
    if stream
        .set_read_timeout(Some(wait))
        .and_then(|()| stream.set_write_timeout(Some(wait)))
        .and_then(|()| stream.write_all(format!("{line}\n").as_bytes()))
        .is_err()
    {
        return unavailable("the lamp's keys are not running");
    }
    let mut reply = String::new();
    if BufReader::new(stream.take(4_096))
        .read_line(&mut reply)
        .is_err()
        || reply.is_empty()
    {
        return unavailable("the lamp's keys did not answer in time");
    }
    let Ok(reply) = serde_json::from_str::<Value>(reply.trim()) else {
        return unavailable("the lamp's keys did not answer in time");
    };
    if let Some(data) = reply.get("done") {
        return (Ok(Some(data.clone())), None);
    }
    let why = reply.get("refused").and_then(Value::as_str);
    let code = reply.get("code").and_then(Value::as_str);
    match (why, code) {
        (Some(why), Some(code)) => (Err(why.to_owned()), Some(code.to_owned())),
        _ => unavailable("the lamp's keys did not answer in time"),
    }
}

/// A change's result, waiting to be said.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub kind: String,
    pub result: Result<Option<Value>, String>,
    /// The app's own id for the request, said back in `data.request_id`.
    pub request_id: Option<String>,
    /// A failure's code, said in `data.code` (the keys' kinds, #2094).
    pub code: Option<String>,
}

impl Outcome {
    /// What the journal says of it: its kind, its status and the app's
    /// `request_id`, never its data (a network's name) or its error.
    pub fn journal(&self) -> String {
        let status = if self.result.is_ok() {
            "success"
        } else {
            "failure"
        };
        let id = self.request_id.as_deref().map_or_else(
            || "no request_id".to_owned(),
            |id| format!("request_id {id}"),
        );
        format!("a change's result, {} {status}, {id}", self.kind)
    }

    /// The reply for it, as `identity` says it: V1's `success` or `failure`,
    /// with the request's id in `data` if it had one.
    pub fn reply(&self, identity: &crate::wire::Identity, now: &str) -> Value {
        let mut reply = identity.data(now, &self.kind, &self.result);
        let added = [
            ("request_id", self.request_id.as_ref()),
            ("code", self.code.as_ref().filter(|_| self.result.is_err())),
        ];
        for (field, value) in added {
            let Some(value) = value else { continue };
            let data = reply
                .as_object_mut()
                .map(|reply| reply.entry("data").or_insert_with(|| json!({})));
            if let Some(Value::Object(data)) = data {
                data.insert(field.into(), json!(value));
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
    /// The keys helper's socket and wait, with `LAMP_OWN_KEYS=on` (#2094).
    pub keys: Option<(PathBuf, Duration)>,
    /// The broker the lamp is on now, `host:port`, for the Wi-Fi helper.
    broker: Mutex<String>,
    /// A Wi-Fi change is under way: a second is refused at once.
    wifi_busy: Arc<AtomicBool>,
    /// A Wi-Fi change's hold on the broker's link and the app's
    /// [`WIFI_CONFIRM_KIND`].
    wifi_link: Arc<WifiLink>,
    /// A keys request is under way: a second fails `busy` (#2094).
    keys_busy: Arc<AtomicBool>,
    /// [`PLACE_KINDS`] are taken, with `LAMP_OWNER_PLACE=on` (#4333).
    place: bool,
    done: Arc<Mutex<Vec<Outcome>>>,
    /// How long a failure with no join waits for the broker's ack before
    /// the link is made again: [`WIFI_RESULT_ACK_WAIT`], shorter only in
    /// this file's tests (#1806).
    result_ack_wait: Duration,
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
        Self::with_helpers(socket, wait, wifi, None)
    }

    /// With the Wi-Fi helper's and the keys helper's sockets and waits, or none.
    pub fn with_helpers(
        socket: impl Into<PathBuf>,
        wait: Duration,
        wifi: Option<(PathBuf, Duration)>,
        keys: Option<(PathBuf, Duration)>,
    ) -> Arc<Self> {
        Arc::new(Self {
            socket: socket.into(),
            wait,
            wifi,
            keys,
            broker: Mutex::default(),
            wifi_busy: Arc::default(),
            wifi_link: Arc::default(),
            keys_busy: Arc::default(),
            place: false,
            done: Arc::default(),
            result_ack_wait: WIFI_RESULT_ACK_WAIT,
        })
    }

    /// [`Changes::with_wifi`], a failure's ack waited for only `ack_wait`.
    #[cfg(test)]
    fn with_wifi_result_ack_wait(
        socket: impl Into<PathBuf>,
        wait: Duration,
        wifi: Option<(PathBuf, Duration)>,
        ack_wait: Duration,
    ) -> Arc<Self> {
        let mut changes = Self::with_wifi(socket, wait, wifi);
        Arc::get_mut(&mut changes)
            .expect("just made")
            .result_ack_wait = ack_wait;
        changes
    }

    /// The product's: the helpers' sockets, the Wi-Fi's only when `wifi`,
    /// the keys' only when `keys`.
    pub fn product(wifi: bool, keys: bool) -> Arc<Self> {
        Self::with_helpers(
            SOCKET,
            WAIT,
            wifi.then(|| (PathBuf::from(WIFI_SOCKET), WIFI_WAIT)),
            keys.then(|| (PathBuf::from(KEYS_SOCKET), KEYS_WAIT)),
        )
    }

    /// The same, taking [`PLACE_KINDS`] too when `on`. Only on changes just
    /// made, before they are shared; shared ones are as they were.
    pub fn taking_the_place(mut self: Arc<Self>, on: bool) -> Arc<Self> {
        if let Some(changes) = Arc::get_mut(&mut self) {
            changes.place = on;
        }
        self
    }

    /// Whether [`PLACE_KINDS`] are taken.
    pub fn place_on(&self) -> bool {
        self.place
    }

    /// Whether [`KEYS_KINDS`] are taken.
    pub fn keys_on(&self) -> bool {
        self.keys.is_some()
    }

    /// Asks the keys helper for the app's `data` on a thread of its own.
    /// Only a lamp without the switch is refused at once ("not on this
    /// lamp"); every other request is acked `starting` and its result
    /// follows, a failure with its code (#2094, the contract on #2094 at
    /// 11:1x): `bad_field` and `busy` at once, the helper's after. The key
    /// goes into the helper's line, nowhere else.
    pub fn start_keys(&self, kind: &str, data: &Value) -> Result<(), String> {
        let (socket, wait) = self
            .keys
            .clone()
            .ok_or_else(|| crate::wire::not_here(kind))?;
        let request_id = request_id(data).ok().flatten();
        let fail = |why: String, code: &str| {
            if let Ok(mut done) = self.done.lock() {
                done.push(Outcome {
                    kind: kind.to_owned(),
                    result: Err(why),
                    request_id: request_id.clone(),
                    code: Some(code.into()),
                });
            }
        };
        let line = match keys_line(kind, data) {
            Ok((line, _)) => line,
            Err(why) => {
                fail(why, "bad_field");
                return Ok(());
            }
        };
        if self.keys_busy.swap(true, Ordering::AcqRel) {
            fail("a key change is already under way".into(), "busy");
            return Ok(());
        }
        let (done, busy, kind) = (self.done.clone(), self.keys_busy.clone(), kind.to_owned());
        std::thread::spawn(move || {
            let (result, code) = ask_keys(&socket, &line, wait);
            busy.store(false, Ordering::Release);
            if let Ok(mut done) = done.lock() {
                done.push(Outcome {
                    kind,
                    result,
                    request_id,
                    code,
                });
            }
        });
        Ok(())
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
        let link = self.wifi_link.clone();
        let ack_wait = self.result_ack_wait;
        std::thread::spawn(move || {
            let say = |kind: &str, result| {
                if let Ok(mut done) = done.lock() {
                    done.push(Outcome {
                        kind: kind.into(),
                        result,
                        request_id: request_id.clone(),
                        code: None,
                    });
                }
            };
            ask_wifi(
                &socket,
                &line,
                wait,
                ack_wait,
                &link,
                request_id.clone(),
                say,
            );
            busy.store(false, Ordering::Release);
        });
        Ok(())
    }

    /// The broker's loop: whether a Wi-Fi change wants the link dropped and
    /// made again now. Once per want; the loop then makes one try at once
    /// and backs off from its first wait.
    pub fn take_relink(&self) -> bool {
        let Ok(mut relink) = self.wifi_link.relink.lock() else {
            return false;
        };
        if !matches!(*relink, Relink::Wanted) {
            return false;
        }
        *relink = Relink::Making;
        true
    }

    /// The broker's loop: woken when a Wi-Fi change wants the link made
    /// again (it may wake for nothing: [`Self::take_relink`] says).
    pub async fn relink_asked(&self) {
        self.wifi_link.asked.notified().await;
    }

    /// The broker's loop, at each CONNACK: a link made after a relink is
    /// over the network the lamp is on now, and the Wi-Fi's results held
    /// for it go out on it.
    pub fn online(&self) {
        if let Ok(mut relink) = self.wifi_link.relink.lock()
            && matches!(*relink, Relink::Making)
        {
            *relink = Relink::Idle;
        }
    }

    /// The broker's loop, at a PUBACK of a change's result: once it is
    /// [`WIFI_JOINED_KIND`]'s, the confirmation window starts; a
    /// [`WIFI_KIND`] result's, the change waiting on it knows its link held.
    pub fn acked(&self, outcome: &Outcome) {
        let waiting = self.wifi_link.awaiting_ack.lock().ok().and_then(|mut w| {
            if w.as_ref().is_some_and(|(kind, _)| *kind == outcome.kind) {
                w.take()
            } else {
                None
            }
        });
        if let Some((_, send)) = waiting {
            let _ = send.send(Event::Acked);
        }
    }

    /// The app's [`WIFI_CONFIRM_KIND`]: handed to the change waiting for it
    /// within its window, and `success` again for a second one within it;
    /// or why not, after it as before it. A `request_id` in `data` must be
    /// the change's (follow-up 5); with none, it is taken as the change's,
    /// and one is taken for a change that had none, with nothing to differ
    /// from (#2686's P3 5).
    pub fn confirm_wifi(&self, data: &Value) -> Result<(), String> {
        let refused = || "no Wi-Fi change waits for a confirmation".to_owned();
        let asked = request_id(data)?;
        let mut confirm = self.wifi_link.confirm.lock().map_err(|_| refused())?;
        let other = |id: &Option<String>| asked.is_some() && id.is_some() && asked != *id;
        let not_this = || "this wifi.confirm's request_id is not the Wi-Fi change's".to_owned();
        match confirm.take() {
            Some(Confirm::Waiting(send, by, id)) if Instant::now() < by => {
                if other(&id) {
                    *confirm = Some(Confirm::Waiting(send, by, id));
                    return Err(not_this());
                }
                // Sent under the lock the change's thread takes to stop
                // waiting: handed on, or refused, never both.
                if send.send(Event::Confirm).is_err() {
                    return Err(refused());
                }
                *confirm = Some(Confirm::Taken(by, id));
                Ok(())
            }
            Some(Confirm::Taken(by, id)) if Instant::now() < by => {
                let mine = !other(&id);
                *confirm = Some(Confirm::Taken(by, id));
                mine.then_some(()).ok_or_else(not_this)
            }
            _ => Err(refused()),
        }
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
                    code: None,
                });
            }
        });
    }

    /// The results come since the last call, oldest first. A Wi-Fi
    /// change's are held while its relink is wanted or being made: they go
    /// out only on the link made after it.
    pub fn take(&self) -> Vec<Outcome> {
        let Ok(mut done) = self.done.lock() else {
            return Vec::new();
        };
        // A `wifi.joined` whose wait for its ack has ended is never said:
        // the change went on without it (follow-up 3).
        if !self.wifi_link.joined_wanted() {
            done.retain(|outcome| outcome.kind != WIFI_JOINED_KIND);
        }
        if self.wifi_link.settled() {
            return std::mem::take(&mut *done);
        }
        let (wifi, rest) = std::mem::take(&mut *done)
            .into_iter()
            .partition(|outcome| [WIFI_KIND, WIFI_JOINED_KIND].contains(&outcome.kind.as_str()));
        *done = wifi;
        rest
    }
}

/// The languages the voice speaks (#4473), as setup writes them.
pub const LANGUAGES: [&str; 5] = ["en", "vi", "ja", "zh", "ko"];

/// `system.info`'s `host.language` from `text` (setup's file): its code,
/// else `en`, the voice's own reading of no file or one it doesn't know.
pub fn language(text: &str) -> &'static str {
    let code = text.lines().next().unwrap_or("").trim();
    LANGUAGES
        .into_iter()
        .find(|known| *known == code)
        .unwrap_or("en")
}

/// The lamp's name in the app from `text` (the helper's file), if it holds one.
pub fn display_name(text: &str) -> Option<String> {
    let name = text.lines().next()?.trim();
    (!name.is_empty() && name.chars().count() <= 64 && !name.chars().any(char::is_control))
        .then(|| name.to_owned())
}

#[cfg(test)]
mod tests {

    /// #2094, the contract on #2094 (11:1x): every keys request but one on
    /// a lamp without the switch gets a result, a failure with its code
    /// (`bad_field`, `busy`, `unavailable`) or the helper's answer, and the
    /// app's `request_id` back on `keys.remove` and `keys.get` too.
    #[test]
    fn every_keys_request_gets_a_coded_result_with_its_id() {
        let wait_for = |changes: &Changes, count: usize| {
            let started = Instant::now();
            let mut got = Vec::new();
            while got.len() < count && started.elapsed() < Duration::from_secs(5) {
                got.extend(changes.take());
                std::thread::sleep(Duration::from_millis(10));
            }
            got
        };
        let identity = identity();
        // Nothing listening: unavailable.
        let gone = std::env::temp_dir().join(format!("lco-{}-keys-gone.sock", std::process::id()));
        let _ = std::fs::remove_file(&gone);
        let changes = Changes::with_helpers(
            "/nowhere.sock",
            Duration::from_secs(1),
            None,
            Some((gone, Duration::from_secs(1))),
        );
        changes
            .start_keys("keys.get", &json!({"request_id": "g-1"}))
            .unwrap();
        let got = wait_for(&changes, 1);
        let reply = got[0].reply(&identity, "t");
        assert_eq!(reply["status"], "failure");
        assert_eq!(reply["error"], "the lamp's keys are not running");
        assert_eq!(
            reply["data"],
            json!({"request_id": "g-1", "code": "unavailable"})
        );
        // A bad field: a result at once, with no helper asked.
        changes
            .start_keys(
                "keys.set",
                &json!({"service": "openai", "key": "sk-planted", "request_id": "s-1"}),
            )
            .unwrap();
        let reply = changes.take()[0].reply(&identity, "t");
        assert_eq!(reply["error"], "service must be deepseek or deepgram");
        assert_eq!(
            reply["data"],
            json!({"request_id": "s-1", "code": "bad_field"})
        );
        assert!(!reply.to_string().contains("sk-planted"));
        // A helper that answers: remove and get say the id back on success,
        // with no code; a second request while one is under way is busy.
        let socket = std::env::temp_dir().join(format!("lco-{}-keys-ok.sock", std::process::id()));
        let _ = std::fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket).unwrap();
        let (release, held) = std::sync::mpsc::channel::<()>();
        let helper = std::thread::spawn(move || {
            use std::io::{BufRead, Write};
            for (at, answer) in [
                r#"{"done":{"service":"deepgram","uses":"lamp"}}"#,
                r#"{"done":{"deepseek":"lamp","deepgram":"lamp","since":{"deepseek":null,"deepgram":null},"deepseek_host":null}}"#,
            ]
            .into_iter()
            .enumerate()
            {
                let (mut stream, _) = listener.accept().unwrap();
                let mut line = String::new();
                std::io::BufReader::new(stream.try_clone().unwrap())
                    .read_line(&mut line)
                    .unwrap();
                if at == 0 {
                    held.recv().unwrap();
                }
                stream.write_all(format!("{answer}\n").as_bytes()).unwrap();
            }
        });
        let changes = Changes::with_helpers(
            "/nowhere.sock",
            Duration::from_secs(1),
            None,
            Some((socket.clone(), Duration::from_secs(5))),
        );
        changes
            .start_keys(
                "keys.remove",
                &json!({"service": "deepgram", "request_id": "r-1"}),
            )
            .unwrap();
        changes
            .start_keys("keys.get", &json!({"request_id": "busy-1"}))
            .unwrap();
        let busy = changes.take();
        assert_eq!(busy.len(), 1);
        let reply = busy[0].reply(&identity, "t");
        assert_eq!(reply["error"], "a key change is already under way");
        assert_eq!(
            reply["data"],
            json!({"request_id": "busy-1", "code": "busy"})
        );
        release.send(()).unwrap();
        let removed = wait_for(&changes, 1)[0].reply(&identity, "t");
        assert_eq!(removed["status"], "success");
        assert_eq!(
            removed["data"],
            json!({"service": "deepgram", "uses": "lamp", "request_id": "r-1"})
        );
        changes
            .start_keys("keys.get", &json!({"request_id": "g-2"}))
            .unwrap();
        let got = wait_for(&changes, 1)[0].reply(&identity, "t");
        assert_eq!(got["status"], "success");
        assert_eq!(got["data"]["request_id"], "g-2");
        assert_eq!(got["data"]["deepseek"], "lamp");
        helper.join().unwrap();
        let _ = std::fs::remove_file(&socket);
    }

    /// #2094: the keys' kinds only with all three switches; their lines
    /// carry the key to the helper alone, and no refusal holds one.
    #[test]
    fn own_keys_need_their_switch_and_their_lines_hold_only_what_the_helper_reads() {
        assert!(keys_gates(Some("on"), Some("on"), Some("on")));
        for (cloud, change, keys) in [
            (Some("on"), Some("on"), None),
            (Some("on"), None, Some("on")),
            (None, Some("on"), Some("on")),
            (Some("on"), Some("on"), Some("1")),
        ] {
            assert!(!keys_gates(cloud, change, keys));
        }
        let off = Changes::new("/nowhere.sock", Duration::from_secs(1));
        assert!(!off.keys_on());
        assert_eq!(
            off.start_keys(
                "keys.set",
                &json!({"service": "deepseek", "key": "sk-planted"})
            ),
            Err("not on this lamp: keys.set".into())
        );
        let (line, id) = keys_line(
            "keys.set",
            &json!({"service": "deepseek", "key": "sk-planted", "base_url": "https://x.invalid", "request_id": "r1"}),
        )
        .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&line).unwrap(),
            json!({"set": "deepseek", "key": "sk-planted", "base_url": "https://x.invalid"})
        );
        assert_eq!(id.as_deref(), Some("r1"));
        assert_eq!(
            keys_line("keys.remove", &json!({"service": "deepgram"}))
                .unwrap()
                .0,
            r#"{"remove":"deepgram"}"#
        );
        assert_eq!(
            keys_line("keys.get", &json!({})).unwrap().0,
            r#"{"which":true}"#
        );
        for (kind, data) in [
            (
                "keys.set",
                json!({"service": "openai", "key": "sk-planted"}),
            ),
            ("keys.set", json!({"service": "deepseek", "key": 7})),
            (
                "keys.set",
                json!({"service": "deepseek", "key": "sk-planted", "base_url": 1}),
            ),
            ("keys.remove", json!({})),
        ] {
            let why = keys_line(kind, &data).unwrap_err();
            assert!(!why.contains("sk-planted"), "{why}");
        }
    }
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

    /// Waits up to 5 s for the first result, making the link again when a
    /// Wi-Fi change wants it, as the broker's loop does.
    fn first(changes: &Changes) -> (String, Result<Option<Value>, String>) {
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            if changes.take_relink() {
                changes.online();
            }
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
        // #4473: the language goes as given; root's helper checks it.
        assert_eq!(
            line("language.set", &json!({"language": "ja"})),
            Ok(r#"{"language":"ja"}"#.to_owned())
        );
        assert_eq!(
            line("language.set", &json!({"language": 1})),
            Err("language must be text".to_owned())
        );
    }

    /// #4473: `host.language` is the file's code, else `en`, the voice's
    /// own reading; the codes are setup's.
    #[test]
    fn the_language_reads_as_its_code_else_english() {
        assert_eq!(LANGUAGES, lamp_setup::serve::LANGUAGES);
        for code in LANGUAGES {
            assert_eq!(language(&format!("{code}\n")), code);
        }
        for other in ["", "\n", "fr", "JA", "ja-JP"] {
            assert_eq!(language(other), "en", "{other:?}");
        }
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

    /// #4333: the owner's place needs its own switch beside the names'.
    #[test]
    fn the_owners_place_needs_its_own_switch_beside_the_names() {
        assert!(place_gates(Some("on"), Some("on"), Some("on")));
        for (cloud, change, place) in [
            (Some("on"), Some("on"), None),
            (Some("on"), None, Some("on")),
            (None, Some("on"), Some("on")),
            (Some("on"), Some("on"), Some("1")),
        ] {
            assert!(
                !place_gates(cloud, change, place),
                "{cloud:?} {change:?} {place:?}"
            );
        }
    }

    /// #4333, with the switch off (the names' switch on or not): setting
    /// the place is a kind this lamp lacks, as before, `system.info` lists
    /// neither kind, and nothing reaches the helper. Without the helper the
    /// clear is refused the same way.
    #[test]
    fn off_the_owners_place_is_refused_as_a_kind_this_lamp_lacks() {
        let data = json!({"name": "Ba Dinh", "lat": 21.03, "lon": 105.83, "set_at": "2026-10-29T08:30:00Z"});
        for changes in [
            None,
            Some(Changes::new(socket("place-never"), WAIT)),
            Some(Changes::new(socket("place-never"), WAIT).taking_the_place(false)),
        ] {
            let mut lamp = Fake {
                changes: changes.clone(),
                ..Fake::default()
            };
            let kinds: &[&str] = if changes.is_some() {
                &["owner.place"]
            } else {
                &PLACE_KINDS
            };
            for &kind in kinds {
                assert_eq!(
                    answer(&mut lamp, kind, &data),
                    Err(format!("not on this lamp: {kind}"))
                );
                let sent = format!(r#"{{"cmd":"data","kind":"{kind}","data":{data}}}"#);
                let replied: Value = serde_json::from_str(
                    &reply(&identity(), &mut lamp, sent.as_bytes(), "t").unwrap(),
                )
                .unwrap();
                assert_eq!(replied["status"], "failure");
            }
            let info = answer(&mut lamp, "system.info", &Value::Null)
                .unwrap()
                .unwrap();
            assert!(!info["can"].to_string().contains("owner.place"), "{info}");
            if let Some(changes) = changes {
                std::thread::sleep(Duration::from_millis(50));
                assert!(changes.take().is_empty());
            }
        }
    }

    /// #4356: with the switch off, a place kept while it was on can still
    /// be cleared from the app: acked at once, `null` to the helper, and its
    /// result after, though `can` never lists it.
    #[test]
    fn off_the_owners_place_can_still_be_cleared_unlisted() {
        let path = socket("place-off-clear");
        let serving = helper(&path, Duration::ZERO, "{\"done\":{\"cleared\":true}}\n");
        let changes = Changes::new(&path, Duration::from_secs(5));
        assert!(!changes.place_on());
        let mut lamp = Fake {
            changes: Some(changes.clone()),
            ..Fake::default()
        };
        let info = answer(&mut lamp, "system.info", &Value::Null)
            .unwrap()
            .unwrap();
        assert!(!info["can"].to_string().contains("owner.place"), "{info}");
        let sent = br#"{"cmd":"data","kind":"owner.place.clear","data":{}}"#;
        let acked: Value =
            serde_json::from_str(&reply(&identity(), &mut lamp, sent, "t").unwrap()).unwrap();
        assert_eq!(
            (&acked["kind"], &acked["status"]),
            (&json!("owner.place.clear"), &json!("starting"))
        );
        let (kind, result) = first(&changes);
        assert_eq!(
            (kind.as_str(), result),
            ("owner.place.clear", Ok(Some(json!({"cleared": true}))))
        );
        assert_eq!(serving.join().unwrap(), "{\"owner_place\":null}\n");
        let _ = std::fs::remove_file(&path);
    }

    /// #4333: what leaves lamp-cloud is already on the grid, never finer
    /// than the phone's; a clear is `null`; no refusal holds a value.
    #[test]
    fn the_owners_place_is_rounded_before_it_leaves_and_a_clear_is_null() {
        assert_eq!(
            place_line(
                "owner.place",
                &json!({"name": " Ba Dinh ", "lat": 21.035_123, "lon": 105.834_987,
                        "set_at": "2026-10-29T08:30:00Z", "street": "12 Nowhere Street"})
            ),
            Ok(
                r#"{"owner_place":{"lat":21.04,"lon":105.83,"name":"Ba Dinh","set_at":"2026-10-29T08:30:00Z"}}"#
                    .to_owned()
            )
        );
        assert_eq!(
            place_line("owner.place.clear", &json!({})),
            Ok(r#"{"owner_place":null}"#.to_owned())
        );
        for data in [
            json!({"name": "Secret Lane", "lat": 91, "lon": 105.8, "set_at": "2026-10-29T08:30:00Z"}),
            json!({"name": "Secret Lane", "lat": 21.0, "lon": "105.8", "set_at": "2026-10-29T08:30:00Z"}),
            json!({"name": "Secret\nLane", "lat": 21.0, "lon": 105.8, "set_at": "2026-10-29T08:30:00Z"}),
        ] {
            let why = place_line("owner.place", &data).unwrap_err();
            assert!(!why.contains("Secret") && !why.contains("105"), "{why}");
        }
    }

    /// #4333: set and clear, acked at once and their result after, as the
    /// names are; the helper takes the rounded place, and the result says
    /// what was kept.
    #[test]
    fn the_owners_place_is_acked_at_once_and_its_result_follows() {
        let path = socket("place");
        let serving = helper(
            &path,
            Duration::from_millis(100),
            "{\"done\":{\"name\":\"Ba Dinh\",\"lat\":21.04,\"lon\":105.83,\"set_at\":\"2026-10-29T08:30:00Z\"}}\n",
        );
        let changes = Changes::new(&path, Duration::from_secs(5)).taking_the_place(true);
        assert!(changes.place_on());
        let mut lamp = Fake {
            changes: Some(changes.clone()),
            ..Fake::default()
        };
        let info = answer(&mut lamp, "system.info", &Value::Null)
            .unwrap()
            .unwrap();
        for kind in PLACE_KINDS {
            assert!(info["can"].as_array().unwrap().contains(&json!(kind)));
        }
        let sent = br#"{"cmd":"data","kind":"owner.place","data":{"name":"Ba Dinh","lat":21.035123,"lon":105.834987,"set_at":"2026-10-29T08:30:00Z"}}"#;
        let acked: Value =
            serde_json::from_str(&reply(&identity(), &mut lamp, sent, "t").unwrap()).unwrap();
        assert_eq!(
            (&acked["kind"], &acked["status"]),
            (&json!("owner.place"), &json!("starting"))
        );
        let (kind, result) = first(&changes);
        assert_eq!(kind, "owner.place");
        let said = identity().data("t", &kind, &result);
        assert_eq!(said["status"], "success");
        assert_eq!(said["data"]["lat"], 21.04);
        let took: Value = serde_json::from_str(&serving.join().unwrap()).unwrap();
        assert_eq!(
            took,
            json!({"owner_place": {"name": "Ba Dinh", "lat": 21.04, "lon": 105.83, "set_at": "2026-10-29T08:30:00Z"}})
        );
        let _ = std::fs::remove_file(&path);
        let serving = helper(&path, Duration::ZERO, "{\"done\":{\"cleared\":true}}\n");
        let sent = br#"{"cmd":"data","kind":"owner.place.clear","data":{}}"#;
        let acked: Value =
            serde_json::from_str(&reply(&identity(), &mut lamp, sent, "t").unwrap()).unwrap();
        assert_eq!(acked["status"], "starting");
        let (kind, result) = first(&changes);
        assert_eq!(
            (kind.as_str(), result),
            ("owner.place.clear", Ok(Some(json!({"cleared": true}))))
        );
        assert_eq!(serving.join().unwrap(), "{\"owner_place\":null}\n");
        // A place that doesn't fit fails at once, and nothing is asked.
        let sent = br#"{"cmd":"data","kind":"owner.place","data":{"name":"Ba Dinh","lat":95,"lon":105.8,"set_at":"2026-10-29T08:30:00Z"}}"#;
        let failed: Value =
            serde_json::from_str(&reply(&identity(), &mut lamp, sent, "t").unwrap()).unwrap();
        assert_eq!(
            (&failed["status"], &failed["error"]),
            (
                &json!("failure"),
                &json!("lat must be a number from -90 to 90")
            )
        );
        let _ = std::fs::remove_file(&path);
    }

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
            if changes.take_relink() {
                changes.online();
            }
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
            code: None,
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

    /// A Wi-Fi helper on `path` that proves the new network, waits for
    /// `lamp-cloud`'s word that the app was told, opens its window of
    /// `window` only then, and keeps the network only with a confirmation
    /// within it, as `lamp-setup wifi` does; it hands back the lines it took
    /// after its request, and how long after its window opened each came.
    fn wifi_helper(
        path: &std::path::Path,
        window: Duration,
    ) -> std::thread::JoinHandle<Vec<String>> {
        let listener = UnixListener::bind(path).unwrap();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let reader = BufReader::new(stream.try_clone().unwrap());
            let mut lines = reader.lines();
            let _request = lines.next().unwrap().unwrap();
            let mut stream = stream;
            let joined = json!({"joined": {"ssid": "New Home", "online_within_ms": 5_000,
                "confirm_within_ms": window.as_millis() as u64}});
            stream.write_all(format!("{joined}\n").as_bytes()).unwrap();
            let (send, heard) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                for line in lines {
                    let Ok(line) = line else { return };
                    if send.send(line).is_err() {
                        return;
                    }
                }
            });
            let online = heard.recv().unwrap();
            let deadline = Instant::now() + window;
            let mut took = vec![online];
            while let Ok(line) =
                heard.recv_timeout(deadline.saturating_duration_since(Instant::now()))
            {
                took.push(line);
            }
            let last: &[u8] = if took.iter().any(|line| line == r#"{"confirm":true}"#) {
                b"{\"done\":{\"ssid\":\"New Home\"}}\n"
            } else {
                b"{\"refused\":\"the app did not confirm the new Wi-Fi within 120 s; the lamp is back on its old Wi-Fi\"}\n"
            };
            stream.write_all(last).unwrap();
            took
        })
    }

    /// Waits up to 5 s for the change to want its link made again, as the
    /// broker's loop does, and says it is up again.
    fn link_back(changes: &Changes) {
        let until = Instant::now() + Duration::from_secs(5);
        while !changes.take_relink() {
            assert!(Instant::now() < until, "no relink wanted");
            std::thread::sleep(Duration::from_millis(10));
        }
        changes.online();
    }

    /// Waits up to 5 s for the next result.
    fn next(changes: &Changes) -> Outcome {
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(done) = changes.take().into_iter().next() {
                return done;
            }
            assert!(Instant::now() < until, "no result");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// The reviewer-lead's P2 and P3 and the first builder's P2-1 and P2-2
    /// on #2617. A proven new network is said `wifi.joined` only once the
    /// link is made again, with the helper's own window; the window starts
    /// when the broker acks that, not at the join; a confirmation 1 s before
    /// its end is handed to the helper once and answered `success`, a second
    /// one too, and `wifi.set`'s `success` follows once kept, with no
    /// `failure` and no other relink.
    #[test]
    fn p3_a_confirm_1_s_before_the_helpers_deadline_is_success_only() {
        let path = socket("joined");
        let serving = wifi_helper(&path, Duration::from_secs(2));
        let changes = Changes::with_wifi(socket("names"), WAIT, Some((path.clone(), WIFI_WAIT)));
        assert_eq!(
            changes.confirm_wifi(&json!({})),
            Err("no Wi-Fi change waits for a confirmation".to_owned())
        );
        changes.set_broker("broker.test", 1883);
        let data = json!({"ssid": "New Home", "password": FAKE_PASSWORD, "request_id": "r-8"});
        changes.start_wifi(&data).unwrap();
        // Joined: nothing said on the old link, which is dropped first.
        std::thread::sleep(Duration::from_millis(200));
        assert!(changes.take().is_empty());
        link_back(&changes);
        let joined = next(&changes);
        let reply = joined.reply(&identity(), "t");
        assert_eq!(
            (&reply["kind"], &reply["status"]),
            (&json!(WIFI_JOINED_KIND), &json!("success"))
        );
        // The helper's window, not a constant of lamp-cloud's.
        assert_eq!(
            reply["data"],
            json!({"ssid": "New Home", "confirm_within_s": 2, "request_id": "r-8"})
        );
        // The broker's ack is late: the window has not started.
        std::thread::sleep(Duration::from_millis(1_500));
        changes.acked(&joined);
        let acked = Instant::now();
        std::thread::sleep(Duration::from_secs(1));
        assert!(acked.elapsed() < Duration::from_millis(1_500));
        assert_eq!(changes.confirm_wifi(&json!({})), Ok(()));
        assert_eq!(changes.confirm_wifi(&json!({})), Ok(()), "the app's resend");
        assert_eq!(
            serving.join().unwrap(),
            [r#"{"online":true}"#, r#"{"confirm":true}"#]
        );
        let kept = next(&changes).reply(&identity(), "t");
        assert_eq!(
            (&kept["kind"], &kept["status"], &kept["data"]),
            (
                &json!(WIFI_KIND),
                &json!("success"),
                &json!({"ssid": "New Home", "request_id": "r-8"})
            )
        );
        let until = Instant::now() + Duration::from_secs(5);
        while changes.wifi_busy.load(Ordering::Acquire) {
            assert!(Instant::now() < until, "still busy");
            std::thread::sleep(Duration::from_millis(10));
        }
        // Kept: no failure follows, and the link stays.
        assert!(changes.take().is_empty());
        assert!(!changes.take_relink());
        assert_eq!(
            changes.confirm_wifi(&json!({})),
            Err("no Wi-Fi change waits for a confirmation".to_owned())
        );
        let _ = std::fs::remove_file(&path);
    }

    /// The reviewer-lead's P3 on #2617: a confirmation 1 s after the
    /// helper's window is refused, never handed on, and the lamp's going
    /// back is said after the link is made again, over the old network.
    #[test]
    fn p3_a_confirm_1_s_after_the_helpers_deadline_is_failure_only() {
        let path = socket("joined-late");
        let serving = wifi_helper(&path, Duration::from_secs(1));
        let changes = Changes::with_wifi(socket("names"), WAIT, Some((path.clone(), WIFI_WAIT)));
        changes.set_broker("broker.test", 1883);
        let data = json!({"ssid": "New Home", "password": FAKE_PASSWORD, "request_id": "r-9"});
        changes.start_wifi(&data).unwrap();
        link_back(&changes);
        let joined = next(&changes);
        assert_eq!(joined.kind, WIFI_JOINED_KIND);
        changes.acked(&joined);
        std::thread::sleep(Duration::from_secs(2));
        assert_eq!(
            changes.confirm_wifi(&json!({})),
            Err("no Wi-Fi change waits for a confirmation".to_owned())
        );
        assert_eq!(serving.join().unwrap(), [r#"{"online":true}"#]);
        // The way back changed the network under the link: it is made again
        // before the failure is said.
        std::thread::sleep(Duration::from_millis(200));
        assert!(changes.take().is_empty());
        link_back(&changes);
        let failed = next(&changes);
        assert_eq!(failed.kind, WIFI_KIND);
        assert_eq!(
            failed.result,
            Err("the app did not confirm the new Wi-Fi within 120 s; the lamp is back on its old Wi-Fi".to_owned())
        );
        let _ = std::fs::remove_file(&path);
    }

    /// Mobile on lamp-0c89 (#1959, 21:2x): a change that never joined (a
    /// wrong password) left the old link alive, so its failure is said on
    /// it, with no relink.
    #[test]
    fn a_failure_with_no_join_is_said_on_the_link_there_is() {
        let path = socket("wrong-key");
        let serving = helper(
            &path,
            Duration::ZERO,
            "{\"refused\":\"wrong WiFi password: the router rejected the key; the lamp is back on its old Wi-Fi\"}\n",
        );
        let changes = Changes::with_wifi(socket("names"), WAIT, Some((path.clone(), WIFI_WAIT)));
        changes.set_broker("broker.test", 1883);
        changes
            .start_wifi(&json!({"ssid": "New Home", "password": FAKE_PASSWORD}))
            .unwrap();
        let failed = next(&changes);
        assert!(failed.result.is_err());
        assert!(!changes.take_relink());
        serving.join().unwrap();
        let _ = std::fs::remove_file(&path);
    }

    /// A Wi-Fi helper on `path` that says `joined` with `online_ms` and
    /// `confirm_ms`, takes what `lamp-cloud` says for `listen`, then answers
    /// `last`; it hands back the lines it took.
    fn joined_helper(
        path: &std::path::Path,
        online_ms: u64,
        confirm_ms: u64,
        listen: Duration,
        last: &'static str,
    ) -> std::thread::JoinHandle<Vec<String>> {
        let listener = UnixListener::bind(path).unwrap();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let reader = BufReader::new(stream.try_clone().unwrap());
            let mut lines = reader.lines();
            let _request = lines.next().unwrap().unwrap();
            let mut stream = stream;
            let joined = json!({"joined": {"ssid": "New Home", "online_within_ms": online_ms,
                "confirm_within_ms": confirm_ms}});
            stream.write_all(format!("{joined}\n").as_bytes()).unwrap();
            let (send, heard) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                for line in lines {
                    let Ok(line) = line else { return };
                    if send.send(line).is_err() {
                        return;
                    }
                }
            });
            let deadline = Instant::now() + listen;
            let mut took = Vec::new();
            while let Ok(line) =
                heard.recv_timeout(deadline.saturating_duration_since(Instant::now()))
            {
                took.push(line);
            }
            stream.write_all(last.as_bytes()).unwrap();
            took
        })
    }

    const NOT_CONFIRMED: &str = "{\"refused\":\"the app did not confirm the new Wi-Fi within 120 s; the lamp is back on its old Wi-Fi\"}\n";

    /// Waits up to 5 s for the change to end.
    fn ended(changes: &Changes) {
        let until = Instant::now() + Duration::from_secs(5);
        while changes.wifi_busy.load(Ordering::Acquire) {
            assert!(Instant::now() < until, "still busy");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// The first builder's follow-up 2 on #2617: lamp-cloud's wait for the
    /// broker's ack of `wifi.joined` ends before the helper's, which started
    /// first: an ack after it is not told the helper as `online`.
    #[test]
    fn followup_2_lamp_clouds_online_deadline_ends_before_the_helpers() {
        let path = socket("online-late");
        let serving = joined_helper(&path, 1_500, 1_000, Duration::from_secs(2), NOT_CONFIRMED);
        let changes = Changes::with_wifi(socket("names"), WAIT, Some((path.clone(), WIFI_WAIT)));
        changes.set_broker("broker.test", 1883);
        changes
            .start_wifi(&json!({"ssid": "New Home", "password": FAKE_PASSWORD}))
            .unwrap();
        link_back(&changes);
        let joined = next(&changes);
        // Inside the helper's 1.5 s, past lamp-cloud's own, the margin less.
        std::thread::sleep(
            Duration::from_millis(1_500).saturating_sub(WIFI_ONLINE_MARGIN)
                + Duration::from_millis(300),
        );
        changes.acked(&joined);
        assert_eq!(serving.join().unwrap(), Vec::<String>::new());
        let _ = std::fs::remove_file(&path);
    }

    /// Follow-up 3: a `wifi.joined` the broker never acked is not said once
    /// the change has gone on without it; only `wifi.set`'s failure is.
    #[test]
    fn followup_3_a_joined_never_acked_is_dropped_once_its_wait_ended() {
        let path = socket("joined-dropped");
        let serving = joined_helper(&path, 300, 1_000, Duration::from_millis(600), NOT_CONFIRMED);
        let changes = Changes::with_wifi(socket("names"), WAIT, Some((path.clone(), WIFI_WAIT)));
        changes.set_broker("broker.test", 1883);
        changes
            .start_wifi(&json!({"ssid": "New Home", "password": FAKE_PASSWORD}))
            .unwrap();
        // The link is not back before the change ends.
        ended(&changes);
        assert_eq!(serving.join().unwrap(), Vec::<String>::new());
        link_back(&changes);
        let said: Vec<String> = changes.take().into_iter().map(|done| done.kind).collect();
        assert_eq!(said, [WIFI_KIND]);
        let _ = std::fs::remove_file(&path);
    }

    /// Follow-up 3: a failure with no join is said on the link there is, but
    /// one the broker does not ack within `WIFI_RESULT_ACK_WAIT` wants the
    /// link made again (a dead link otherwise waits for the keepalive).
    #[test]
    fn followup_3_a_failure_with_no_join_not_acked_in_a_few_seconds_relinks() {
        let wrong = "{\"refused\":\"wrong WiFi password: the router rejected the key; the lamp is back on its old Wi-Fi\"}\n";
        for ack in [false, true] {
            let path = socket(&format!("no-ack-{ack}"));
            let serving = helper(&path, Duration::ZERO, wrong);
            // The product's 5 s wait, shortened for this test only (#1806):
            // the relink is still decided by the ack, not by the time. A
            // second leaves the acked case's ack, sent once the result is
            // seen, room on a loaded runner.
            let ack_wait = Duration::from_secs(1);
            let changes = Changes::with_wifi_result_ack_wait(
                socket("names"),
                WAIT,
                Some((path.clone(), WIFI_WAIT)),
                ack_wait,
            );
            changes.set_broker("broker.test", 1883);
            changes
                .start_wifi(&json!({"ssid": "New Home", "password": FAKE_PASSWORD}))
                .unwrap();
            let failed = next(&changes);
            assert!(failed.result.is_err());
            assert!(!changes.take_relink(), "not at once");
            if ack {
                changes.acked(&failed);
            }
            let until = Instant::now() + ack_wait + Duration::from_millis(500);
            // The worker clears busy after resolving the ack and asking for any
            // relink. Observe that completion before checking the final result.
            while changes.wifi_busy.load(Ordering::Acquire) {
                assert!(
                    Instant::now() < until,
                    "still awaiting result ack; acked: {ack}"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            assert_eq!(changes.take_relink(), !ack, "acked: {ack}");
            ended(&changes);
            serving.join().unwrap();
            let _ = std::fs::remove_file(&path);
        }
    }

    /// Follow-up 5: a `wifi.confirm` naming a `request_id` other than the
    /// change's is refused and not handed on; one with the change's, or with
    /// none, is taken as before.
    #[test]
    fn followup_5_a_confirm_for_another_request_id_is_refused() {
        let path = socket("confirm-id");
        let serving = wifi_helper(&path, Duration::from_secs(2));
        let changes = Changes::with_wifi(socket("names"), WAIT, Some((path.clone(), WIFI_WAIT)));
        changes.set_broker("broker.test", 1883);
        let data = json!({"ssid": "New Home", "password": FAKE_PASSWORD, "request_id": "r-8"});
        changes.start_wifi(&data).unwrap();
        link_back(&changes);
        let joined = next(&changes);
        changes.acked(&joined);
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(
            changes.confirm_wifi(&json!({"request_id": "r-other"})),
            Err("this wifi.confirm's request_id is not the Wi-Fi change's".to_owned())
        );
        assert_eq!(changes.confirm_wifi(&json!({"request_id": "r-8"})), Ok(()));
        assert_eq!(
            changes.confirm_wifi(&json!({"request_id": "r-other"})),
            Err("this wifi.confirm's request_id is not the Wi-Fi change's".to_owned())
        );
        assert_eq!(changes.confirm_wifi(&json!({})), Ok(()), "none, as today");
        assert_eq!(
            serving.join().unwrap(),
            [r#"{"online":true}"#, r#"{"confirm":true}"#]
        );
        let _ = std::fs::remove_file(&path);
    }

    /// #2686's P3 5: a change sent with no `request_id` takes a confirm that
    /// has one, rather than going back after 120 s.
    #[test]
    fn a_confirm_with_a_request_id_is_taken_for_a_change_without_one() {
        let path = socket("confirm-no-id");
        let serving = wifi_helper(&path, Duration::from_secs(2));
        let changes = Changes::with_wifi(socket("names"), WAIT, Some((path.clone(), WIFI_WAIT)));
        changes.set_broker("broker.test", 1883);
        let data = json!({"ssid": "New Home", "password": FAKE_PASSWORD});
        changes.start_wifi(&data).unwrap();
        link_back(&changes);
        let joined = next(&changes);
        changes.acked(&joined);
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(changes.confirm_wifi(&json!({"request_id": "r-9"})), Ok(()));
        assert_eq!(changes.confirm_wifi(&json!({"request_id": "r-9"})), Ok(()));
        assert_eq!(
            serving.join().unwrap(),
            [r#"{"online":true}"#, r#"{"confirm":true}"#]
        );
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
