//! Bench-owned DeepSeek wire client. No fallback, retry, redirect or live CLI.
//! Raw bodies contain no authorization headers; keys come only from the environment.
mod stream;

use crate::{
    Result, hash, invalid,
    model::{Call, Provenance, Provider, Reply},
};
use reqwest::{
    Url,
    header::{CONTENT_TYPE, HeaderValue},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    net::IpAddr,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

const MAX_WIRE: usize = 4 * 1024 * 1024;

/// Public, frozen settings. `endpoint` is the complete chat/completions URL;
/// relay normalization happens before registration, never by guessing at runtime.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub endpoint: String,
    pub requested_model: String,
    pub expected_reported_model: String,
    pub context_tokens: u64,
}
impl Config {
    fn validate(&self, fixture: bool) -> Result<Url> {
        let url =
            Url::parse(&self.endpoint).map_err(|_| invalid("invalid public chat endpoint"))?;
        let loopback = url
            .host_str()
            .and_then(|s| s.trim_matches(['[', ']']).parse::<IpAddr>().ok())
            .is_some_and(|ip| ip.is_loopback());
        if !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.host_str().is_none()
            || if fixture {
                url.scheme() != "http" || !loopback
            } else {
                url.scheme() != "https"
            }
        {
            return Err(invalid(
                "chat endpoint must be credential-free HTTPS, or an explicit loopback fixture",
            ));
        }
        for model in [&self.requested_model, &self.expected_reported_model] {
            if model.is_empty()
                || model.len() > 128
                || !model
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-_/.:".contains(&c))
            {
                return Err(invalid("invalid pinned model identity"));
            }
        }
        if !(1024..=1_048_576).contains(&self.context_tokens) {
            return Err(invalid("invalid pinned context limit"));
        }
        Ok(url)
    }
}

pub struct Http {
    config: Config,
    endpoint: Url,
    auth: HeaderValue,
    // Never Debug/Serialize this struct, its key, or HTTP errors/headers.
    key: String,
    client: reqwest::Client,
    runtime: tokio::runtime::Runtime,
    artifacts: PathBuf,
    next: u64,
    provenance: Option<Provenance>,
}
impl Http {
    /// This constructor does not authorize spending. A live runner must attach
    /// the approved campaign and run audits to Client before invoking it.
    pub fn from_environment(config: Config, key_variable: &str, artifacts: &Path) -> Result<Self> {
        config.validate(false)?;
        if key_variable.is_empty()
            || key_variable.len() > 128
            || !key_variable
                .bytes()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_')
        {
            return Err(invalid("invalid credential environment variable name"));
        }
        let key = std::env::var(key_variable)
            .map_err(|_| invalid("required provider credential is not set"))?;
        Self::create(config, key, artifacts, false)
    }
    /// A fixed dummy credential, usable only at a literal loopback IP over HTTP.
    /// Tests never read, modify or transmit a real environment credential.
    pub fn loopback_fixture(config: Config, artifacts: &Path) -> Result<Self> {
        Self::create(
            config,
            "bench-offline-fixture-credential".into(),
            artifacts,
            true,
        )
    }
    fn create(config: Config, key: String, artifacts: &Path, fixture: bool) -> Result<Self> {
        let endpoint = config.validate(fixture)?;
        if key.len() < 8 || key.len() > 4096 || !key.bytes().all(|c| c.is_ascii_graphic()) {
            return Err(invalid("invalid provider credential"));
        }
        if contains_secret(&serde_json::to_value(&config)?, &key) {
            return Err(invalid("credential found in public provider configuration"));
        }
        let mut auth = HeaderValue::from_str(&format!("Bearer {key}"))
            .map_err(|_| invalid("invalid provider credential"))?;
        auth.set_sensitive(true);
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|_| invalid("chat TLS setup failed"))?
        .with_root_certificates(roots)
        .with_no_client_auth();
        let client = reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            .https_only(!fixture)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .no_proxy()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(30))
            .pool_max_idle_per_host(0)
            .build()
            .map_err(|_| invalid("chat client setup failed"))?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        fs::DirBuilder::new().mode(0o700).create(artifacts)?;
        File::open(
            artifacts
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new(".")),
        )?
        .sync_all()?;
        Ok(Self {
            config,
            endpoint,
            auth,
            key,
            client,
            runtime,
            artifacts: artifacts.into(),
            next: 0,
            provenance: None,
        })
    }
    fn request(&self, call: &Call) -> Result<Vec<u8>> {
        call.validate()?;
        let Call::Chat {
            messages,
            max_output_tokens,
            response_format,
            tools,
            tool_choice,
            ..
        } = call
        else {
            return Err(invalid("embedding transport is not configured"));
        };
        let mut body = json!({"model":self.config.requested_model, "messages":messages,
            "stream":true, "stream_options":{"include_usage":true},
            "thinking":{"type":"disabled"}, "max_tokens":max_output_tokens});
        if let Some(format) = response_format {
            body["response_format"] = format.clone();
        }
        if !tools.is_empty() {
            body["tools"] = json!(tools);
            body["tool_choice"] = json!(tool_choice);
        } else if tool_choice != "none" {
            return Err(invalid("tool selection requires declared tools"));
        }
        if contains_secret(&body, &self.key) {
            return Err(invalid(
                "credential found in request body; refused before dispatch",
            ));
        }
        let bytes = serde_json::to_vec(&body)?;
        // A byte per token plus message/control reserves is intentionally a
        // conservative refusal bound. Full history is never silently trimmed.
        let upper = (bytes.len() as u64)
            .checked_add(256 + 128 * messages.len() as u64)
            .and_then(|n| n.checked_add(*max_output_tokens))
            .ok_or_else(|| invalid("context bound overflow"))?;
        if upper > self.config.context_tokens {
            return Err(invalid(
                "complete request exceeds conservative pinned context bound",
            ));
        }
        if bytes
            .windows(self.key.len())
            .any(|w| w == self.key.as_bytes())
        {
            return Err(invalid(
                "credential found in request body; refused before dispatch",
            ));
        }
        Ok(bytes)
    }
}
fn save(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
impl Provider for Http {
    fn invoke(&mut self, call: &Call, deadline: Instant) -> Result<Reply> {
        self.provenance = None;
        if Instant::now() >= deadline || self.next >= 100_000 {
            return Err(invalid("chat expired or call limit reached"));
        }
        let request = self.request(call)?;
        // Refuse replacement/symlink of the private evidence directory.
        let meta = fs::symlink_metadata(&self.artifacts)?;
        if !meta.is_dir() || meta.permissions().mode() & 0o077 != 0 {
            return Err(invalid("provider artifact directory is not private"));
        }
        self.next += 1;
        let artifact = format!("{:06}", self.next);
        let dir = self.artifacts.join(&artifact);
        fs::DirBuilder::new().mode(0o700).create(&dir)?;
        save(&dir.join("request.json"), &request)?;
        File::open(&dir)?.sync_all()?;
        File::open(&self.artifacts)?.sync_all()?;
        let mut raw = Vec::new();
        let mut status = None;
        let mut parser = stream::Stream::new(&self.config.expected_reported_model, &self.key);
        if Instant::now() >= deadline {
            return Err(invalid("chat deadline expired while saving request"));
        }
        let result = self.runtime.block_on(async {
            tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), async {
                let mut response = tokio::time::timeout(
                    Duration::from_secs(40),
                    self.client
                        .post(self.endpoint.clone())
                        .header(reqwest::header::AUTHORIZATION, self.auth.clone())
                        .header(CONTENT_TYPE, "application/json")
                        .header(reqwest::header::ACCEPT, "text/event-stream")
                        .body(request.clone())
                        .send(),
                )
                .await
                .map_err(|_| invalid("chat headers deadline expired"))?
                .map_err(|_| invalid("chat connection or headers failed"))?;
                status = Some(response.status().as_u16());
                let accepted = response.status().is_success();
                let event_stream = response
                    .headers()
                    .get(CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .is_some_and(|v| {
                        v.split(';')
                            .next()
                            .is_some_and(|s| s.trim().eq_ignore_ascii_case("text/event-stream"))
                    });
                while let Some(chunk) = response
                    .chunk()
                    .await
                    .map_err(|_| invalid("chat response transport failed"))?
                {
                    let remaining = MAX_WIRE - raw.len();
                    raw.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
                    if chunk.len() > remaining {
                        return Err(invalid("chat response exceeds four MiB"));
                    }
                    if accepted && event_stream && parser.feed(&chunk, deadline)? {
                        return parser.reply();
                    }
                }
                if !accepted {
                    return Err(invalid("chat service refused request"));
                }
                if !event_stream {
                    return Err(invalid("chat response is not an event stream"));
                }
                Err(invalid("chat stream ended before terminal marker"))
            })
            .await
            .map_err(|_| invalid("chat absolute deadline expired"))?
        });
        // Credential echoes and failed evidence writes stop further spending.
        // Publish safe accounting metadata before disk writes so a known overrun
        // cannot disappear when the filesystem fails after the service replied.
        let credential_echo = parser.credential_echo || !wire_safe_to_save(&raw, &self.key);
        let mut provenance = Provenance {
            transport: "deepseek_http_sse".into(),
            requested_model: self.config.requested_model.clone(),
            reported_model: (!credential_echo).then(|| parser.model.clone()).flatten(),
            provider_request_id: (!credential_echo).then(|| parser.id.clone()).flatten(),
            system_fingerprint: (!credential_echo)
                .then(|| parser.fingerprint.clone())
                .flatten(),
            finish_reason: (!credential_echo).then(|| parser.finish.clone()).flatten(),
            http_status: status,
            request_sha256: hash(&request),
            response_sha256: hash(&raw),
            artifact,
            response_complete: parser.done,
            evidence_complete: false,
            usage: parser.done.then(|| parser.usage.clone()).flatten(),
        };
        self.provenance = Some(provenance.clone());
        if credential_echo {
            return Err(invalid(
                "provider response echoed a credential; evidence refused",
            ));
        }
        save(&dir.join("response.sse"), &raw)?;
        provenance.evidence_complete = true;
        save(
            &dir.join("provenance.json"),
            &serde_json::to_vec_pretty(&provenance)?,
        )?;
        File::open(&dir)?.sync_all()?;
        self.provenance = Some(provenance);
        result
    }
    fn take_provenance(&mut self) -> Option<Provenance> {
        self.provenance.take()
    }
}

// Inspect JSON strings even in rejected SSE frames or non-JSON envelopes.
// A malformed/truncated quoted string is not safe to archive: retain only the
// digest and safe accounting metadata. This scan is independent of acceptance.
fn wire_safe_to_save(bytes: &[u8], secret: &str) -> bool {
    if bytes.windows(secret.len()).any(|w| w == secret.as_bytes()) {
        return false;
    }
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] != b'"' {
            at += 1;
            continue;
        }
        let start = at;
        at += 1;
        let mut escaped = false;
        while at < bytes.len() {
            let byte = bytes[at];
            at += 1;
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                break;
            }
        }
        let Ok(decoded) = serde_json::from_slice::<String>(&bytes[start..at]) else {
            return false;
        };
        if decoded.contains(secret) {
            return false;
        }
    }
    true
}
fn contains_secret(value: &Value, secret: &str) -> bool {
    match value {
        Value::String(s) => s.contains(secret),
        Value::Array(items) => items.iter().any(|v| contains_secret(v, secret)),
        Value::Object(fields) => fields
            .iter()
            .any(|(key, v)| key.contains(secret) || contains_secret(v, secret)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn buffered_events_after_a_semantic_error_still_get_credential_scanning() {
        let bytes = br#"data: {"error":"stop here"}

data: {"error":"bench-offline-fixture-\u0063redential"}

"#;
        assert!(!wire_safe_to_save(
            bytes,
            "bench-offline-fixture-credential"
        ));
        assert!(!wire_safe_to_save(
            br#"data: {"unfinished":"\u0063"#,
            "fixture-key"
        ));
    }
    #[test]
    fn escaped_request_and_nested_response_strings_are_checked_as_values() {
        let key = "quote\"and\\slash";
        let body = json!({"messages":[{"content":key}],"opaque":{"nested":[key]}});
        let bytes = serde_json::to_vec(&body).unwrap();
        assert!(!bytes.windows(key.len()).any(|w| w == key.as_bytes()));
        assert!(contains_secret(&body, key));
        assert!(contains_secret(
            &serde_json::from_slice::<Value>(&bytes).unwrap(),
            key
        ));
    }
}
