//! The backend's HTTP ping, every 15 s beside MQTT, as V1's
//! `beclient`/`status_reporter` (Nine's contract on #1396, §4): `POST
//! {base}/ping` with the voice's key as the bearer. Its reply may hand out
//! or rotate the broker's login.
use crate::account::{Account, PingMqtt};
use crate::wire::Identity;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// V1's `StatusReportInterval`.
pub const EVERY: Duration = Duration::from_secs(15);

/// Where the ping goes: the backend's base with a trailing `/v1` cut off
/// (the ping lives one level above the OpenAI-compatible base), and
/// `?mqtt=true` while no broker is kept, so the reply carries one. `None`
/// without a base.
pub fn url(account: &Account) -> Option<String> {
    let base = account.llm_base_url.trim().trim_end_matches('/');
    let base = base.strip_suffix("/v1").unwrap_or(base);
    if base.is_empty() {
        return None;
    }
    let mut url = format!("{base}/ping");
    if account.mqtt_endpoint.trim().is_empty() {
        url.push_str("?mqtt=true");
    }
    Some(url)
}

/// V1's `PingPayload`, with the fields V2 has.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Payload {
    pub status: &'static str,
    pub setup_completed: bool,
    pub mac: String,
    pub version: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub local_ip: String,
    pub device: String,
    pub device_id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub timezone: String,
    /// V1 always sends these two.
    pub wakeword_enabled: bool,
    pub voice_input_mode: &'static str,
    /// `true` when lamp-cloud runs with `LAMP_CLOUD_TLS=on`, so the backend
    /// can hand this lamp its TLS broker (#6031); left out otherwise, so a
    /// lamp without the switch sends what it sent before.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub mqtt_tls: bool,
}

impl Payload {
    pub fn of(identity: &Identity) -> Self {
        Self {
            status: "working",
            setup_completed: true,
            mac: identity.mac.clone(),
            version: identity.version.clone(),
            local_ip: identity.local_ip.clone(),
            device: identity.device.clone(),
            device_id: identity.id.clone(),
            timezone: identity.timezone.clone(),
            wakeword_enabled: false,
            voice_input_mode: "automatic",
            mqtt_tls: false,
        }
    }

    /// The payload saying whether the broker is reached over TLS
    /// ([`crate::online::tls_on`], #6031).
    pub fn with_tls(mut self, tls: bool) -> Self {
        self.mqtt_tls = tls;
        self
    }
}

/// V1's `PingResponse`. A reply that does not read as one is no reply, as
/// V1 takes it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct Reply {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub device_id: String,
    #[serde(default)]
    pub mqtt: Option<PingMqtt>,
    /// Where the backend places the lamp: `""` or `"CN"` (#6414). Anything
    /// that is not a short code of letters reads as `""`, so a strange value
    /// never loses the rest of the reply.
    #[serde(default, deserialize_with = "region_code")]
    pub region: String,
    /// Where the broker takes MQTT over WSS (#6256, B11), for when its TCP
    /// port keeps failing (`crate::wss`). Anything not a string reads as
    /// `""`. Kept in memory only: the first ping goes out at start, before
    /// TCP has failed [`crate::wss::FAILURES`] times.
    #[serde(default, deserialize_with = "text")]
    pub mqtt_ws_url: String,
}

fn text<'de, D: serde::Deserializer<'de>>(from: D) -> Result<String, D::Error> {
    Ok(match serde_json::Value::deserialize(from)? {
        serde_json::Value::String(text) => text.trim().to_owned(),
        _ => String::new(),
    })
}

fn region_code<'de, D: serde::Deserializer<'de>>(from: D) -> Result<String, D::Error> {
    Ok(match serde_json::Value::deserialize(from)? {
        serde_json::Value::String(code)
            if code.len() <= 8 && code.chars().all(|c| c.is_ascii_alphabetic()) =>
        {
            code.to_ascii_uppercase()
        }
        _ => String::new(),
    })
}

/// The region a ping's reply last said, kept in `region` in lamp-cloud's
/// state folder for the voice to read (#6414): 0644, written whole (a new
/// file renamed over it), and only when it changes. Kept across boots, so
/// a start compares with what the last run wrote.
pub struct Region {
    path: PathBuf,
    kept: String,
}

impl Region {
    pub fn at(path: PathBuf) -> Self {
        let kept = std::fs::read_to_string(&path)
            .map(|text| text.trim().to_owned())
            .unwrap_or_default();
        Self { path, kept }
    }

    /// Takes a reply's region: `None` when it is the one kept, else what
    /// writing it came to. A failed write is not tried again until the
    /// region changes once more, so the journal says it once.
    pub fn take(&mut self, region: &str) -> Option<std::io::Result<()>> {
        if region == self.kept {
            return None;
        }
        self.kept = region.to_owned();
        Some(write_region(&self.path, region))
    }
}

fn write_region(path: &Path, region: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let beside = path.with_extension("new");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o644)
        .open(&beside)?;
    file.write_all(format!("{region}\n").as_bytes())?;
    file.sync_all()?;
    std::fs::rename(&beside, path)
}

pub fn reply(body: &[u8]) -> Option<Reply> {
    serde_json::from_slice(body).ok()
}

/// The account after a reply: a new broker or device id taken, `None` when
/// nothing changed.
pub fn after(account: &Account, reply: &Reply) -> Option<Account> {
    let mut next = reply
        .mqtt
        .as_ref()
        .and_then(|mqtt| account.rotated(mqtt))
        .unwrap_or_else(|| account.clone());
    if !reply.device_id.trim().is_empty() {
        next.device_id = reply.device_id.trim().to_owned();
    }
    (next != *account).then_some(next)
}

/// A wait that doubles from 1 s to 60 s, and starts again from 1 s once a
/// connection holds.
#[derive(Clone, Debug)]
pub struct Backoff {
    next: Duration,
}

pub const FIRST: Duration = Duration::from_secs(1);
pub const MOST: Duration = Duration::from_secs(60);

impl Default for Backoff {
    fn default() -> Self {
        Self { next: FIRST }
    }
}

impl Backoff {
    /// How long to wait before the next try.
    pub fn wait(&mut self) -> Duration {
        let wait = self.next;
        self.next = (self.next * 2).min(MOST);
        wait
    }
    pub fn reset(&mut self) {
        self.next = FIRST;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn account(base: &str, endpoint: &str) -> Account {
        Account {
            device_id: "d-42".into(),
            mqtt_endpoint: endpoint.into(),
            mqtt_port: 1883,
            mqtt_username: "lamp".into(),
            mqtt_password: "pw".into(),
            fa_channel: "fa/d-42".into(),
            fd_channel: "fd/d-42".into(),
            llm_base_url: base.into(),
        }
    }

    #[test]
    fn the_ping_goes_above_the_v1_base_and_asks_for_a_broker_while_none_is_kept() {
        assert_eq!(
            url(&account("https://api.test/v1", "broker.test")).as_deref(),
            Some("https://api.test/ping")
        );
        assert_eq!(
            url(&account("https://api.test/v1/", "")).as_deref(),
            Some("https://api.test/ping?mqtt=true")
        );
        assert_eq!(
            url(&account("https://api.test", "b")).as_deref(),
            Some("https://api.test/ping")
        );
        assert_eq!(url(&account(" ", "b")), None);
    }

    #[test]
    fn the_payload_is_v1s_and_a_reply_may_move_the_broker_or_the_id() {
        let identity = Identity {
            device: "lamp".into(),
            version: "0.1.0+abc".into(),
            id: "d-42".into(),
            mac: "lamp-7f72".into(),
            timezone: String::new(),
            local_ip: "192.168.1.42".into(),
        };
        assert_eq!(
            serde_json::to_value(Payload::of(&identity)).unwrap(),
            json!({"status":"working","setup_completed":true,"mac":"lamp-7f72",
                "version":"0.1.0+abc","local_ip":"192.168.1.42","device":"lamp",
                "device_id":"d-42","wakeword_enabled":false,"voice_input_mode":"automatic"})
        );
        let kept = account("https://api.test/v1", "broker.test");
        let plain = reply(br#"{"status":"ok"}"#).unwrap();
        assert_eq!(after(&kept, &plain), None);
        let moved = reply(
            br#"{"status":"ok","mqtt":{"mqtt_server":"b2.test","mqtt_port":"1884","mqtt_usr":"u2","mqtt_pwd":"p2"}}"#,
        )
        .unwrap();
        let next = after(&kept, &moved).unwrap();
        assert_eq!(
            (next.mqtt_endpoint.as_str(), next.mqtt_port),
            ("b2.test", 1884)
        );
        let renamed = reply(br#"{"device_id":"d-43"}"#).unwrap();
        assert_eq!(after(&kept, &renamed).unwrap().device_id, "d-43");
        // Not a reply: nothing, as V1.
        assert_eq!(reply(b"<html>"), None);
    }

    /// #6031: with `LAMP_CLOUD_TLS=on` the ping says `"mqtt_tls": true`;
    /// off, unset or any other value, the key is not there at all, so the
    /// body is byte for byte what it was.
    #[test]
    fn the_ping_says_mqtt_tls_only_with_its_switch_on() {
        let identity = Identity {
            device: "lamp".into(),
            version: "0.1.0+abc".into(),
            id: "d-42".into(),
            mac: "lamp-7f72".into(),
            timezone: String::new(),
            local_ip: String::new(),
        };
        let before = serde_json::to_vec(&Payload::of(&identity)).unwrap();
        for value in [None, Some("off"), Some("ON"), Some("1"), Some("on ")] {
            let body = Payload::of(&identity).with_tls(crate::online::tls_from(value));
            let bytes = serde_json::to_vec(&body).unwrap();
            assert_eq!(bytes, before, "{value:?}");
            assert!(
                serde_json::to_value(&body)
                    .unwrap()
                    .get("mqtt_tls")
                    .is_none()
            );
        }
        let on = Payload::of(&identity).with_tls(crate::online::tls_from(Some("on")));
        let value = serde_json::to_value(&on).unwrap();
        assert_eq!(value["mqtt_tls"], json!(true));
        let mut without = value.as_object().unwrap().clone();
        without.remove("mqtt_tls");
        assert_eq!(
            serde_json::Value::Object(without),
            serde_json::to_value(Payload::of(&identity)).unwrap(),
            "nothing else changes"
        );
    }

    /// #6414: a reply's region, missing or strange, never loses the reply.
    #[test]
    fn a_reply_reads_with_its_region_without_one_or_with_a_strange_one() {
        let with = reply(br#"{"status":"ok","region":"CN"}"#).unwrap();
        assert_eq!(with.region, "CN");
        assert_eq!(reply(br#"{"status":"ok"}"#).unwrap().region, "");
        for strange in ["5", "null", r#"["CN"]"#, r#""C N""#, r#""../x""#] {
            let body = format!(r#"{{"status":"ok","device_id":"d-43","region":{strange}}}"#);
            let read = reply(body.as_bytes()).unwrap();
            assert_eq!(
                (read.region.as_str(), read.device_id.as_str()),
                ("", "d-43")
            );
        }
    }

    /// #6256 F9: the WSS address, missing or strange, never loses the reply.
    #[test]
    fn a_reply_reads_with_its_wss_address_without_one_or_with_a_strange_one() {
        let with = reply(br#"{"status":"ok","mqtt_ws_url":"wss://b.test/mqtt"}"#).unwrap();
        assert_eq!(with.mqtt_ws_url, "wss://b.test/mqtt");
        assert_eq!(reply(br#"{"status":"ok"}"#).unwrap().mqtt_ws_url, "");
        for strange in ["null", "443", "[\"wss://x\"]", "{}"] {
            let body = format!(r#"{{"status":"ok","device_id":"d-9","mqtt_ws_url":{strange}}}"#);
            let read = reply(body.as_bytes()).unwrap();
            assert_eq!(
                (read.mqtt_ws_url.as_str(), read.device_id.as_str()),
                ("", "d-9")
            );
        }
    }

    #[test]
    fn the_region_is_written_only_when_it_changes_and_read_back_at_start() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("lamp-cloud-region-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("region");
        let mut region = Region::at(path.clone());
        assert!(region.take("").is_none(), "nothing kept is no region");
        assert!(!path.exists());
        assert!(region.take("CN").unwrap().is_ok());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "CN\n");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode & 0o044, 0o044, "readable by others: {mode:o}");
        assert!(region.take("CN").is_none(), "unchanged: not written");
        // A new start reads what was kept.
        let mut again = Region::at(path.clone());
        assert!(again.take("CN").is_none());
        assert!(again.take("").unwrap().is_ok());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_wait_doubles_to_a_minute_and_starts_again_once_connected() {
        let mut backoff = Backoff::default();
        let waits: Vec<u64> = (0..8).map(|_| backoff.wait().as_secs()).collect();
        assert_eq!(waits, [1, 2, 4, 8, 16, 32, 60, 60]);
        backoff.reset();
        assert_eq!(backoff.wait(), FIRST);
    }
}
