//! The account link setup keeps: the company broker's address, its login,
//! the two channels and the backend's base (setup's
//! `/etc/lampOS/setup/backend.json`, root's, 0600, read through systemd's
//! `LoadCredential=`). The broker's
//! password is a secret: it is never printed, in a log line or a debug one.
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

/// What setup kept from the app's request (V1's `SetupRequest` fields), and
/// what a ping's reply may replace.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    #[serde(default)]
    pub device_id: String,
    #[serde(default)]
    pub mqtt_endpoint: String,
    /// A number or a string: the app sends one, a ping's reply the other.
    #[serde(default, deserialize_with = "port")]
    pub mqtt_port: u16,
    #[serde(default)]
    pub mqtt_username: String,
    #[serde(default)]
    pub mqtt_password: String,
    #[serde(default)]
    pub fa_channel: String,
    #[serde(default)]
    pub fd_channel: String,
    /// The backend's base, the ping's (`/v1` cut off).
    #[serde(default)]
    pub llm_base_url: String,
}

/// Shown in place of a secret.
const HIDDEN: &str = "<hidden>";

impl fmt::Debug for Account {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Account")
            .field("device_id", &self.device_id)
            .field("mqtt_endpoint", &self.mqtt_endpoint)
            .field("mqtt_port", &self.mqtt_port)
            .field("mqtt_username", &self.mqtt_username)
            .field("mqtt_password", &HIDDEN)
            .field("fa_channel", &self.fa_channel)
            .field("fd_channel", &self.fd_channel)
            .field("llm_base_url", &self.llm_base_url)
            .finish()
    }
}

fn port<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u16, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Port {
        Number(u16),
        Text(String),
    }
    match Port::deserialize(deserializer)? {
        Port::Number(port) => Ok(port),
        Port::Text(text) if text.trim().is_empty() => Ok(0),
        Port::Text(text) => text
            .trim()
            .parse()
            .map_err(|_| serde::de::Error::custom("mqtt_port is not a port")),
    }
}

impl Account {
    /// Enough to go online: a broker, a port, a login and both channels.
    /// The client id is V1's, `os-server-{device_id}`, so the id is needed
    /// too.
    pub fn complete(&self) -> bool {
        [
            &self.device_id,
            &self.mqtt_endpoint,
            &self.mqtt_username,
            &self.fa_channel,
            &self.fd_channel,
        ]
        .iter()
        .all(|field| !field.trim().is_empty())
            && self.mqtt_port != 0
    }

    /// V1's client id. Only one client may hold it: the unit conflicts
    /// with V1's `os-server`.
    pub fn client_id(&self) -> String {
        format!("os-server-{}", self.device_id.trim())
    }

    /// This account with a ping reply's broker in place of its own, if the
    /// reply carries one that differs; `None` when nothing changes.
    pub fn rotated(&self, mqtt: &PingMqtt) -> Option<Self> {
        if mqtt.mqtt_server.trim().is_empty() {
            return None;
        }
        let port = mqtt.mqtt_port.trim().parse().unwrap_or(self.mqtt_port);
        let pick = |new: &str, old: &str| {
            if new.trim().is_empty() {
                old.to_owned()
            } else {
                new.trim().to_owned()
            }
        };
        let next = Self {
            mqtt_endpoint: mqtt.mqtt_server.trim().to_owned(),
            mqtt_port: port,
            mqtt_username: pick(&mqtt.mqtt_usr, &self.mqtt_username),
            mqtt_password: if mqtt.mqtt_pwd.is_empty() {
                self.mqtt_password.clone()
            } else {
                mqtt.mqtt_pwd.clone()
            },
            fa_channel: pick(&mqtt.fa_channel, &self.fa_channel),
            fd_channel: pick(&mqtt.fd_channel, &self.fd_channel),
            ..self.clone()
        };
        (next != *self).then_some(next)
    }
}

/// Keep `account` at `path`, as only this process's user may read it
/// (0600), written whole: beside, synced, renamed. The rotated login a ping
/// handed out lives here, in `lamp-cloud`'s own state folder, which the
/// reset removes.
pub fn keep(path: &Path, account: &Account) -> io::Result<()> {
    let beside = path.with_extension("new");
    let text = serde_json::to_vec(account).map_err(io::Error::other)?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&beside)?;
    file.write_all(&text)?;
    file.sync_all()?;
    std::fs::rename(&beside, path)
}

/// The account kept at `path`, if one is there and reads.
pub fn kept(path: &Path) -> Option<Account> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// The broker a ping's reply hands out (V1's `beclient.MQTTConfig`).
#[derive(Clone, Default, PartialEq, Eq, Deserialize)]
pub struct PingMqtt {
    #[serde(default)]
    pub mqtt_server: String,
    #[serde(default)]
    pub mqtt_port: String,
    #[serde(default)]
    pub mqtt_usr: String,
    #[serde(default)]
    pub mqtt_pwd: String,
    #[serde(default)]
    pub fa_channel: String,
    #[serde(default)]
    pub fd_channel: String,
}

impl fmt::Debug for PingMqtt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PingMqtt")
            .field("mqtt_server", &self.mqtt_server)
            .field("mqtt_port", &self.mqtt_port)
            .field("mqtt_usr", &self.mqtt_usr)
            .field("mqtt_pwd", &HIDDEN)
            .field("fa_channel", &self.fa_channel)
            .field("fd_channel", &self.fd_channel)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Account {
        serde_json::from_value(serde_json::json!({
            "device_id": "d-42",
            "mqtt_endpoint": "broker.test",
            "mqtt_port": 1883,
            "mqtt_username": "lamp",
            "mqtt_password": "s3cret-pw",
            "fa_channel": "fa/d-42",
            "fd_channel": "fd/d-42",
            "llm_base_url": "https://api.test/v1"
        }))
        .unwrap()
    }

    #[test]
    fn a_kept_account_is_complete_and_its_secret_is_never_printed() {
        let account = sample();
        assert!(account.complete());
        assert_eq!(account.client_id(), "os-server-d-42");
        let printed = format!("{account:?}");
        assert!(!printed.contains("s3cret-pw"), "{printed}");
        assert!(printed.contains("<hidden>"));
        // The port as the app's string reads the same.
        let text: Account =
            serde_json::from_value(serde_json::json!({"mqtt_port": "1883"})).unwrap();
        assert_eq!(text.mqtt_port, 1883);
        assert!(serde_json::from_value::<Account>(serde_json::json!({"mqtt_port": "x"})).is_err());
        // Any field missing: not complete.
        for blank in [
            Account {
                device_id: " ".into(),
                ..sample()
            },
            Account {
                mqtt_port: 0,
                ..sample()
            },
            Account {
                fd_channel: String::new(),
                ..sample()
            },
            Account::default(),
        ] {
            assert!(!blank.complete(), "{blank:?}");
        }
    }

    #[test]
    fn a_rotated_account_is_kept_for_this_user_alone() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::path::PathBuf::from(format!("/tmp/lck-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rotated.json");
        keep(&path, &sample()).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(kept(&path), Some(sample()));
        assert_eq!(kept(&dir.join("none.json")), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_ping_reply_rotates_only_what_it_changes() {
        let account = sample();
        // The same broker: nothing to do.
        let same = PingMqtt {
            mqtt_server: "broker.test".into(),
            mqtt_port: "1883".into(),
            ..PingMqtt::default()
        };
        assert_eq!(account.rotated(&same), None);
        // No broker in the reply: nothing either.
        assert_eq!(account.rotated(&PingMqtt::default()), None);
        let moved = PingMqtt {
            mqtt_server: "broker2.test".into(),
            mqtt_port: "8883".into(),
            mqtt_usr: "lamp2".into(),
            mqtt_pwd: "new-pw".into(),
            fa_channel: String::new(),
            fd_channel: String::new(),
        };
        let next = account.rotated(&moved).unwrap();
        assert_eq!(
            (
                next.mqtt_endpoint.as_str(),
                next.mqtt_port,
                next.mqtt_username.as_str()
            ),
            ("broker2.test", 8883, "lamp2")
        );
        assert_eq!(next.mqtt_password, "new-pw");
        assert_eq!(
            (next.fa_channel.as_str(), next.device_id.as_str()),
            ("fa/d-42", "d-42")
        );
        assert!(!format!("{moved:?}").contains("new-pw"));
    }
}
