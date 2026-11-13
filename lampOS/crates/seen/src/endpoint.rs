//! Where activity's frames go (#6543 phase 2): V1's address, built as V1's
//! HAL builds it (`hal/config.py`), on V1's base: the account's
//! `llm_base_url` with its trailing `/` trimmed and ending in exactly one
//! `/v1` ([`v1_base`]). V1 lamps hold `…/api/v1/ai/v1`; setup stores
//! `…/api/v1/ai/`, and without the `/v1` the relay answers 404 "route not
//! found".
//!
//! - The socket: `<v1 base>/ws/hal/api/dl/action-analysis/ws`, `https`
//!   made `wss` (V1's `replace("http","ws")`); any other path under the
//!   same base with [`Endpoint::socket`].
//! - The server's public key: `GET <v1 base>/crypto/public-key`, the body a
//!   PEM, with `X-API-Key` (V1's `fetch_public_key`), unless a local PEM is
//!   given (V1's `DL_PUBLIC_KEY_FILE`).
//! - The key: the lamp's account key (`/etc/lampOS/voice/credential`), sent
//!   as `X-API-Key`, as V1 sent `llm_api_key`.
//!
//! The key goes only to the company relay, `https://campaign-api.autonomous.ai`
//! (as `lamp_cloud`'s `hermes_llm::relay_base` holds it), or to this
//! machine (`127.0.0.1`, `localhost`, `[::1]`, for the simulator and the
//! tests). Any other base is refused, and nothing is sent.
use rsa::RsaPublicKey;
use std::{fmt, path::Path};
use zeroize::Zeroizing;

/// The company relay's host: the only remote host the key goes to.
pub const COMPANY_RELAY_HOST: &str = "campaign-api.autonomous.ai";
/// V1's activity path (`DL_MOTION_ENDPOINT`).
pub const ACTIVITY_PATH: &str = "/ws/hal/api/dl/action-analysis/ws";
/// V1's public key path (`DL_PUBLIC_KEY_URL`).
pub const PUBLIC_KEY_PATH: &str = "/crypto/public-key";
/// The longest key file or PEM read.
const MOST_FILE: u64 = 16 * 1024;

/// Where activity's bursts go, and the key they go with.
#[derive(Clone)]
pub struct Endpoint {
    /// V1's base ([`v1_base`]), `http(s)`, the perception paths hang off.
    pub base: String,
    /// The WebSocket's URL.
    pub socket_url: String,
    /// The public key's URL.
    pub public_key_url: String,
    /// The account key, sent as `X-API-Key`.
    pub api_key: Zeroizing<String>,
    /// The server's public key, when given ahead (a local PEM, or the
    /// tests'); else fetched before the first burst.
    pub public_key: Option<RsaPublicKey>,
}

impl fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Endpoint")
            .field("base", &self.base)
            .field("socket_url", &self.socket_url)
            .field("public_key_url", &self.public_key_url)
            .field("api_key", &"<hidden>")
            .field("public_key", &self.public_key.is_some())
            .finish()
    }
}

/// The host of `base` when it is allowed a key, with its scheme: the relay
/// over HTTPS only, or this machine over either.
fn allowed(base: &str) -> Option<bool> {
    let (secure, rest) = if let Some(rest) = base.strip_prefix("https://") {
        (true, rest)
    } else if let Some(rest) = base.strip_prefix("http://") {
        (false, rest)
    } else {
        return None;
    };
    let authority = rest.split('/').next().unwrap_or_default();
    if authority.contains('@') {
        return None;
    }
    if secure && authority == COMPANY_RELAY_HOST {
        return Some(true);
    }
    let host = match authority.strip_prefix('[') {
        Some(v6) => v6.split(']').next().map(|host| format!("[{host}]")),
        None => authority.split(':').next().map(str::to_owned),
    }?;
    matches!(host.as_str(), "127.0.0.1" | "localhost" | "[::1]").then_some(secure)
}

/// V1's perception base from the account's `llm_base_url`: trailing `/`
/// trimmed, ending in exactly one `/v1` (added when missing, never doubled).
pub fn v1_base(base: &str) -> String {
    let mut base = base.trim().trim_end_matches('/');
    while let Some(rest) = base.strip_suffix("/v1") {
        base = rest.trim_end_matches('/');
    }
    format!("{base}/v1")
}

impl Endpoint {
    /// From the account's `llm_base_url` as stored and the account key; the
    /// paths go on [`v1_base`]. `Err` names why nothing may be sent.
    pub fn new(base: &str, api_key: &str) -> Result<Self, String> {
        let base = v1_base(base);
        allowed(&base).ok_or_else(|| {
            format!("the perception base is not the company relay ({COMPANY_RELAY_HOST}) or this machine")
        })?;
        let api_key = api_key.trim();
        if api_key.is_empty() || api_key.chars().any(|c| c.is_control()) {
            return Err("the account key is empty or not one line".into());
        }
        let mut endpoint = Self {
            public_key_url: format!("{base}{PUBLIC_KEY_PATH}"),
            base,
            socket_url: String::new(),
            api_key: Zeroizing::new(api_key.to_owned()),
            public_key: None,
        };
        endpoint.socket_url = endpoint.socket(ACTIVITY_PATH);
        Ok(endpoint)
    }

    /// The WebSocket URL of V1's `path` on this base: `https` made `wss`,
    /// `http` made `ws`.
    pub fn socket(&self, path: &str) -> String {
        let (scheme, rest) = match self.base.strip_prefix("https://") {
            Some(rest) => ("wss", rest),
            None => (
                "ws",
                self.base.strip_prefix("http://").unwrap_or(&self.base),
            ),
        };
        format!("{scheme}://{rest}{path}")
    }

    /// From setup's `backend.json` (its `llm_base_url`) and the key file,
    /// as a unit's `LoadCredential=` hands them over.
    pub fn from_files(account: &Path, key: &Path) -> Result<Self, String> {
        let account = read_small(account).map_err(|error| format!("the account: {error}"))?;
        let base = serde_json::from_str::<serde_json::Value>(&account)
            .ok()
            .and_then(|value| value.get("llm_base_url")?.as_str().map(str::to_owned))
            .ok_or("the account has no llm_base_url")?;
        let key = Zeroizing::new(read_small(key).map_err(|error| format!("the key: {error}"))?);
        Self::new(&base, &key)
    }

    /// With the server's public key from a local PEM (V1's
    /// `DL_PUBLIC_KEY_FILE`), so none is fetched.
    pub fn with_public_key_file(mut self, pem: &Path) -> Result<Self, String> {
        let pem = read_small(pem).map_err(|error| format!("the public key: {error}"))?;
        self.public_key =
            Some(crate::crypto::public_key_from_pem(&pem).map_err(|e| e.to_string())?);
        Ok(self)
    }
}

/// A small text file, at most [`MOST_FILE`] bytes.
fn read_small(path: &Path) -> Result<String, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut text = String::new();
    file.take(MOST_FILE + 1)
        .read_to_string(&mut text)
        .map_err(|error| error.to_string())?;
    if text.len() as u64 > MOST_FILE {
        return Err("too long".into());
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_address_is_v1s_on_exactly_one_v1() {
        const RELAY: &str = "https://campaign-api.autonomous.ai/api/v1/ai";
        for base in [
            RELAY.to_owned(),
            format!("{RELAY}/"),
            format!("{RELAY}/v1"),
            format!("{RELAY}/v1/"),
            format!(" {RELAY}//v1//v1/ "),
        ] {
            let endpoint = Endpoint::new(&base, "k").unwrap();
            assert_eq!(endpoint.base, format!("{RELAY}/v1"), "{base}");
            assert_eq!(
                endpoint.public_key_url,
                "https://campaign-api.autonomous.ai/api/v1/ai/v1/crypto/public-key",
                "{base}"
            );
            assert_eq!(
                endpoint.socket_url,
                "wss://campaign-api.autonomous.ai/api/v1/ai/v1/ws/hal/api/dl/action-analysis/ws",
                "{base}"
            );
            assert_eq!(
                endpoint.socket("/ws/hal/api/dl/pose-estimation/ws"),
                "wss://campaign-api.autonomous.ai/api/v1/ai/v1/ws/hal/api/dl/pose-estimation/ws",
                "{base}"
            );
        }
        let local = Endpoint::new("http://127.0.0.1:8080", "k").unwrap();
        assert_eq!(
            local.socket_url,
            "ws://127.0.0.1:8080/v1/ws/hal/api/dl/action-analysis/ws"
        );
        assert_eq!(
            local.public_key_url,
            "http://127.0.0.1:8080/v1/crypto/public-key"
        );
        assert!(Endpoint::new("http://[::1]:9/x", "k").is_ok());
        assert!(Endpoint::new("https://localhost", "k").is_ok());
    }

    #[test]
    fn the_key_goes_only_to_the_relay_or_this_machine() {
        for base in [
            "http://campaign-api.autonomous.ai/api/v1/ai",
            "https://campaign-api.autonomous.ai.evil.example/x",
            "https://user@campaign-api.autonomous.ai/x",
            "https://campaign-api.autonomous.ai:444/x",
            "https://example.com",
            "ftp://127.0.0.1",
            "127.0.0.1",
            "",
        ] {
            assert!(Endpoint::new(base, "k").is_err(), "{base}");
        }
        assert!(Endpoint::new("https://campaign-api.autonomous.ai", "").is_err());
        assert!(Endpoint::new("https://campaign-api.autonomous.ai", "a\nb").is_err());
    }

    #[test]
    fn the_key_is_never_printed() {
        let endpoint = Endpoint::new("http://127.0.0.1:1", "secret-key").unwrap();
        assert!(!format!("{endpoint:?}").contains("secret-key"));
    }
}
