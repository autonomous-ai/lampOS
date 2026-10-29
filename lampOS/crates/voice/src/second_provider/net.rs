//! The second route's network clients, speaking V1's protocols: a
//! streaming speech-to-text websocket (Deepgram or the Autonomous proxy),
//! a streamed OpenAI-compatible chat completion, and streamed speech from
//! OpenAI or ElevenLabs. Every connect and read is bounded, and no error
//! carries a key, a URL or a server's words.
use std::{fmt, sync::Arc, time::Duration};

mod chat;
mod gemini_tts;
pub mod key_terms;
pub mod routes;
pub mod search;
mod sentences;
mod stt;
mod tts;
pub mod weather;

pub use chat::{
    Chat, ChatEvent, ChatStream, Declare, Force, Forced, GEMINI_REASONING, Message, Narrow,
    ToolCall, cache_counts,
};
pub use gemini_tts::{GeminiSpeech, GeminiTts};
pub use sentences::Sentences;
pub use stt::{Early, QUIET_HELD_ENVIRONMENT, Replies, SttStream, check as stt_check};
pub use tts::{PACKET_SAMPLES, TTS_SAMPLE_RATE, Tts, TtsStream, Turn, default_model, voice_name};
/// A piece of speech's request as `Tts` makes it, for `probe-cascade --wire
/// --speech`.
pub(crate) use tts::{
    Request as SpeechRequest, SocketRequest, on_elevenlabs, request as speech_request,
    socket_answer, socket_piece, socket_request,
};
#[cfg(test)]
pub(crate) use tts::{SocketShape, socket_end, socket_opening};

/// To open a connection, TLS included.
pub(crate) const CONNECT: Duration = Duration::from_secs(10);
/// For a streamed response to say anything more. A chat model thinking
/// before its first word is the slowest case seen in V1 (6-19 s).
pub(crate) const STALL: Duration = Duration::from_secs(30);

/// `LAMP_SECOND_CHAT_KEPT=on` (#2602, the first builder's 02:14 on 23 Oct):
/// the chat client keeps an idle connection in its pool at most
/// [`CHAT_POOL_IDLE`], not reqwest's 90 s. TCP keepalive is reqwest's own
/// either way (15 s, then 3 probes 15 s apart, and a 30 s TCP user timeout on
/// Linux), so this changes only how long a connection is reused. On
/// lamp-4ace on 22 Oct three chat requests never reached the relay (Backend's
/// 02:09), which fits a request written onto a pooled connection the path had
/// already dropped (a home router or the relay's front may drop an idle one
/// sooner than 90 s); that cause isn't shown yet.
/// `on`, and nothing else; the other clients are unchanged.
pub const CHAT_KEPT_ENVIRONMENT: &str = "LAMP_SECOND_CHAT_KEPT";
/// How long an idle chat connection is kept with [`CHAT_KEPT_ENVIRONMENT`],
/// counted from when it went back to the pool.
pub const CHAT_POOL_IDLE: Duration = Duration::from_secs(25);

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
pub(crate) fn tls() -> Result<rustls::ClientConfig> {
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
    client_kept(None)
}

/// The chat's client: [`http_client`], or with [`CHAT_KEPT_ENVIRONMENT`] on,
/// idle connections kept at most [`CHAT_POOL_IDLE`]; and whether it is kept
/// so.
fn chat_client() -> Result<(reqwest::Client, bool)> {
    let kept = chat_kept(std::env::var(CHAT_KEPT_ENVIRONMENT).ok().as_deref());
    Ok((client_kept(kept.then_some(CHAT_POOL_IDLE))?, kept))
}

/// Whether [`CHAT_KEPT_ENVIRONMENT`]'s value keeps the chat's connections
/// briefly: `on`, and nothing else.
fn chat_kept(value: Option<&str>) -> bool {
    value == Some("on")
}

/// [`http_client`] keeping no idle connection, so each request opens its
/// own: a speech piece asked again after it failed to start is never sent on
/// the connection that may have held it (#4338, lamp-3e83 on 29 Oct).
pub(crate) fn fresh_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .tls_backend_preconfigured(tls()?)
        .https_only(!cfg!(test))
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(CONNECT)
        .read_timeout(STALL)
        .pool_max_idle_per_host(0)
        .build()
        .map_err(|_| Error::Transport("HTTPS client setup"))
}

/// [`http_client`], with `idle` as its pool's idle limit when given, and then
/// at most one idle connection per host, so a warm still running when a chat
/// goes can't leave two pooled (El's P3 on #3115).
fn client_kept(idle: Option<Duration>) -> Result<reqwest::Client> {
    let builder = reqwest::Client::builder()
        .tls_backend_preconfigured(tls()?)
        .https_only(!cfg!(test))
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(CONNECT)
        .read_timeout(STALL);
    match idle {
        Some(idle) => builder.pool_idle_timeout(idle).pool_max_idle_per_host(1),
        None => builder,
    }
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
        let status = response.status().as_u16();
        // Why it refused, as the service's codes alone (#1712: Gemini's chat
        // answered HTTP 400 ten times out of ten on 21 Oct, and nothing said
        // whether for the key, the model or the request).
        let codes = refusal_codes(&refusal_head(response).await);
        if codes.is_empty() {
            eprintln!("lamp-voice second: a service answered HTTP {status}, naming no code");
        } else {
            eprintln!(
                "lamp-voice second: a service answered HTTP {status} ({})",
                codes.join(", ")
            );
        }
        return Err(Error::Status(status));
    }
    Ok(response)
}

/// The most of a refusal's body [`refusal_head`] reads, and how long it waits.
const REFUSAL_BYTES: usize = 4 << 10;
const REFUSAL_WAIT: Duration = Duration::from_secs(1);
/// The most codes, and the longest, [`refusal_codes`] keeps.
const MOST_CODES: usize = 4;
const CODE_CHARS: usize = 40;
/// More digits than this and it's taken for a key, not a code: DeepSeek's
/// and Deepgram's keys are hex in one case, and no service's code has more
/// than a few (the reviewer on #2397).
const CODE_DIGITS: usize = 4;

/// A refusal's body, up to [`REFUSAL_BYTES`] or [`REFUSAL_WAIT`], whichever
/// comes first; empty if it breaks.
async fn refusal_head(mut response: reqwest::Response) -> Vec<u8> {
    let mut head = Vec::new();
    let _ = tokio::time::timeout(REFUSAL_WAIT, async {
        while head.len() < REFUSAL_BYTES {
            match response.chunk().await {
                Ok(Some(piece)) => head.extend_from_slice(&piece),
                _ => break,
            }
        }
    })
    .await;
    head.truncate(REFUSAL_BYTES);
    head
}

/// The codes a refusal names: Google's `error.status` and each
/// `error.details[].reason`, OpenAI's `error.code` and `error.type`. A
/// value is kept only when it reads as a code, in one case, of letters,
/// digits and `_`, `.` or `-`, at most [`CODE_CHARS`] and at most
/// [`CODE_DIGITS`] digits, so that no server's words reach the journal, nor
/// a string shaped like a key (mixed case, or many digits); at most
/// [`MOST_CODES`].
fn refusal_codes(body: &[u8]) -> Vec<String> {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return Vec::new();
    };
    // Google's OpenAI-compatible endpoint answers a refusal as a one-item
    // array, `[{"error": …}]` (the backend on #2133, 21 Oct).
    let error = match &value {
        serde_json::Value::Array(items) => {
            &items.first().unwrap_or(&serde_json::Value::Null)["error"]
        }
        _ => &value["error"],
    };
    let reasons = error["details"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|detail| &detail["reason"]);
    let mut codes = Vec::new();
    for candidate in [&error["status"], &error["code"], &error["type"]]
        .into_iter()
        .chain(reasons)
    {
        let Some(code) = candidate.as_str() else {
            continue;
        };
        let one_case = !code.chars().any(|c| c.is_ascii_lowercase())
            || !code.chars().any(|c| c.is_ascii_uppercase());
        let a_code = (1..=CODE_CHARS).contains(&code.len())
            && code.chars().any(|c| c.is_ascii_alphabetic())
            && code
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
            && one_case
            && code.chars().filter(|c| c.is_ascii_digit()).count() <= CODE_DIGITS;
        if a_code && !codes.iter().any(|kept| kept == code) && codes.len() < MOST_CODES {
            codes.push(code.to_owned());
        }
    }
    codes
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

/// The local socket a response came on: the same one again means the
/// connection was reused from the client's pool. HTTP/1.1 only here (no
/// `http2` feature), so one connection carries one request at a time.
pub(crate) fn local_socket(response: &reqwest::Response) -> Option<std::net::SocketAddr> {
    response
        .extensions()
        .get::<hyper_util::client::legacy::connect::HttpInfo>()
        .map(hyper_util::client::legacy::connect::HttpInfo::local_addr)
}

/// A connection opened ahead with a `GET` of `url` (#1712), unless `used`
/// says one went within `after`: at most `max` bytes of the answer are read,
/// so the connection goes back to the pool. The status, how long it took,
/// the connection's local socket, whether its answer was read to its end and
/// whether it keeps its connection ([`keeps_its_connection`]; only with both
/// is the connection kept); `None` when not due, or when it failed.
pub(crate) async fn warm_get(
    client: &reqwest::Client,
    used: &std::sync::Mutex<Option<std::time::Instant>>,
    after: Duration,
    url: &str,
    auth: (&'static str, &str),
    max: usize,
) -> Option<(u16, Duration, Option<std::net::SocketAddr>, bool, bool)> {
    let began = std::time::Instant::now();
    {
        let mut used = used.lock().ok()?;
        if used.is_some_and(|at| began.duration_since(at) < after) {
            return None;
        }
        *used = Some(began);
    }
    let request = client.get(url).header(auth.0, auth.1);
    let mut response = tokio::time::timeout(CONNECT + STALL, request.send())
        .await
        .ok()?
        .ok()?;
    let status = response.status().as_u16();
    let socket = local_socket(&response);
    let keeps = keeps_its_connection(&response);
    let mut read = 0;
    let mut ended = false;
    while read <= max {
        match tokio::time::timeout(STALL, response.chunk()).await {
            Ok(Ok(Some(bytes))) => read += bytes.len(),
            Ok(Ok(None)) => {
                ended = true;
                break;
            }
            _ => break,
        }
    }
    Some((status, began.elapsed(), socket, ended, keeps))
}

/// Whether `response` leaves its connection for another request: not with
/// `connection: close`.
pub(crate) fn keeps_its_connection(response: &reqwest::Response) -> bool {
    !response
        .headers()
        .get(reqwest::header::CONNECTION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("close"))
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
    fn a_refusal_names_its_codes_and_none_of_its_words() {
        // Google's answer to a key it doesn't know.
        let google = br#"{"error":{"code":400,"message":"API key not valid. Please pass a valid API key.","status":"INVALID_ARGUMENT","details":[{"@type":"type.googleapis.com/google.rpc.ErrorInfo","reason":"API_KEY_INVALID","domain":"googleapis.com"}]}}"#;
        assert_eq!(
            refusal_codes(google),
            ["INVALID_ARGUMENT", "API_KEY_INVALID"]
        );
        // OpenAI's form, as DeepSeek and OpenRouter answer.
        let openai = br#"{"error":{"message":"The model `x` does not exist","type":"invalid_request_error","code":"model_not_found"}}"#;
        assert_eq!(
            refusal_codes(openai),
            ["model_not_found", "invalid_request_error"]
        );
        // Words, a mixed-case string like a key, a long one, a number:
        // none is a code.
        let words = br#"{"error":{"status":"the key sk-ab12 was refused","code":"sk3XyZ9qLm2PwR8tVb7N","type":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA","details":[{"reason":42}]}}"#;
        assert!(refusal_codes(words).is_empty());
        // Keys in one case, as DeepSeek's and Deepgram's are: too many digits.
        let deepseek_like = br#"{"error":{"code":"sk-0123456789abcdef0123456789abcdef"}}"#;
        assert!(refusal_codes(deepseek_like).is_empty());
        let hex_40 = br#"{"error":{"status":"0123456789abcdef0123456789abcdef01234567"}}"#;
        assert!(refusal_codes(hex_40).is_empty());
        // Google's OpenAI-compatible endpoint, as an array of one.
        let array = br#"[{"error":{"code":400,"message":"Missing or invalid Authorization header.","status":"INVALID_ARGUMENT"}}]"#;
        assert_eq!(refusal_codes(array), ["INVALID_ARGUMENT"]);
        assert!(refusal_codes(b"[]").is_empty());
        // Not JSON, or JSON without an error.
        assert!(refusal_codes(b"<html>Bad Gateway</html>").is_empty());
        assert!(refusal_codes(br#"{"status":0,"message":"no"}"#).is_empty());
    }

    #[test]
    fn errors_say_the_step_and_nothing_else() {
        assert_eq!(Error::Status(401).to_string(), "service answered HTTP 401");
        assert_eq!(Error::Timeout("connect").to_string(), "connect timed out");
    }
}
