//! An OAuth token renewed through the backend, as V1's
//! `connector_refresh.go` does: `POST <base>/connector/refresh-token` with
//! `{"connector","refresh_token"}`, the lamp's key as the bearer and its id
//! in `X-Device-ID`. No client secret is on the lamp.
//!
//! The backend's link and the lamp's key are read as lamp-cloud reads them,
//! from systemd's credentials (`LoadCredential=account:` setup's
//! `backend.json`, `key:` the voice's credential), for this alone.
use crate::catalog::Method;
use crate::net::{Call, Reply};
use crate::store::{DEFAULT_LIFETIME_S, Entry};
use serde::Deserialize;
use std::{fmt, path::Path};

/// V1's `connectorRefreshPath`, after the base with `/v1` cut off.
pub const PATH: &str = "/connector/refresh-token";

/// The backend, for refresh only. Its `Debug` hides the key.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Backend {
    /// The backend's base, `/v1` cut off, no trailing `/`.
    pub base: String,
    pub key: String,
    pub device_id: String,
}

impl fmt::Debug for Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Backend")
            .field("base", &self.base)
            .field("key", &"<hidden>")
            .field("device_id", &self.device_id)
            .finish()
    }
}

/// The two fields of setup's account read here; the broker's login is
/// never read.
#[derive(Default, Deserialize)]
#[serde(default)]
struct Account {
    device_id: String,
    llm_base_url: String,
}

impl Backend {
    /// From systemd's credentials folder: `account` and `key`. `None`
    /// without a base or a key, or a base that is not https.
    pub fn load(credentials: &Path) -> Option<Backend> {
        let account: Account =
            serde_json::from_slice(&std::fs::read(credentials.join("account")).ok()?).ok()?;
        let key = std::fs::read_to_string(credentials.join("key")).ok()?;
        Backend::new(&account.llm_base_url, key.trim(), &account.device_id)
    }

    /// A backend from its parts, the base's trailing `/` and `/v1` cut off.
    pub fn new(base: &str, key: &str, device_id: &str) -> Option<Backend> {
        let base = base.trim().trim_end_matches('/');
        let base = base.strip_suffix("/v1").unwrap_or(base);
        if !base.starts_with("https://") || key.is_empty() || key.chars().any(char::is_control) {
            return None;
        }
        Some(Backend {
            base: base.to_owned(),
            key: key.to_owned(),
            device_id: device_id.trim().to_owned(),
        })
    }

    /// The refresh request for `entry`.
    pub fn call(&self, entry: &Entry) -> Call {
        let body = serde_json::json!({
            "connector": entry.connector,
            "refresh_token": entry.refresh_token,
        });
        let mut headers = vec![
            ("authorization".to_owned(), format!("Bearer {}", self.key)),
            ("content-type".to_owned(), "application/json".to_owned()),
            ("accept".to_owned(), "application/json".to_owned()),
        ];
        if !self.device_id.is_empty() {
            headers.push(("x-device-id".to_owned(), self.device_id.clone()));
        }
        Call {
            method: Method::Post,
            url: format!("{}{PATH}", self.base),
            headers,
            body: Some(body.to_string().into_bytes()),
            most: crate::net::BODY_MOST,
        }
    }
}

/// V1's `connectorRefreshResult`. No `Debug`: it holds the tokens.
#[derive(Default, Deserialize)]
#[serde(default)]
struct Refreshed {
    access_token: String,
    refresh_token: String,
    token_type: String,
    expires_in: i64,
}

/// `entry` with the backend's answer applied (V1's `loadRefreshedCreds`),
/// or why not, in words that hold nothing of the answer.
pub fn apply(entry: &Entry, reply: &Reply, now: i64) -> Result<Entry, &'static str> {
    if reply.status != 200 {
        return Err("the backend didn't renew the link");
    }
    let refreshed: Refreshed =
        serde_json::from_slice(&reply.body).map_err(|_| "the backend's answer didn't read")?;
    if refreshed.access_token.is_empty()
        || refreshed.access_token.chars().any(char::is_control)
        || refreshed.refresh_token.chars().any(char::is_control)
    {
        return Err("the backend's answer had no usable token");
    }
    // V1's guard: a 0 would store "now" and spin the refresh.
    if refreshed.expires_in <= 0 {
        return Err("the backend's answer had no lifetime");
    }
    let mut next = entry.clone();
    next.access_token = refreshed.access_token;
    if !refreshed.token_type.is_empty() {
        next.token_type = refreshed.token_type;
    }
    next.expires_at = now + refreshed.expires_in.min(10 * DEFAULT_LIFETIME_S * 24);
    if !refreshed.refresh_token.is_empty() {
        next.refresh_token = refreshed.refresh_token;
    }
    next.obtained_at = now;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_refresh_is_v1s_request_and_applies_its_answer() {
        let backend = Backend::new("https://api.test/v1/", "lamp-key-planted", "d-1").unwrap();
        assert_eq!(backend.base, "https://api.test");
        assert!(!format!("{backend:?}").contains("planted"));
        assert!(Backend::new("http://api.test", "k", "d").is_none());
        assert!(Backend::new("https://api.test", "", "d").is_none());
        let entry = Entry::from_set(
            "notion",
            &json!({"auth_type":"oauth","access_token":"old","refresh_token":"r-1","refresh":true,"expires_in":10}),
            0,
        )
        .unwrap();
        let call = backend.call(&entry);
        assert_eq!(call.url, "https://api.test/connector/refresh-token");
        let body: serde_json::Value = serde_json::from_slice(call.body.as_ref().unwrap()).unwrap();
        assert_eq!(body, json!({"connector":"notion","refresh_token":"r-1"}));
        assert!(call.headers.contains(&("x-device-id".into(), "d-1".into())));
        let reply = |status, body: serde_json::Value| Reply {
            status,
            body: body.to_string().into_bytes(),
            ..Reply::default()
        };
        let next = apply(
            &entry,
            &reply(200, json!({"access_token":"new","expires_in":3600})),
            100,
        )
        .unwrap();
        assert_eq!(next.access_token, "new");
        assert_eq!(next.refresh_token, "r-1");
        assert_eq!(next.expires_at, 3700);
        assert!(apply(&entry, &reply(200, json!({"access_token":"new"})), 1).is_err());
        assert!(
            apply(
                &entry,
                &reply(401, json!({"access_token":"new","expires_in":5})),
                1
            )
            .is_err()
        );
    }

    #[test]
    fn the_backend_is_read_from_lamp_clouds_credentials() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("account"),
            json!({"device_id":"d-9","llm_base_url":"https://api.test/v1","mqtt_password":"x"})
                .to_string(),
        )
        .unwrap();
        std::fs::write(dir.path().join("key"), "k-1\n").unwrap();
        let backend = Backend::load(dir.path()).unwrap();
        assert_eq!(
            backend,
            Backend::new("https://api.test", "k-1", "d-9").unwrap()
        );
    }
}
