//! A small, blocking HTTP/1.1 client over loopback TCP: enough for Hermes'
//! API server (aiohttp): one request per connection (`Connection: close`),
//! `Content-Length`, chunked or close-delimited bodies, and an incremental
//! body reader for Server-Sent Events. No TLS: the server is bound to
//! 127.0.0.1 only.

use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use serde_json::Value;

/// Why a call failed below HTTP.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum HttpError {
    /// Nothing was sent: no address, refused, or no connect in time. The
    /// server never saw the request.
    Connect(String),
    /// Sent (or partly), then the connection broke or timed out: the server
    /// may have acted on it.
    Io(String),
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpError::Connect(why) => write!(f, "connect: {why}"),
            HttpError::Io(why) => write!(f, "{why}"),
        }
    }
}

/// One whole response.
#[derive(Clone, Debug)]
pub(crate) struct Resp {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Resp {
    #[allow(dead_code)] // read by tests and the replay header check
    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        header_of(&self.headers, name)
    }

    pub(crate) fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }

    /// The server's own words for an error (`error.message`, `error.code`),
    /// never the request's headers.
    pub(crate) fn error_words(&self) -> String {
        let v = self.json();
        let e = v.get("error").unwrap_or(&Value::Null);
        let message = e.get("message").and_then(Value::as_str).unwrap_or("");
        let code = e.get("code").and_then(Value::as_str).unwrap_or("");
        match (code.is_empty(), message.is_empty()) {
            (false, false) => format!("HTTP {} {code}: {message}", self.status),
            (false, true) => format!("HTTP {} {code}", self.status),
            (true, false) => format!("HTTP {}: {message}", self.status),
            (true, true) => format!("HTTP {}", self.status),
        }
    }
}

fn header_of<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// `http://host:port` (any path after it ignored) as a socket address and
/// the `Host` header's value.
pub(crate) fn authority(base: &str) -> Result<(SocketAddr, String), HttpError> {
    let rest = base
        .strip_prefix("http://")
        .ok_or_else(|| HttpError::Connect(format!("only http:// is spoken here, not {base}")))?;
    let host = rest.split('/').next().unwrap_or(rest);
    let host = if host.contains(':') {
        host.to_owned()
    } else {
        format!("{host}:80")
    };
    let addr = host
        .to_socket_addrs()
        .map_err(|e| HttpError::Connect(format!("{host}: {e}")))?
        .next()
        .ok_or_else(|| HttpError::Connect(format!("{host}: no address")))?;
    Ok((addr, host))
}

fn left(deadline: Instant) -> Option<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
}

fn idle(e: &std::io::Error) -> bool {
    matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)
}

/// A request to send.
pub(crate) struct Req<'a> {
    pub method: &'a str,
    pub path: &'a str,
    /// Extra headers (`Authorization`, `Idempotency-Key`, ...).
    pub headers: Vec<(&'a str, String)>,
    pub body: Option<Vec<u8>>,
}

impl<'a> Req<'a> {
    pub(crate) fn new(method: &'a str, path: &'a str) -> Req<'a> {
        Req {
            method,
            path,
            headers: Vec::new(),
            body: None,
        }
    }

    pub(crate) fn header(mut self, name: &'a str, value: impl Into<String>) -> Req<'a> {
        self.headers.push((name, value.into()));
        self
    }

    pub(crate) fn json(mut self, body: &Value) -> Req<'a> {
        self.body = Some(body.to_string().into_bytes());
        self.headers
            .push(("Content-Type", "application/json".to_owned()));
        self
    }
}

/// Connects and writes `req`; the stream is left to read the response.
fn open(base: &str, req: &Req<'_>, deadline: Instant) -> Result<TcpStream, HttpError> {
    let (addr, host) = authority(base)?;
    let wait = left(deadline).ok_or_else(|| HttpError::Connect("no time to connect".to_owned()))?;
    let mut stream = TcpStream::connect_timeout(&addr, wait)
        .map_err(|e| HttpError::Connect(format!("{host}: {e}")))?;
    let _ = stream.set_nodelay(true);
    let _ = stream.set_write_timeout(Some(wait));
    let mut head = format!(
        "{} {} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nUser-Agent: lamp-tasks\r\n",
        req.method, req.path
    );
    for (k, v) in &req.headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    let body = req.body.as_deref().unwrap_or(&[]);
    if req.body.is_some() || req.method == "POST" {
        head.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    head.push_str("\r\n");
    let mut out = head.into_bytes();
    out.extend_from_slice(body);
    stream
        .write_all(&out)
        .and_then(|_| stream.flush())
        .map_err(|e| HttpError::Io(format!("writing the request: {e}")))?;
    Ok(stream)
}

/// A response head: its status, its headers, and the bytes read past it.
type Head = (u16, Vec<(String, String)>, Vec<u8>);

/// The response head, and the bytes read past it.
fn read_head(stream: &mut TcpStream, deadline: Instant) -> Result<Head, HttpError> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        if let Some(at) = find(&buf, b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..at]).into_owned();
            let rest = buf[at + 4..].to_vec();
            let mut lines = head.split("\r\n");
            let status_line = lines.next().unwrap_or("");
            let status = status_line
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse::<u16>().ok())
                .ok_or_else(|| HttpError::Io(format!("not an HTTP reply: {status_line:.60}")))?;
            let headers = lines
                .filter_map(|l| l.split_once(':'))
                .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
                .collect();
            return Ok((status, headers, rest));
        }
        if buf.len() > 64 * 1024 {
            return Err(HttpError::Io("the reply head is too long".to_owned()));
        }
        let wait = left(deadline).ok_or_else(|| HttpError::Io("no reply in time".to_owned()))?;
        let _ = stream.set_read_timeout(Some(wait));
        match stream.read(&mut chunk) {
            Ok(0) => return Err(HttpError::Io("closed before a reply".to_owned())),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) if idle(&e) => {}
            Err(e) => return Err(HttpError::Io(format!("reading the reply: {e}"))),
        }
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// One request, its whole response read before `deadline`.
pub(crate) fn call(base: &str, req: &Req<'_>, deadline: Instant) -> Result<Resp, HttpError> {
    let mut stream = open(base, req, deadline)?;
    let (status, headers, rest) = read_head(&mut stream, deadline)?;
    let mut body = Body::new(stream, &headers, rest);
    let mut out = Vec::new();
    loop {
        let wait =
            left(deadline).ok_or_else(|| HttpError::Io("no whole reply in time".to_owned()))?;
        match body.read_some(wait) {
            Ok(Some(bytes)) => out.extend_from_slice(&bytes),
            Ok(None) => break,
            Err(Chunk::Idle) => {}
            Err(Chunk::Broken(why)) => return Err(HttpError::Io(why)),
        }
    }
    Ok(Resp {
        status,
        headers,
        body: out,
    })
}

/// A response whose body is read as it comes (an SSE stream).
pub(crate) struct Streaming {
    pub status: u16,
    #[allow(dead_code)] // kept for the journal and tests
    pub headers: Vec<(String, String)>,
    pub body: Body,
}

/// Opens `req` and reads only the head before `deadline`.
pub(crate) fn stream(base: &str, req: &Req<'_>, deadline: Instant) -> Result<Streaming, HttpError> {
    let mut stream = open(base, req, deadline)?;
    let (status, headers, rest) = read_head(&mut stream, deadline)?;
    Ok(Streaming {
        status,
        body: Body::new(stream, &headers, rest),
        headers,
    })
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Chunk {
    /// Nothing within the wait; the body goes on.
    Idle,
    Broken(String),
}

enum Framing {
    /// `Content-Length` bytes left.
    Length(usize),
    Chunked,
    /// Until the server closes.
    Close,
}

/// A response body, decoded incrementally.
pub(crate) struct Body {
    stream: Option<TcpStream>,
    framing: Framing,
    /// Bytes read from the socket and not yet decoded.
    raw: Vec<u8>,
    /// Chunked: bytes of the current chunk still to come.
    chunk_left: usize,
    /// Chunked: the CRLF after a chunk's data is still to come.
    chunk_crlf: bool,
    done: bool,
    /// Decoded bytes not yet split into lines ([`Self::next_line`]).
    pending: Vec<u8>,
}

impl Body {
    fn new(stream: TcpStream, headers: &[(String, String)], rest: Vec<u8>) -> Body {
        let chunked = header_of(headers, "transfer-encoding")
            .is_some_and(|v| v.to_ascii_lowercase().contains("chunked"));
        let framing = if chunked {
            Framing::Chunked
        } else if let Some(n) =
            header_of(headers, "content-length").and_then(|v| v.parse::<usize>().ok())
        {
            Framing::Length(n)
        } else {
            Framing::Close
        };
        Body::with(Some(stream), framing, rest)
    }

    fn with(stream: Option<TcpStream>, framing: Framing, raw: Vec<u8>) -> Body {
        Body {
            stream,
            framing,
            raw,
            chunk_left: 0,
            chunk_crlf: false,
            done: false,
            pending: Vec::new(),
        }
    }

    /// Decodes what `raw` holds; `Ok(bytes)` (maybe empty) or a framing error.
    fn decode(&mut self) -> Result<Vec<u8>, Chunk> {
        let mut out = Vec::new();
        if self.done {
            // Past the body's end (a chunked trailer's CRLF): nothing more.
            self.raw.clear();
            return Ok(out);
        }
        match &mut self.framing {
            Framing::Length(n) => {
                let take = (*n).min(self.raw.len());
                out.extend(self.raw.drain(..take));
                *n -= take;
                if *n == 0 {
                    self.done = true;
                }
            }
            Framing::Close => out.append(&mut self.raw),
            Framing::Chunked => loop {
                if self.chunk_crlf {
                    if self.raw.len() < 2 {
                        break;
                    }
                    if &self.raw[..2] != b"\r\n" {
                        return Err(Chunk::Broken("bad chunk end".to_owned()));
                    }
                    self.raw.drain(..2);
                    self.chunk_crlf = false;
                }
                if self.chunk_left == 0 {
                    let Some(at) = find(&self.raw, b"\r\n") else {
                        break;
                    };
                    let line = String::from_utf8_lossy(&self.raw[..at]).into_owned();
                    let size = line.split(';').next().unwrap_or("").trim();
                    let size = usize::from_str_radix(size, 16)
                        .map_err(|_| Chunk::Broken(format!("bad chunk size {size:.20}")))?;
                    self.raw.drain(..at + 2);
                    if size == 0 {
                        self.done = true;
                        self.raw.clear();
                        break;
                    }
                    self.chunk_left = size;
                }
                if self.raw.is_empty() {
                    break;
                }
                let take = self.chunk_left.min(self.raw.len());
                out.extend(self.raw.drain(..take));
                self.chunk_left -= take;
                if self.chunk_left == 0 {
                    self.chunk_crlf = true;
                }
            },
        }
        Ok(out)
    }

    /// The next decoded bytes within `wait`: `Ok(None)` at the body's end.
    pub(crate) fn read_some(&mut self, wait: Duration) -> Result<Option<Vec<u8>>, Chunk> {
        loop {
            let out = self.decode()?;
            if !out.is_empty() {
                return Ok(Some(out));
            }
            if self.done {
                return Ok(None);
            }
            let Some(stream) = self.stream.as_mut() else {
                return match self.framing {
                    Framing::Close => Ok(None),
                    _ => Err(Chunk::Broken("the body ended early".to_owned())),
                };
            };
            let _ = stream.set_read_timeout(Some(wait.max(Duration::from_millis(1))));
            let mut buf = [0u8; 8192];
            match stream.read(&mut buf) {
                Ok(0) => {
                    self.stream = None;
                    if matches!(self.framing, Framing::Close) {
                        self.done = true;
                        return Ok(None);
                    }
                    return Err(Chunk::Broken("closed mid-body".to_owned()));
                }
                Ok(n) => self.raw.extend_from_slice(&buf[..n]),
                Err(e) if idle(&e) => return Err(Chunk::Idle),
                Err(e) => return Err(Chunk::Broken(format!("read: {e}"))),
            }
        }
    }

    /// The next line (no `\r\n`/`\n`) within `wait`: `Ok(None)` at the end.
    pub(crate) fn next_line(&mut self, wait: Duration) -> Result<Option<String>, Chunk> {
        loop {
            if let Some(at) = self.pending.iter().position(|&b| b == b'\n') {
                let mut line: Vec<u8> = self.pending.drain(..=at).collect();
                line.pop();
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                return Ok(Some(String::from_utf8_lossy(&line).into_owned()));
            }
            match self.read_some(wait)? {
                Some(bytes) => self.pending.extend_from_slice(&bytes),
                None if self.pending.is_empty() => return Ok(None),
                None => {
                    let line = std::mem::take(&mut self.pending);
                    return Ok(Some(String::from_utf8_lossy(&line).into_owned()));
                }
            }
        }
    }
}

/// One Server-Sent Event, or a comment line (`: keepalive`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Sse {
    Event { id: Option<String>, data: String },
    Comment(String),
}

/// Folds SSE lines into events (the `data:` lines of one event joined by
/// `\n`, dispatched at the blank line).
#[derive(Default)]
pub(crate) struct SseParser {
    id: Option<String>,
    data: Vec<String>,
}

impl SseParser {
    pub(crate) fn line(&mut self, line: &str) -> Option<Sse> {
        if line.is_empty() {
            if self.data.is_empty() {
                self.id = None;
                return None;
            }
            return Some(Sse::Event {
                id: self.id.take(),
                data: std::mem::take(&mut self.data).join("\n"),
            });
        }
        if let Some(c) = line.strip_prefix(':') {
            return Some(Sse::Comment(c.trim().to_owned()));
        }
        let (field, value) = match line.split_once(':') {
            Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
            None => (line, ""),
        };
        match field {
            "data" => self.data.push(value.to_owned()),
            "id" => self.id = Some(value.to_owned()),
            _ => {}
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_all(raw: &[u8], framing: Framing) -> Result<Vec<u8>, Chunk> {
        let mut body = Body::with(None, framing, Vec::new());
        let mut out = Vec::new();
        // Byte by byte: every split point of the framing is crossed.
        for b in raw {
            body.raw.push(*b);
            out.extend(body.decode()?);
        }
        Ok(out)
    }

    #[test]
    fn chunked_bodies_decode_at_every_split() {
        let raw = b"5\r\nhello\r\n1;ext=1\r\n \r\n6\r\nworld!\r\n0\r\n\r\n";
        assert_eq!(decode_all(raw, Framing::Chunked).unwrap(), b"hello world!");
        assert!(decode_all(b"zz\r\n", Framing::Chunked).is_err());
    }

    #[test]
    fn length_bodies_stop_at_their_length() {
        assert_eq!(decode_all(b"abcdef", Framing::Length(3)).unwrap(), b"abc");
    }

    #[test]
    fn sse_lines_fold_into_events_and_comments() {
        let mut p = SseParser::default();
        let mut got = Vec::new();
        for l in [
            ": open",
            "",
            "id: 3",
            "data: {\"a\":1}",
            "",
            "data: x",
            "data: y",
            "",
        ] {
            got.extend(p.line(l));
        }
        assert_eq!(
            got,
            vec![
                Sse::Comment("open".to_owned()),
                Sse::Event {
                    id: Some("3".to_owned()),
                    data: "{\"a\":1}".to_owned()
                },
                Sse::Event {
                    id: None,
                    data: "x\ny".to_owned()
                },
            ]
        );
    }

    #[test]
    fn only_http_on_an_address() {
        assert!(authority("ws://127.0.0.1:1").is_err());
        let (addr, host) = authority("http://127.0.0.1:8642").unwrap();
        assert_eq!(addr.port(), 8642);
        assert_eq!(host, "127.0.0.1:8642");
    }
}
