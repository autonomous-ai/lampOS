//! The backend's HTTP ping, every 15 s beside MQTT, as V1's
//! `beclient`/`status_reporter` (Nine's contract on #1396, §4): `POST
//! {base}/ping` with the voice's key as the bearer. Its reply may hand out
//! or rotate the broker's login.
use crate::account::{Account, PingMqtt};
use crate::wire::Identity;
use serde::{Deserialize, Serialize};
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
        }
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

    #[test]
    fn the_wait_doubles_to_a_minute_and_starts_again_once_connected() {
        let mut backoff = Backoff::default();
        let waits: Vec<u64> = (0..8).map(|_| backoff.wait().as_secs()).collect();
        assert_eq!(waits, [1, 2, 4, 8, 16, 32, 60, 60]);
        backoff.reset();
        assert_eq!(backoff.wait(), FIRST);
    }
}
