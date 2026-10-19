//! The second route's network clients, speaking V1's protocols: a
//! streaming speech-to-text websocket (Deepgram or the Autonomous proxy),
//! a streamed OpenAI-compatible chat completion, and streamed speech from
//! OpenAI or ElevenLabs. Every connect and read is bounded, and no error
//! carries a key, a URL or a server's words.
use std::{fmt, sync::Arc, time::Duration};

mod chat;
mod gemini_tts;
pub mod key_terms;
pub mod search;
mod sentences;
mod stt;
mod tts;
pub mod weather;

pub use chat::{Chat, ChatEvent, ChatStream, Declare, GEMINI_REASONING, Message, ToolCall};
pub use gemini_tts::{GeminiSpeech, GeminiTts};
pub use sentences::Sentences;
pub use stt::{Early, Replies, SttStream, check as stt_check};
pub use tts::{PACKET_SAMPLES, TTS_SAMPLE_RATE, Tts, TtsStream, default_model};

/// To open a connection, TLS included.
pub(crate) const CONNECT: Duration = Duration::from_secs(10);
/// For a streamed response to say anything more. A chat model thinking
/// before its first word is the slowest case seen in V1 (6-19 s).
pub(crate) const STALL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// A configured URL that cannot be used.
    Url,
    /// No connection, or it broke; which step.
    Transport(&'static str),
    /// A step took longer than its bound.
    Timeout(&'static str),
    /// The service answered with this HTTP status.
    Status(u16),
    /// The service said something this client does not read.
    Protocol(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Url => f.write_str("unusable service URL"),
            Error::Transport(step) => write!(f, "{step} failed"),
            Error::Timeout(step) => write!(f, "{step} timed out"),
            Error::Status(code) => write!(f, "service answered HTTP {code}"),
            Error::Protocol(what) => write!(f, "unexpected {what}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// V1's `_ensure_openai_v1`: the Autonomous proxy's base URL ending in
/// `/ai` is given `/v1`; any other is used as it is, less a trailing `/`.
pub(crate) fn openai_base(url: &str) -> String {
    let base = url.trim_end_matches('/');
    if base.contains("campaign-api.autonomous.ai") && base.ends_with("/ai") {
        format!("{base}/v1")
    } else {
        base.to_owned()
    }
}

/// TLS 1.2 and 1.3 with the `ring` provider and the webpki roots, as the
/// other lampOS clients have it.
fn tls() -> Result<rustls::ClientConfig> {
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    Ok(rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|_| Error::Transport("TLS setup"))?
        .with_root_certificates(roots)
        .with_no_client_auth())
}

/// HTTPS only, except to the tests' plain local server.
fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .tls_backend_preconfigured(tls()?)
        .https_only(!cfg!(test))
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(CONNECT)
        .read_timeout(STALL)
        .build()
        .map_err(|_| Error::Transport("HTTPS client setup"))
}

/// Send a JSON POST and check the status. The body is a string so that no
/// new `reqwest` feature (and lock entry) is needed.
async fn post(
    client: &reqwest::Client,
    url: &str,
    auth: (&'static str, &str),
    body: &serde_json::Value,
) -> Result<reqwest::Response> {
    let request = client
        .post(url)
        .header(auth.0, auth.1)
        .header("content-type", "application/json")
        .body(body.to_string());
    let response = tokio::time::timeout(CONNECT + STALL, request.send())
        .await
        .map_err(|_| Error::Timeout("request"))?
        .map_err(|error| {
            if error.is_timeout() {
                Error::Timeout("request")
            } else {
                Error::Transport("request")
            }
        })?;
    if !response.status().is_success() {
        return Err(Error::Status(response.status().as_u16()));
    }
    Ok(response)
}

/// A JSON `GET` with the key in `authorization` (a service's model list),
/// bounded as a request and at 1 MiB.
pub async fn get_json(url: &str, authorization: &str) -> Result<serde_json::Value> {
    fetch_json(url, Some(authorization)).await
}

/// A JSON `GET` of a service that takes no key (Open-Meteo), bounded as
/// [`get_json`] is.
pub(crate) async fn get_public_json(url: &str) -> Result<serde_json::Value> {
    fetch_json(url, None).await
}

async fn fetch_json(url: &str, authorization: Option<&str>) -> Result<serde_json::Value> {
    let client = http_client()?;
    let mut request = client.get(url);
    if let Some(authorization) = authorization {
        request = request.header("authorization", authorization);
    }
    let mut response = tokio::time::timeout(CONNECT + STALL, request.send())
        .await
        .map_err(|_| Error::Timeout("request"))?
        .map_err(|_| Error::Transport("request"))?;
    if !response.status().is_success() {
        return Err(Error::Status(response.status().as_u16()));
    }
    let mut bytes = Vec::new();
    while let Some(piece) = chunk(&mut response).await? {
        bytes.extend(piece);
        if bytes.len() > 1 << 20 {
            return Err(Error::Protocol("answer length"));
        }
    }
    serde_json::from_slice(&bytes).map_err(|_| Error::Protocol("answer"))
}

/// The next piece of a streamed body; `None` at its end.
async fn chunk(response: &mut reqwest::Response) -> Result<Option<Vec<u8>>> {
    let next = response.chunk().await.map_err(|error| {
        if error.is_timeout() {
            Error::Timeout("response")
        } else {
            Error::Transport("response")
        }
    })?;
    Ok(next.map(|bytes| bytes.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_proxy_url_ending_in_ai_gets_v1() {
        assert_eq!(
            openai_base("https://campaign-api.autonomous.ai/api/v1/ai/"),
            "https://campaign-api.autonomous.ai/api/v1/ai/v1"
        );
        assert_eq!(
            openai_base("https://campaign-api.autonomous.ai/api/v1/ai/v1"),
            "https://campaign-api.autonomous.ai/api/v1/ai/v1"
        );
        assert_eq!(
            openai_base("https://api.openai.com/v1/"),
            "https://api.openai.com/v1"
        );
        assert_eq!(
            openai_base("https://example.com/ai"),
            "https://example.com/ai"
        );
    }

    #[test]
    fn errors_say_the_step_and_nothing_else() {
        assert_eq!(Error::Status(401).to_string(), "service answered HTTP 401");
        assert_eq!(Error::Timeout("connect").to_string(), "connect timed out");
    }
}
