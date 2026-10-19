//! `lamp-voice probe-cascade PROVIDER_CONFIG SECOND_CONFIG [RUNS] --wire`:
//! where a chat request's time goes between the lamp and the service, for
//! DeepSeek through the relay and on its own API (#1934; the first builder
//! on #1722, 00:56). On lamp-4ace the relay's answers began about 0.6 s
//! later than DeepSeek's own. Each request is made by hand on a connection
//! of its own, and timed at each step:
//!
//! - **dns**, **tcp**, **tls**: the name looked up, the TCP connect, the
//!   TLS handshake;
//! - **send**: the request written, its body whole;
//! - **first byte**: from the request's last byte to the response's first;
//! - **events**: from the same point to the stream's first and second
//!   `data:` events, the gap between them, and to its first words or call;
//! - **done**: to the stream's end.
//!
//! Each run asks each target two requests: **lamp**, a spoken turn's size
//! (the character, a turn's changing part, and every function a turn
//! declares, the actions part's read from `LAMP_ACTIONS_DIR` as
//! `probe-tools` reads them; the route's own was 31 KB with 31 functions on
//! lamp-4ace), and **small**, one short line and no functions. A first byte
//! late by the same on both says the network or the relay's own wait; late
//! on the lamp's only, the request's size; events that come together after
//! a late first byte, the relay holding the stream back.
//!
//! The relay is `deepseek_base_url` (or `LAMP_SECOND_DEEPSEEK_BASE_URL`),
//! with the route's credential; DeepSeek's own API takes the unit's
//! `deepseek.key`. Each request is the route's own as `Chat` builds it,
//! `hosted.deepseek_model` with thinking off. It prints counts and
//! milliseconds only: never the words asked or answered, a key, a host or a
//! URL.
use super::super::net::{self, Chat, Declare, Message};
use super::{cache, percentile};
use crate::second_config::{CREDENTIALS_ENV, DEEPSEEK_CREDENTIAL, unit_credential};
use serde_json::Value;
use std::{
    io::{self, Read, Write},
    net::{TcpStream, ToSocketAddrs},
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

/// To connect, and for the answer to say anything more: the route's bounds
/// (`net::CONNECT`, `net::STALL`).
const CONNECT: Duration = net::CONNECT;
const STALL: Duration = net::STALL;
/// The most of an answer read: a sentence or two is far less.
const MOST_ANSWER: usize = 1 << 20;
/// The small request's system message.
pub const SMALL: &str = "You are a desk lamp. Answer in one short sentence.";

/// Which of the two requests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    Lamp,
    Small,
}

impl Size {
    pub const ALL: [Self; 2] = [Self::Lamp, Self::Small];

    pub fn name(self) -> &'static str {
        match self {
            Self::Lamp => "lamp",
            Self::Small => "small",
        }
    }

    /// The request's messages in `run`: a spoken turn's, with its own
    /// changing part each run, or one short line.
    pub fn messages(self, character: &str, run: u32) -> Vec<Message> {
        match self {
            Self::Lamp => vec![
                Message::system(format!("{character}\n\n{}", cache::changing(run))),
                Message::user(cache::QUESTION),
            ],
            Self::Small => vec![Message::system(SMALL), Message::user(cache::QUESTION)],
        }
    }
}

/// One request's times, in ms: the connection's steps, then from the
/// request's last byte written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Times {
    pub dns: u64,
    pub tcp: u64,
    pub tls: u64,
    pub send: u64,
    pub answer: Answer,
}

/// What the response said as it came, in ms from the request's last byte.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Answer {
    pub status: u16,
    pub first_byte: Option<u64>,
    pub first_event: Option<u64>,
    pub second_event: Option<u64>,
    /// The first event with words or a call.
    pub words: Option<u64>,
    pub done: u64,
    /// `data:` events before `[DONE]`.
    pub events: u32,
}

impl Answer {
    /// Between the stream's first two events.
    pub fn gap(&self) -> Option<u64> {
        Some(self.second_event?.saturating_sub(self.first_event?))
    }
}

fn ms(from: Instant) -> u64 {
    from.elapsed().as_millis().try_into().unwrap_or(u64::MAX)
}

fn shown(value: Option<u64>) -> String {
    value.map_or_else(|| "none".to_owned(), |v| v.to_string())
}

impl Times {
    /// The run's line, for a request of `bytes`.
    pub fn shown(&self, bytes: usize) -> String {
        let a = &self.answer;
        format!(
            "bytes={bytes} status={} dns_ms={} tcp_ms={} tls_ms={} send_ms={} first_byte_ms={} \
             first_event_ms={} second_event_ms={} gap_ms={} words_ms={} done_ms={} events={}",
            a.status,
            self.dns,
            self.tcp,
            self.tls,
            self.send,
            shown(a.first_byte),
            shown(a.first_event),
            shown(a.second_event),
            shown(a.gap()),
            shown(a.words),
            a.done,
            a.events
        )
    }
}

/// A chunked body's bytes, from its chunks as they come.
#[derive(Default)]
struct Chunked {
    line: Vec<u8>,
    left: usize,
    /// The line end after a chunk's data, still to skip.
    after: usize,
    ended: bool,
}

impl Chunked {
    fn feed(&mut self, mut bytes: &[u8], out: &mut Vec<u8>) -> io::Result<()> {
        while !bytes.is_empty() && !self.ended {
            if self.after > 0 {
                let skip = self.after.min(bytes.len());
                self.after -= skip;
                bytes = &bytes[skip..];
            } else if self.left > 0 {
                let take = self.left.min(bytes.len());
                out.extend_from_slice(&bytes[..take]);
                self.left -= take;
                bytes = &bytes[take..];
                if self.left == 0 {
                    self.after = 2;
                }
            } else {
                let Some(end) = bytes.iter().position(|&byte| byte == b'\n') else {
                    self.line.extend_from_slice(bytes);
                    if self.line.len() > 64 {
                        return Err(io::Error::other("chunk size line"));
                    }
                    return Ok(());
                };
                self.line.extend_from_slice(&bytes[..end]);
                bytes = &bytes[end + 1..];
                let line = String::from_utf8_lossy(&self.line).into_owned();
                let size = line.split(';').next().unwrap_or_default().trim();
                let size =
                    usize::from_str_radix(size, 16).map_err(|_| io::Error::other("chunk size"))?;
                self.line.clear();
                if size == 0 {
                    self.ended = true;
                } else {
                    self.left = size;
                }
            }
        }
        Ok(())
    }
}

/// One `data:` line of the stream.
#[derive(Debug, PartialEq, Eq)]
enum Event {
    /// An event; whether it has words or a call.
    Data(bool),
    Done,
}

/// The stream's `data:` lines, from its body's bytes as they come.
#[derive(Default)]
struct Events {
    line: Vec<u8>,
}

impl Events {
    fn feed(&mut self, bytes: &[u8]) -> Vec<Event> {
        let mut events = Vec::new();
        for &byte in bytes {
            if byte != b'\n' {
                if self.line.len() < MOST_ANSWER {
                    self.line.push(byte);
                }
                continue;
            }
            let line = std::mem::take(&mut self.line);
            let line = String::from_utf8_lossy(&line);
            let Some(data) = line.trim_end_matches('\r').strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data == "[DONE]" {
                events.push(Event::Done);
            } else {
                let delta = serde_json::from_str::<Value>(data)
                    .ok()
                    .map(|chunk| chunk["choices"][0]["delta"].clone())
                    .unwrap_or_default();
                let words = delta["content"]
                    .as_str()
                    .is_some_and(|text| !text.is_empty())
                    || delta["tool_calls"]
                        .as_array()
                        .is_some_and(|calls| !calls.is_empty());
                events.push(Event::Data(words));
            }
        }
        events
    }
}

/// Reads an HTTP/1.1 response's head and its event stream from `reader`,
/// timing each from `written`: up to `[DONE]`, the stream's end or an
/// answer other than 200, whose body is not read.
pub fn read_answer<R: Read + ?Sized>(reader: &mut R, written: Instant) -> io::Result<Answer> {
    let mut answer = Answer::default();
    let mut head = Vec::new();
    let mut chunked: Option<Chunked> = None;
    let mut in_head = true;
    let mut events = Events::default();
    let mut body = Vec::new();
    let mut read = 0usize;
    let mut buffer = vec![0u8; 16 << 10];
    'reading: loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        let at = ms(written);
        answer.first_byte.get_or_insert(at);
        read += n;
        if read > MOST_ANSWER {
            return Err(io::Error::other("answer too long"));
        }
        let mut bytes = &buffer[..n];
        let rest: Vec<u8>;
        if in_head {
            head.extend_from_slice(bytes);
            let Some(end) = head.windows(4).position(|four| four == b"\r\n\r\n") else {
                continue;
            };
            rest = head.split_off(end + 4);
            in_head = false;
            let text = String::from_utf8_lossy(&head).to_ascii_lowercase();
            answer.status = text
                .split_whitespace()
                .nth(1)
                .and_then(|code| code.parse().ok())
                .ok_or_else(|| io::Error::other("status line"))?;
            if answer.status != 200 {
                break;
            }
            if text
                .lines()
                .any(|line| line.starts_with("transfer-encoding:") && line.contains("chunked"))
            {
                chunked = Some(Chunked::default());
            }
            bytes = &rest;
        }
        body.clear();
        match chunked.as_mut() {
            Some(chunks) => chunks.feed(bytes, &mut body)?,
            None => body.extend_from_slice(bytes),
        }
        for event in events.feed(&body) {
            match event {
                Event::Done => break 'reading,
                Event::Data(words) => {
                    answer.events += 1;
                    match answer.events {
                        1 => answer.first_event = Some(at),
                        2 => answer.second_event = Some(at),
                        _ => {}
                    }
                    if words {
                        answer.words.get_or_insert(at);
                    }
                }
            }
        }
        if chunked.as_ref().is_some_and(|chunks| chunks.ended) {
            break;
        }
    }
    answer.done = ms(written);
    Ok(answer)
}

/// A step's failure, by its kind only: an error's own words may name the
/// host.
fn step(name: &'static str) -> impl Fn(io::Error) -> io::Error {
    move |error| io::Error::new(error.kind(), format!("{name}: {:?}", error.kind()))
}

/// `body` posted to `url` with `auth`, on a connection of its own, timed
/// at each step. HTTPS with `tls`; a plain `http` URL only in tests.
pub fn ask(
    url: &reqwest::Url,
    auth: &str,
    body: &[u8],
    tls: Option<Arc<rustls::ClientConfig>>,
) -> io::Result<Times> {
    let secure = url.scheme() == "https";
    if !secure && !cfg!(test) {
        return Err(io::Error::other("the probe asks over HTTPS only"));
    }
    let host = url.host_str().ok_or_else(|| io::Error::other("no host"))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| io::Error::other("no port"))?;
    let mut times = Times::default();
    let from = Instant::now();
    let address = (host, port)
        .to_socket_addrs()
        .map_err(step("dns"))?
        .next()
        .ok_or_else(|| io::Error::other("dns: no address"))?;
    times.dns = ms(from);
    let from = Instant::now();
    let mut tcp = TcpStream::connect_timeout(&address, CONNECT).map_err(step("tcp"))?;
    times.tcp = ms(from);
    tcp.set_nodelay(true)?;
    tcp.set_read_timeout(Some(STALL))?;
    tcp.set_write_timeout(Some(STALL))?;
    let path = match url.query() {
        Some(query) => format!("{}?{query}", url.path()),
        None => url.path().to_owned(),
    };
    let named = match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_owned(),
    };
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {named}\r\nAuthorization: {auth}\r\n\
         Content-Type: application/json\r\nAccept: text/event-stream\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let asked = |stream: &mut dyn ReadWrite, times: &mut Times| -> io::Result<()> {
        let from = Instant::now();
        stream.write_all(request.as_bytes()).map_err(step("send"))?;
        stream.write_all(body).map_err(step("send"))?;
        stream.flush().map_err(step("send"))?;
        times.send = ms(from);
        times.answer = read_answer(stream, Instant::now()).map_err(step("answer"))?;
        Ok(())
    };
    match tls.filter(|_| secure) {
        Some(config) => {
            let name = rustls::pki_types::ServerName::try_from(host.to_owned())
                .map_err(|_| io::Error::other("tls: name"))?;
            let mut connection = rustls::ClientConnection::new(config, name)
                .map_err(|_| io::Error::other("tls: setup"))?;
            let from = Instant::now();
            while connection.is_handshaking() {
                connection.complete_io(&mut tcp).map_err(step("tls"))?;
            }
            times.tls = ms(from);
            let mut stream = rustls::StreamOwned::new(connection, tcp);
            asked(&mut stream, &mut times)?;
        }
        None if secure => return Err(io::Error::other("tls: no configuration")),
        None => asked(&mut tcp, &mut times)?,
    }
    Ok(times)
}

/// Read and write, for one closure over a TLS stream or a plain one.
trait ReadWrite: Read + Write {}
impl<T: Read + Write> ReadWrite for T {}

/// One target's and size's times over the runs, for the summary.
#[derive(Default)]
pub struct Kept {
    pub bytes: usize,
    pub tcp: Vec<u64>,
    pub tls: Vec<u64>,
    pub send: Vec<u64>,
    pub first_byte: Vec<u64>,
    pub gap: Vec<u64>,
    pub words: Vec<u64>,
    pub done: Vec<u64>,
    pub failed: u32,
}

impl Kept {
    pub fn keep(&mut self, times: &Times) {
        self.tcp.push(times.tcp);
        self.tls.push(times.tls);
        self.send.push(times.send);
        self.first_byte.extend(times.answer.first_byte);
        self.gap.extend(times.answer.gap());
        self.words.extend(times.answer.words);
        self.done.push(times.answer.done);
    }

    fn medians(&self) -> [Option<u64>; 6] {
        [
            &self.tcp,
            &self.tls,
            &self.send,
            &self.first_byte,
            &self.gap,
            &self.words,
        ]
        .map(|values| percentile(values, 50))
    }

    pub fn summary(&self, target: &str, size: Size) -> String {
        let show = |values: &[u64], p| shown(percentile(values, p));
        format!(
            "wire target={target} size={} runs={} failed={} bytes={} tcp_p50_ms={} tls_p50_ms={} \
             send_p50_ms={} first_byte_p50_ms={} first_byte_p90_ms={} gap_p50_ms={} \
             words_p50_ms={} words_p90_ms={} done_p50_ms={}",
            size.name(),
            self.done.len() as u32 + self.failed,
            self.failed,
            self.bytes,
            show(&self.tcp, 50),
            show(&self.tls, 50),
            show(&self.send, 50),
            show(&self.first_byte, 50),
            show(&self.first_byte, 90),
            show(&self.gap, 50),
            show(&self.words, 50),
            show(&self.words, 90),
            show(&self.done, 50),
        )
    }
}

/// The relay's medians less DeepSeek's own, step by step, for one size.
pub fn difference(relay: &Kept, direct: &Kept, size: Size) -> String {
    let names = ["tcp", "tls", "send", "first_byte", "gap", "words"];
    let steps = relay
        .medians()
        .into_iter()
        .zip(direct.medians())
        .zip(names)
        .map(|((relay, direct), name)| match (relay, direct) {
            (Some(relay), Some(direct)) => {
                format!(" {name}_ms={:+}", i128::from(relay) - i128::from(direct))
            }
            _ => format!(" {name}_ms=none"),
        })
        .collect::<String>();
    format!("wire relay-less-direct size={}{steps}", size.name())
}

/// The probe, as `lamp-voice probe-cascade … --wire` runs it.
pub fn run(provider_config: &Path, second_config: &Path, runs: u32) -> io::Result<()> {
    let provider = crate::config::ProviderConfig::load(provider_config)?;
    let character = provider.instruction(false)?;
    let config = crate::second_config::load_from_path(second_config)?
        .with_environment(|name| std::env::var_os(name))?;
    let credentials = std::env::var_os(CREDENTIALS_ENV);
    let actions = std::env::var_os(crate::tools::ACTIONS_ENVIRONMENT)
        .filter(|dir| !dir.is_empty())
        .map_or_else(|| "/run/lampos-actions".into(), std::path::PathBuf::from);
    let declared = super::tools::functions(Some(actions));
    let count = declared.len();
    let all: Declare = Arc::new(move || declared.clone());
    let none: Declare = Arc::new(Vec::new);
    let model = &config.hosted.deepseek_model;
    let chats = |base: &str, key: &str| -> io::Result<[Chat; 2]> {
        Ok([
            Chat::deepseek_at(base, model, key, all.clone(), false).map_err(io::Error::other)?,
            Chat::deepseek_at(base, model, key, none.clone(), false).map_err(io::Error::other)?,
        ])
    };
    let mut targets: Vec<(&str, [Chat; 2])> = Vec::new();
    match &config.hosted.deepseek_base_url {
        Some(base) => targets.push(("relay", chats(base, config.llm_key.expose())?)),
        None => println!("wire target=relay: left out: no deepseek_base_url"),
    }
    match unit_credential(credentials.as_deref(), DEEPSEEK_CREDENTIAL) {
        Ok(key) => targets.push((
            "direct",
            chats(crate::second_config::DEEPSEEK_BASE_URL, key.expose())?,
        )),
        Err(error) => println!("wire target=direct: left out: {error}"),
    }
    let tls = Arc::new(net::tls().map_err(io::Error::other)?);
    println!(
        "wire: DeepSeek's {model}, thinking off, {count} functions on the lamp's request, {runs} runs, \
         each request on a connection of its own"
    );
    let mut kept: Vec<[Kept; 2]> = targets.iter().map(|_| Default::default()).collect();
    for run in 1..=runs {
        for (index, size) in Size::ALL.into_iter().enumerate() {
            for ((name, chats), kept) in targets.iter().zip(&mut kept) {
                let (url, auth, body) = chats[index].wire(&size.messages(&character, run));
                let body = serde_json::to_vec(&body)?;
                let url = reqwest::Url::parse(url).map_err(|_| io::Error::other("url"))?;
                let kept = &mut kept[index];
                kept.bytes = body.len();
                match ask(&url, auth, &body, Some(tls.clone())) {
                    Ok(times) => {
                        println!(
                            "wire target={name} size={} run={run} {}",
                            size.name(),
                            times.shown(body.len())
                        );
                        kept.keep(&times);
                    }
                    Err(error) => {
                        println!(
                            "wire target={name} size={} run={run} failed: {error}",
                            size.name()
                        );
                        kept.failed += 1;
                    }
                }
            }
        }
    }
    for ((name, _), kept) in targets.iter().zip(&kept) {
        for (index, size) in Size::ALL.into_iter().enumerate() {
            println!("{}", kept[index].summary(name, size));
        }
    }
    if targets.len() == 2 {
        for (index, size) in Size::ALL.into_iter().enumerate() {
            println!("{}", difference(&kept[0][index], &kept[1][index], size));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// A reader that gives its parts one read each, `pause` apart.
    struct Parts {
        parts: VecDeque<Vec<u8>>,
        pause: Duration,
    }

    impl Read for Parts {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let Some(part) = self.parts.pop_front() else {
                return Ok(0);
            };
            std::thread::sleep(self.pause);
            buffer[..part.len()].copy_from_slice(&part);
            Ok(part.len())
        }
    }

    const ROLE: &str = r#"data: {"choices":[{"delta":{"role":"assistant","content":""}}]}"#;
    const WORDS: &str = r#"data: {"choices":[{"delta":{"content":"Read a book."}}]}"#;

    /// `events` as one chunked body, each event its own chunk.
    fn chunked(events: &[&str]) -> Vec<Vec<u8>> {
        let mut parts = vec![
            b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n"
                .to_vec(),
        ];
        for event in events {
            let data = format!("{event}\n\n");
            parts.push(format!("{:x}\r\n{data}\r\n", data.len()).into_bytes());
        }
        parts.push(b"0\r\n\r\n".to_vec());
        parts
    }

    #[test]
    fn an_answers_events_are_timed_through_its_chunks_and_split_reads() {
        // The head with the first event, then a chunk cut mid-size-line and
        // mid-data, as reads can cut it.
        let mut parts = chunked(&[ROLE, WORDS, WORDS, "data: [DONE]"]);
        let first = parts.remove(1);
        parts[0].extend(first);
        let whole = parts.remove(1);
        let (a, b) = whole.split_at(1);
        let (b, c) = b.split_at(20);
        for (at, part) in [a, b, c].into_iter().enumerate() {
            parts.insert(1 + at, part.to_vec());
        }
        let mut reader = Parts {
            parts: parts.into(),
            pause: Duration::from_millis(15),
        };
        let answer = read_answer(&mut reader, Instant::now()).unwrap();
        assert_eq!(answer.status, 200);
        assert_eq!(answer.events, 3, "{answer:?}");
        let (first, second) = (answer.first_event.unwrap(), answer.second_event.unwrap());
        assert_eq!(answer.first_byte, Some(first), "the head came with it");
        // The second event came whole only with the third read after it.
        assert!(second >= first + 45, "{answer:?}");
        assert_eq!(answer.words, Some(second));
        assert_eq!(answer.gap(), Some(second - first));
        assert!(reader.parts.len() <= 1, "read to [DONE]");
    }

    #[test]
    fn a_refused_request_says_its_status_and_its_body_is_not_read() {
        let mut reader = Parts {
            parts: [
                b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 30\r\n\r\n".to_vec(),
                br#"{"error":"bad key, not read"}"#.to_vec(),
            ]
            .into(),
            pause: Duration::ZERO,
        };
        let answer = read_answer(&mut reader, Instant::now()).unwrap();
        assert_eq!((answer.status, answer.events), (401, 0));
        assert_eq!(reader.parts.len(), 1);
        let mut garbled = Parts {
            parts: [b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n".to_vec()].into(),
            pause: Duration::ZERO,
        };
        assert!(read_answer(&mut garbled, Instant::now()).is_err());
    }

    /// The request goes as the route makes it, to a local server that
    /// answers its first event at once and the second 100 ms later.
    #[test]
    fn a_request_is_made_by_hand_and_timed_at_each_step() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut asked = Vec::new();
            let mut buffer = [0u8; 4096];
            let (head, length) = loop {
                let n = socket.read(&mut buffer).unwrap();
                asked.extend_from_slice(&buffer[..n]);
                if let Some(end) = asked.windows(4).position(|four| four == b"\r\n\r\n") {
                    let head = String::from_utf8(asked[..end].to_vec()).unwrap();
                    let length: usize = head
                        .lines()
                        .find_map(|line| line.strip_prefix("Content-Length: "))
                        .unwrap()
                        .parse()
                        .unwrap();
                    asked.drain(..end + 4);
                    break (head, length);
                }
            };
            while asked.len() < length {
                let n = socket.read(&mut buffer).unwrap();
                asked.extend_from_slice(&buffer[..n]);
            }
            socket.set_nodelay(true).unwrap();
            let mut parts = chunked(&[ROLE, WORDS, "data: [DONE]"]).into_iter();
            let first: Vec<u8> = parts.by_ref().take(2).flatten().collect();
            socket.write_all(&first).unwrap();
            std::thread::sleep(Duration::from_millis(100));
            for part in parts {
                socket.write_all(&part).unwrap();
            }
            (head, asked)
        });
        let url =
            reqwest::Url::parse(&format!("http://127.0.0.1:{port}/v1/chat/completions")).unwrap();
        let body = br#"{"model":"m","stream":true}"#;
        let times = ask(&url, "Bearer k", body, None).unwrap();
        let (head, asked) = server.join().unwrap();
        assert!(
            head.starts_with("POST /v1/chat/completions HTTP/1.1\r\n"),
            "{head}"
        );
        for line in [
            "Host: 127.0.0.1:",
            "Authorization: Bearer k",
            "Accept: text/event-stream",
            "Connection: close",
        ] {
            assert!(head.contains(line), "{line} in {head}");
        }
        assert_eq!(asked, body);
        let answer = times.answer;
        assert_eq!((answer.status, answer.events), (200, 2));
        assert!(answer.gap().unwrap() >= 50, "{answer:?}");
        assert_eq!(answer.words, answer.second_event);
        assert!(
            times
                .shown(body.len())
                .starts_with("bytes=27 status=200 dns_ms=")
        );
    }

    #[test]
    fn a_failed_step_is_told_by_its_kind_without_the_host() {
        let url = reqwest::Url::parse("https://127.0.0.1:9/v1/chat/completions").unwrap();
        let error = ask(&url, "Bearer k", b"{}", None).unwrap_err();
        // Port 9 refuses, or there is no TLS configuration: either way no
        // address is in the words.
        assert!(!error.to_string().contains("127.0.0.1"), "{error}");
    }

    #[test]
    fn the_lamps_request_declares_every_function_and_the_small_one_none() {
        let declared = cache::functions();
        let count = declared.len();
        let all: Declare = Arc::new(move || declared.clone());
        let none: Declare = Arc::new(Vec::new);
        let base = "https://relay.example.invalid/api/v1/ai/v1/deepseek";
        let lamp = Chat::deepseek_at(base, "deepseek-flash", "k", all, false).unwrap();
        let small = Chat::deepseek_at(base, "deepseek-flash", "k", none, false).unwrap();
        let (url, auth, body) = lamp.wire(&Size::Lamp.messages("CHARACTER", 3));
        assert_eq!(url, format!("{base}/chat/completions"));
        assert_eq!(auth, "Bearer k");
        assert_eq!(body["tools"].as_array().unwrap().len(), count);
        assert_eq!(body["thinking"]["type"], "disabled");
        assert_eq!(body["stream"], true);
        assert!(
            body["messages"][0]["content"]
                .as_str()
                .unwrap()
                .starts_with("CHARACTER\n\nRequest 3 ")
        );
        let (_, _, body) = small.wire(&Size::Small.messages("CHARACTER", 3));
        assert!(body.get("tools").is_none());
        assert_eq!(body["messages"][0]["content"], SMALL);
    }

    #[test]
    fn the_summary_gives_each_steps_median_and_the_relays_extra() {
        let times = |tcp, first_byte| Times {
            dns: 1,
            tcp,
            tls: 2 * tcp,
            send: 1,
            answer: Answer {
                status: 200,
                first_byte: Some(first_byte),
                first_event: Some(first_byte),
                second_event: Some(first_byte + 5),
                words: Some(first_byte + 5),
                done: first_byte + 100,
                events: 9,
            },
        };
        let mut relay = Kept::default();
        let mut direct = Kept::default();
        for (tcp, first_byte) in [(30, 900), (40, 950), (35, 1_000)] {
            relay.keep(&times(tcp, first_byte));
            direct.keep(&times(tcp - 20, first_byte - 600));
        }
        relay.failed = 1;
        assert!(relay.summary("relay", Size::Lamp).starts_with(
            "wire target=relay size=lamp runs=4 failed=1 bytes=0 tcp_p50_ms=35 tls_p50_ms=70 "
        ));
        assert_eq!(
            difference(&relay, &direct, Size::Lamp),
            "wire relay-less-direct size=lamp tcp_ms=+20 tls_ms=+40 send_ms=+0 first_byte_ms=+600 gap_ms=+0 words_ms=+600"
        );
        assert!(difference(&relay, &Kept::default(), Size::Small).ends_with("words_ms=none"));
    }
}
