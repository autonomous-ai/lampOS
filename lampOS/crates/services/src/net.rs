//! Requests to a service: HTTPS only, to the service's own hosts only, no
//! redirect followed, bounded in time and size. The token is put on a call
//! after its guard has passed, and never comes back in an error.
use crate::catalog::{self, Method};
use std::{fmt, future::Future, sync::Arc, time::Duration};

/// How long one request may take, its connection included.
pub const WITHIN: Duration = Duration::from_secs(15);
/// How long connecting may take.
pub const CONNECT_WITHIN: Duration = Duration::from_secs(5);
/// The most of an answer's body read.
pub const BODY_MOST: usize = 1 << 20;

/// TLS 1.2 and 1.3 with the `ring` provider and the webpki roots, as the
/// other lampOS clients have it.
pub fn tls() -> Result<rustls::ClientConfig, String> {
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    Ok(rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|_| "TLS setup".to_owned())?
        .with_root_certificates(roots)
        .with_no_client_auth())
}

/// One request. Its `Debug` names the headers, never their values.
#[derive(Clone, PartialEq, Eq)]
pub struct Call {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    /// The most of the answer's body read: [`BODY_MOST`], or more for one
    /// call that needs it (a PDF attachment's data, `mail::PDF_FETCH`).
    pub most: usize,
}

impl fmt::Debug for Call {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Call")
            .field("method", &self.method)
            .field("url", &self.url.split('?').next().unwrap_or(""))
            .field(
                "headers",
                &self
                    .headers
                    .iter()
                    .map(|(name, _)| name)
                    .collect::<Vec<_>>(),
            )
            .field("body", &self.body.as_ref().map(Vec::len))
            .finish()
    }
}

/// The answer headers kept: what an MCP answer is (`content-type`) and
/// the session it opened (`mcp-session-id`). No other header is read.
pub const KEPT_HEADERS: &[&str] = &["content-type", "mcp-session-id"];
/// The longest kept header value.
pub const HEADER_MOST: usize = 256;

/// An answer: its status, the headers in [`KEPT_HEADERS`] (names in lower
/// case) and its body, at most [`BODY_MOST`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
    pub headers: Vec<(String, String)>,
}

impl Reply {
    /// A kept header's value, by its lower-case name.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(kept, _)| kept == name)
            .map(|(_, value)| value.as_str())
    }
}

/// What sends a call; a fake in tests.
pub trait Http {
    fn send(&self, call: Call) -> impl Future<Output = Result<Reply, String>>;
}

/// A URL's host and path, if it is one a token may go with: `https`, a
/// host, no user or password, the default port, no fragment.
pub fn split(url: &str) -> Result<(String, String), &'static str> {
    let parsed = reqwest::Url::parse(url).map_err(|_| "not a URL")?;
    if parsed.scheme() != "https" {
        return Err("only https is used");
    }
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.port().is_some_and(|port| port != 443)
        || parsed.fragment().is_some()
    {
        return Err("no user, port or fragment in a service's URL");
    }
    let host = parsed.host_str().ok_or("no host")?.to_owned();
    Ok((host, parsed.path().to_owned()))
}

/// Whether `call` may carry `code`'s token: to one of its hosts over https,
/// and a read on its allow-list or a write on its write list.
pub fn guard(code: &str, call: &Call, write: bool) -> Result<(), String> {
    let (host, path) = split(&call.url)?;
    if !catalog::host_allowed(code, &host) {
        return Err(format!(
            "the lamp sends a {code} token only to its own hosts"
        ));
    }
    if write {
        if !catalog::write_allowed(code, call.method, &path) {
            return Err("the lamp doesn't make that write".into());
        }
        return Ok(());
    }
    let body = call
        .body
        .as_deref()
        .and_then(|body| serde_json::from_slice(body).ok());
    catalog::read_allowed(code, call.method, &path, body.as_ref())
}

/// A secret's every spelling in `text` replaced (V1's `scrub`).
pub fn scrub(text: &str, secret: &str) -> String {
    if secret.is_empty() {
        return text.to_owned();
    }
    let encoded: String = secret
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect();
    text.replace(secret, "[credential]")
        .replace(&encoded, "[credential]")
}

/// The client every service call is made with: no redirect followed (one
/// comes back as its own status), bounded in time, its own TLS. Only a
/// test against a local server passes `https_only` false.
pub fn client(https_only: bool) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .tls_backend_preconfigured(tls()?)
        .https_only(https_only)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(WITHIN)
        .connect_timeout(CONNECT_WITHIN)
        .build()
        .map_err(|_| "HTTPS client setup".to_owned())
}

/// `call` sent on `client`: its answer's status, kept headers and at most
/// `call.most` bytes of its body. An error never names the URL.
pub async fn send_on(client: &reqwest::Client, call: Call) -> Result<Reply, String> {
    let most = call.most;
    let method = match call.method {
        Method::Get => reqwest::Method::GET,
        Method::Post => reqwest::Method::POST,
        Method::Patch => reqwest::Method::PATCH,
        Method::Put => reqwest::Method::PUT,
    };
    let mut request = client.request(method, &call.url);
    for (name, value) in &call.headers {
        request = request.header(name, value);
    }
    if let Some(body) = call.body {
        request = request.body(body);
    }
    // reqwest's errors name the URL: dropped whole.
    let mut response = tokio::time::timeout(WITHIN, request.send())
        .await
        .map_err(|_| "the service didn't answer in time".to_owned())?
        .map_err(|_| "the service couldn't be reached".to_owned())?;
    let status = response.status().as_u16();
    let headers = KEPT_HEADERS
        .iter()
        .filter_map(|name| {
            let value = response.headers().get(*name)?.to_str().ok()?;
            (value.len() <= HEADER_MOST).then(|| ((*name).to_owned(), value.to_owned()))
        })
        .collect();
    let mut body = Vec::new();
    while let Some(chunk) = tokio::time::timeout(WITHIN, response.chunk())
        .await
        .map_err(|_| "the service didn't answer in time".to_owned())?
        .map_err(|_| "the service's answer broke off".to_owned())?
    {
        let room = most.saturating_sub(body.len());
        body.extend_from_slice(&chunk[..chunk.len().min(room)]);
        if body.len() >= most {
            break;
        }
    }
    Ok(Reply {
        status,
        body,
        headers,
    })
}

/// The real sender: a client made for each call (no connection outlives
/// the request's own runtime), https only, no redirect.
#[derive(Clone, Copy, Debug, Default)]
pub struct Https;

impl Http for Https {
    async fn send(&self, call: Call) -> Result<Reply, String> {
        send_on(&client(true)?, call).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(method: Method, url: &str) -> Call {
        Call {
            method,
            url: url.into(),
            headers: vec![("authorization".into(), "Bearer planted-4710".into())],
            body: None,
            most: BODY_MOST,
        }
    }

    #[test]
    fn a_token_goes_only_over_https_to_its_hosts() {
        assert!(
            guard(
                "github",
                &call(Method::Get, "https://api.github.com/user"),
                false
            )
            .is_ok()
        );
        for url in [
            "http://api.github.com/user",
            "https://api.github.com:8443/user",
            "https://me@api.github.com/user",
            "https://api.github.com.evil.test/user",
            "https://evil.test/user?host=api.github.com",
            "https://api.linear.app/graphql",
            "https://api.github.com/user#x",
        ] {
            assert!(
                guard("github", &call(Method::Get, url), false).is_err(),
                "{url}"
            );
        }
        // A read route isn't a write route, nor the other way.
        let issue = call(Method::Post, "https://api.github.com/repos/o/r/issues");
        assert!(guard("github", &issue, false).is_err());
        assert!(guard("github", &issue, true).is_ok());
        assert!(
            guard(
                "github",
                &call(Method::Get, "https://api.github.com/user"),
                true
            )
            .is_err()
        );
    }

    #[test]
    fn a_calls_debug_shows_no_header_value_or_query() {
        let mut shown = call(
            Method::Get,
            "https://api.github.com/user?access_token=planted-q",
        );
        shown.body = Some(b"planted-body".to_vec());
        let text = format!("{shown:?}");
        assert!(!text.contains("planted"), "{text}");
        assert!(text.contains("authorization"));
    }

    #[test]
    fn scrub_takes_every_spelling_of_the_secret() {
        let text = "a planted/4710+x b planted%2F4710%2Bx";
        assert_eq!(
            scrub(text, "planted/4710+x"),
            "a [credential] b [credential]"
        );
        assert_eq!(scrub("x", ""), "x");
    }

    /// The shared sender honors the per-call cap, including Spotify's PUT
    /// path; the old fixed BODY_MOST would return all ten bytes here.
    #[tokio::test]
    async fn shared_sender_caps_get_and_put_response_bodies() {
        use std::io::{Read, Write};
        use std::net::TcpListener;

        for method in [Method::Get, Method::Put] {
            let identity = tempfile::tempdir().unwrap();
            let path = format!(
                "/bounded-{}",
                identity.path().file_name().unwrap().to_str().unwrap()
            );
            let owned_path = path.clone();
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            listener.set_nonblocking(true).unwrap();
            let server = std::thread::spawn(move || {
                use std::time::{Duration, Instant};
                let deadline = Instant::now() + Duration::from_secs(5);
                while Instant::now() < deadline {
                    let mut peer = match listener.accept() {
                        Ok((peer, _)) => peer,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(5));
                            continue;
                        }
                        Err(error) => panic!("accept failed: {error}"),
                    };
                    // A foreign discovery connection cannot occupy this fixture
                    // indefinitely or count as the request under test.
                    let header_deadline = deadline.min(Instant::now() + Duration::from_millis(250));
                    let mut request = Vec::new();
                    while request.len() < 8192 && !request.ends_with(b"\r\n\r\n") {
                        let left = header_deadline.saturating_duration_since(Instant::now());
                        if left.is_zero() {
                            break;
                        }
                        peer.set_read_timeout(Some(left)).unwrap();
                        let mut byte = [0];
                        if peer.read_exact(&mut byte).is_err() {
                            break;
                        }
                        request.push(byte[0]);
                    }
                    let target = request
                        .split(|byte| *byte == b'\n')
                        .next()
                        .and_then(|line| std::str::from_utf8(line).ok())
                        .and_then(|line| line.split_whitespace().nth(1));
                    if target != Some(owned_path.as_str()) {
                        continue;
                    }
                    assert!(request.ends_with(b"\r\n\r\n"), "owned headers incomplete");
                    let left = deadline.saturating_duration_since(Instant::now());
                    assert!(!left.is_zero(), "owned response missed deadline");
                    peer.set_write_timeout(Some(left)).unwrap();
                    peer.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\n0123456789").unwrap();
                    return request;
                }
                panic!("owned request missing before fixture deadline");
            });
            // An unrelated request arriving first must not satisfy the test.
            let mut probe = std::net::TcpStream::connect(address).unwrap();
            probe
                .set_write_timeout(Some(std::time::Duration::from_secs(1)))
                .unwrap();
            probe
                .write_all(b"GET /discovery HTTP/1.1\r\nHost: localhost\r\n\r\n")
                .unwrap();
            drop(probe);
            let mut request = call(method, &format!("http://{address}{path}"));
            request.most = 3;
            let reply = send_on(&client(false).unwrap(), request).await;
            // Join even if the client failed: the server's own deadline bounds
            // cleanup and prevents a detached fixture from reaching later tests.
            let wire = server.join().unwrap();
            let reply = reply.unwrap();
            assert_eq!(reply.status, 200);
            assert_eq!(reply.body, b"012");
            let verb = match method {
                Method::Get => "GET",
                Method::Put => "PUT",
                _ => unreachable!(),
            };
            assert!(wire.starts_with(format!("{verb} {path} ").as_bytes()));
        }
    }
}
