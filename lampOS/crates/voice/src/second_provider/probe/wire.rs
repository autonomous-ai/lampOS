//! `lamp-voice probe-cascade PROVIDER_CONFIG SECOND_CONFIG [RUNS] --wire`:
//! where a chat request's time goes between the lamp and the service, for
//! DeepSeek through the relay and on its own API (#1934; the first builder
//! on #1722, 00:56). On lamp-4ace the relay's answers began about 0.6 s
//! later than DeepSeek's own. Each request is made by hand on a connection
//! of its own, and timed at each step:
//!
//! - **dns**, **tcp**, **tls**: the name looked up, the TCP connect, the
//!   TLS handshake;
//! - **send**: the request handed to the kernel, its body whole (on a
//!   large request part of the upload may still be going, and is counted
//!   in the first byte);
//! - **first byte**: from then to the response's first;
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
//! a late first byte, or a first event late after a first byte on time,
//! the relay holding the stream back.
//!
//! Two things it is not. The lamp's request has no memory note, facts,
//! time line or the route's notes, so its `bytes=` is under a real turn's:
//! read it beside the route's own `chat request` journal line. And every
//! request is on a new connection, paying `tcp_ms` and `tls_ms` each time,
//! where the route reuses a kept one: `done_ms` is the request's, not a
//! turn's (the first builder's agent on #2024).
//!
//! With `--kept` (the first builder on #1934, 04:16), each target and size
//! is asked three requests on one kept connection ([`Arm`]): the first,
//! which makes it; one straight after; and one after [`IDLE`] with nothing
//! sent. The straight one pays no connect, TLS or slow start. The idle one
//! is what a real turn pays on the route's kept connection, since the
//! kernel restarts slow start after idle unless
//! `net.ipv4.tcp_slow_start_after_idle` is 0; their difference is what that
//! setting would buy. Each request has its own changing part, as turns do.
//!
//! With `--speech` (#1934, 08:36), the route's speech instead of its chat:
//! a piece an answer's first sentence long ([`PIECES`]), asked of
//! ElevenLabs as `Tts` asks it, through the relay (`tts_base_url`, which is
//! `llm_base_url` unless set, with the route's speech credential) and, with
//! an `elevenlabs.key` unit credential, on ElevenLabs' own API. Each is
//! timed as a chat is, to the answer's first byte, its **first audio** (the
//! body's first byte) and **done** (the whole piece downloaded); with
//! `--kept`, on one kept connection as well. An answer's first sound waits
//! for its first piece's first audio, so the relay's share of that is what
//! each turn pays before the lamp speaks.
//!
//! The relay is `deepseek_base_url` (or `LAMP_SECOND_DEEPSEEK_BASE_URL`),
//! with the route's credential; DeepSeek's own API takes the unit's
//! `deepseek.key`. Each request is the route's own as `Chat` builds it,
//! `hosted.deepseek_model` with thinking off. It prints counts and
//! milliseconds only: never the words asked or answered, a key, a host or a
//! URL.
use super::super::net::{self, Chat, Declare, Message, SpeechRequest, speech_request};
use super::{cache, percentile};
use crate::second_config::{
    CREDENTIALS_ENV, DEEPSEEK_CREDENTIAL, SecondConfig, TtsProvider, unit_credential,
};
use serde_json::Value;
use std::{
    ffi::OsStr,
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

/// Each request on one kept connection (`--kept`): the first, which makes
/// it, one straight after, and one after [`IDLE`] with nothing sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arm {
    First,
    Straight,
    Idle,
}

impl Arm {
    pub const ALL: [Self; 3] = [Self::First, Self::Straight, Self::Idle];

    pub fn name(self) -> &'static str {
        match self {
            Self::First => "first",
            Self::Straight => "straight",
            Self::Idle => "idle",
        }
    }
}

/// The most runs `--kept` takes: each makes 12 paid requests and waits 5 s
/// four times (the reviewer on #2044), or with `--speech` 6 and twice.
pub const MOST_KEPT_RUNS: u32 = 20;

/// How long the kept connection sits idle before [`Arm::Idle`]: past any
/// retransmission timeout, so the kernel restarts slow start unless
/// `net.ipv4.tcp_slow_start_after_idle` is 0 (the first builder on #1934).
pub const IDLE: Duration = Duration::from_secs(5);

/// How an answer's body is read: a chat's event stream, or a piece of
/// speech's audio.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reading {
    Events,
    Audio,
}

/// The pieces `--speech` asks for, one a run in turn: each an answer's
/// first sentence long, as the route's first piece of an answer is.
pub const PIECES: [&str; 4] = [
    "Sure, I can look into that for you.",
    "Good morning! The coffee smells great.",
    "Okay, give me just a second to think.",
    "That sounds like a lovely plan to me.",
];

/// ElevenLabs' own API, for `--speech`'s direct target.
pub const ELEVENLABS_BASE_URL: &str = "https://api.elevenlabs.io/v1";
/// The unit credential with an ElevenLabs key of its own, for the direct
/// target; the route itself has none (its speech goes through the relay).
pub const ELEVENLABS_CREDENTIAL: &str = "elevenlabs.key";

/// One request as the probe sends it: where, its key header and value, its
/// JSON body, and how its answer is read.
pub struct Wired {
    pub url: reqwest::Url,
    pub header: &'static str,
    pub value: String,
    pub body: Vec<u8>,
    pub reading: Reading,
}

impl Wired {
    /// A chat request, as the route's `Chat` builds it.
    pub fn chat(chat: &Chat, messages: &[Message]) -> io::Result<Self> {
        let (url, auth, body) = chat.wire(messages);
        Ok(Self {
            url: reqwest::Url::parse(url).map_err(|_| io::Error::other("url"))?,
            header: "Authorization",
            value: auth.to_owned(),
            body: serde_json::to_vec(&body)?,
            reading: Reading::Events,
        })
    }

    /// A piece of speech, as the route's `Tts` asks for it.
    pub(crate) fn speech(request: SpeechRequest) -> io::Result<Self> {
        Ok(Self {
            url: reqwest::Url::parse(&request.url).map_err(|_| io::Error::other("url"))?,
            header: request.header,
            value: request.value,
            body: serde_json::to_vec(&request.body)?,
            reading: Reading::Audio,
        })
    }
}

/// `run`'s piece of [`PIECES`], as the route asks ElevenLabs for it at
/// `base_url` with `key`.
fn piece(
    base_url: &str,
    key: &str,
    voice: Option<&str>,
    model: &str,
    run: u32,
) -> io::Result<SpeechRequest> {
    let text = PIECES[run as usize % PIECES.len()];
    speech_request(
        TtsProvider::ElevenLabs,
        base_url,
        key,
        voice,
        Some(model),
        text,
    )
    .map_err(io::Error::other)?
    .ok_or_else(|| io::Error::other("nothing to speak"))
}

/// Whether `url` is on ElevenLabs' own API, not the relay.
fn on_elevenlabs(url: &reqwest::Url) -> bool {
    url.host_str().is_some_and(|host| {
        let host = host.to_ascii_lowercase();
        host == "elevenlabs.io" || host.ends_with(".elevenlabs.io")
    })
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
    /// The connection can carry another request: answered 200, its body
    /// read to its end, and not told to close.
    pub kept: bool,
    /// Speech: the body's first byte, and how many bytes it had.
    pub audio: Option<u64>,
    pub audio_bytes: usize,
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
    /// The run's line, for a request of `bytes` read as `reading`.
    pub fn shown(&self, bytes: usize, reading: Reading) -> String {
        let a = &self.answer;
        let steps = format!(
            "bytes={bytes} status={} dns_ms={} tcp_ms={} tls_ms={} send_ms={} first_byte_ms={}",
            a.status,
            self.dns,
            self.tcp,
            self.tls,
            self.send,
            shown(a.first_byte),
        );
        match reading {
            Reading::Events => format!(
                "{steps} first_event_ms={} second_event_ms={} gap_ms={} words_ms={} done_ms={} \
                 events={}",
                shown(a.first_event),
                shown(a.second_event),
                shown(a.gap()),
                shown(a.words),
                a.done,
                a.events
            ),
            Reading::Audio => format!(
                "{steps} first_audio_ms={} done_ms={} audio_bytes={}",
                shown(a.audio),
                a.done,
                a.audio_bytes
            ),
        }
    }
}

/// A chunked body's bytes, from its chunks as they come.
#[derive(Default)]
struct Chunked {
    line: Vec<u8>,
    left: usize,
    /// The line end after a chunk's data, still to skip.
    after: usize,
    /// The last chunk came: what follows is trailers, up to an empty line.
    last: bool,
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
                self.line.clear();
                if self.last {
                    // A trailer, or the empty line that ends the body: read
                    // to it, the connection can carry the next answer.
                    self.ended = line.trim().is_empty();
                    continue;
                }
                let size = line.split(';').next().unwrap_or_default().trim();
                let size =
                    usize::from_str_radix(size, 16).map_err(|_| io::Error::other("chunk size"))?;
                if size == 0 {
                    self.last = true;
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

/// Reads an HTTP/1.1 response's head and its body from `reader`, timing
/// each from `written`: a chat's event stream ([`Reading::Events`]), `done`
/// at `[DONE]` or the stream's end, or a piece of speech's audio
/// ([`Reading::Audio`]), its first byte and `done` at the body's end. The
/// body is read on to its end (its last chunk, or its `Content-Length`), so
/// the connection can carry another request ([`Answer::kept`]). An answer
/// other than 200 is told by its status, and its body is not read.
pub fn read_answer<R: Read + ?Sized>(
    reader: &mut R,
    written: Instant,
    reading: Reading,
) -> io::Result<Answer> {
    let mut answer = Answer::default();
    let mut head = Vec::new();
    let mut chunked: Option<Chunked> = None;
    // The body's length, when the head gives it, and how much of it came.
    let mut length: Option<usize> = None;
    let mut got = 0usize;
    let mut closing = false;
    let mut finished: Option<u64> = None;
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
            length = text
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .and_then(|value| value.trim().parse().ok());
            closing = text
                .lines()
                .any(|line| line.starts_with("connection:") && line.contains("close"));
            bytes = &rest;
        }
        got += bytes.len();
        body.clear();
        match chunked.as_mut() {
            Some(chunks) => chunks.feed(bytes, &mut body)?,
            None => body.extend_from_slice(bytes),
        }
        if reading == Reading::Audio {
            if !body.is_empty() {
                answer.audio.get_or_insert(at);
                answer.audio_bytes += body.len();
            }
        } else {
            for event in events.feed(&body) {
                if finished.is_some() {
                    break;
                }
                match event {
                    Event::Done => finished = Some(at),
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
        }
        let whole = match (&chunked, length) {
            (Some(chunks), _) => chunks.ended,
            (None, Some(length)) => got >= length,
            // Ended only by the connection's close: nothing to read on to.
            (None, None) => finished.is_some(),
        };
        if whole {
            answer.kept = !closing && (chunked.is_some() || length.is_some());
            break 'reading;
        }
    }
    answer.done = finished.unwrap_or_else(|| ms(written));
    Ok(answer)
}

/// A step's failure, by its kind only: an error's own words may name the
/// host.
fn step(name: &'static str) -> impl Fn(io::Error) -> io::Error {
    move |error| io::Error::new(error.kind(), format!("{name}: {:?}", error.kind()))
}

/// `wired` posted on a connection of its own, timed at each step. HTTPS
/// with `tls`; a plain `http` URL only in tests.
pub fn ask(wired: &Wired, tls: Option<Arc<rustls::ClientConfig>>) -> io::Result<Times> {
    let mut times = Times::default();
    let mut connection = Connection::open(&wired.url, tls, &mut times)?;
    (times.send, times.answer) = connection.post(wired, false)?;
    Ok(times)
}

/// A connection made by hand to one service, for one request or several.
pub struct Connection {
    stream: Box<dyn ReadWrite>,
    named: String,
}

impl Connection {
    /// `url`'s service, connected: the name's lookup, the TCP connect and
    /// the TLS handshake timed into `times`. HTTPS with `tls`; a plain
    /// `http` URL only in tests.
    pub fn open(
        url: &reqwest::Url,
        tls: Option<Arc<rustls::ClientConfig>>,
        times: &mut Times,
    ) -> io::Result<Self> {
        let secure = url.scheme() == "https";
        if !secure && !cfg!(test) {
            return Err(io::Error::other("the probe asks over HTTPS only"));
        }
        let host = url.host_str().ok_or_else(|| io::Error::other("no host"))?;
        let port = url
            .port_or_known_default()
            .ok_or_else(|| io::Error::other("no port"))?;
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
        let named = match url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_owned(),
        };
        let stream: Box<dyn ReadWrite> = match tls.filter(|_| secure) {
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
                Box::new(rustls::StreamOwned::new(connection, tcp))
            }
            None if secure => return Err(io::Error::other("tls: no configuration")),
            None => Box::new(tcp),
        };
        Ok(Self { stream, named })
    }

    /// `wired` posted, asking the service to keep the connection for
    /// another when `keep`: how long it took to hand over, and its answer.
    pub fn post(&mut self, wired: &Wired, keep: bool) -> io::Result<(u64, Answer)> {
        let url = &wired.url;
        let path = match url.query() {
            Some(query) => format!("{}?{query}", url.path()),
            None => url.path().to_owned(),
        };
        // A chat asks for its event stream; speech takes any, as `Tts`.
        let accept = match wired.reading {
            Reading::Events => "text/event-stream",
            Reading::Audio => "*/*",
        };
        let request = format!(
            "POST {path} HTTP/1.1\r\nHost: {}\r\n{}: {}\r\n\
             Content-Type: application/json\r\nAccept: {accept}\r\n\
             Content-Length: {}\r\nConnection: {}\r\n\r\n",
            self.named,
            wired.header,
            wired.value,
            wired.body.len(),
            if keep { "keep-alive" } else { "close" }
        );
        let from = Instant::now();
        self.stream
            .write_all(request.as_bytes())
            .map_err(step("send"))?;
        self.stream.write_all(&wired.body).map_err(step("send"))?;
        self.stream.flush().map_err(step("send"))?;
        let send = ms(from);
        let answer = read_answer(&mut *self.stream, Instant::now(), wired.reading)
            .map_err(step("answer"))?;
        Ok((send, answer))
    }
}

/// Read and write, for a TLS stream or a plain one.
trait ReadWrite: Read + Write {}
impl<T: Read + Write> ReadWrite for T {}

/// One target's and kind's times over the runs, for the summary.
#[derive(Default)]
pub struct Kept {
    pub bytes: usize,
    pub tcp: Vec<u64>,
    pub tls: Vec<u64>,
    pub send: Vec<u64>,
    pub first_byte: Vec<u64>,
    pub first_event: Vec<u64>,
    pub gap: Vec<u64>,
    pub words: Vec<u64>,
    pub done: Vec<u64>,
    /// Speech: the first audio byte, and the audio's bytes.
    pub audio: Vec<u64>,
    pub audio_bytes: Vec<u64>,
    pub failed: u32,
    /// Requests not timed: refused, or not made because the service did
    /// not keep the connection.
    pub closed: u32,
}

impl Kept {
    pub fn keep(&mut self, times: &Times) {
        self.tcp.push(times.tcp);
        self.tls.push(times.tls);
        self.send.push(times.send);
        self.first_byte.extend(times.answer.first_byte);
        self.first_event.extend(times.answer.first_event);
        self.gap.extend(times.answer.gap());
        self.words.extend(times.answer.words);
        self.done.push(times.answer.done);
        if let Some(audio) = times.answer.audio {
            self.audio.push(audio);
            self.audio_bytes.push(times.answer.audio_bytes as u64);
        }
    }

    /// A request on a connection of its own: timed when answered 200. A
    /// refused one is counted `closed`, as on a kept connection, so its
    /// quick answer never lowers the medians.
    pub fn answered(&mut self, times: &Times) {
        if times.answer.status == 200 {
            self.keep(times);
        } else {
            self.closed += 1;
        }
    }

    /// The steps compared between two targets, by name: a chat's to its
    /// words, or speech's to its first audio and its end.
    fn steps(&self, reading: Reading) -> Vec<(&'static str, &[u64])> {
        let mut steps = vec![
            ("tcp", &self.tcp[..]),
            ("tls", &self.tls[..]),
            ("send", &self.send[..]),
            ("first_byte", &self.first_byte[..]),
        ];
        match reading {
            Reading::Events => steps.extend([
                ("first_event", &self.first_event[..]),
                ("gap", &self.gap[..]),
                ("words", &self.words[..]),
            ]),
            Reading::Audio => {
                steps.extend([("first_audio", &self.audio[..]), ("done", &self.done[..])]);
            }
        }
        steps
    }

    /// The summary's line, after `label`, for requests read as `reading`.
    pub fn summary(&self, label: &str, reading: Reading) -> String {
        let show = |values: &[u64], p| shown(percentile(values, p));
        let closed = if self.closed > 0 {
            format!(" closed={}", self.closed)
        } else {
            String::new()
        };
        let head = format!(
            "wire {label} runs={} failed={} bytes={} tcp_p50_ms={} tls_p50_ms={} \
             send_p50_ms={} first_byte_p50_ms={} first_byte_p90_ms={}",
            self.done.len() as u32 + self.failed + self.closed,
            self.failed,
            self.bytes,
            show(&self.tcp, 50),
            show(&self.tls, 50),
            show(&self.send, 50),
            show(&self.first_byte, 50),
            show(&self.first_byte, 90),
        );
        let rest = match reading {
            Reading::Events => format!(
                "first_event_p50_ms={} first_event_p90_ms={} gap_p50_ms={} words_p50_ms={} \
                 words_p90_ms={} done_p50_ms={}",
                show(&self.first_event, 50),
                show(&self.first_event, 90),
                show(&self.gap, 50),
                show(&self.words, 50),
                show(&self.words, 90),
                show(&self.done, 50),
            ),
            Reading::Audio => format!(
                "first_audio_p50_ms={} first_audio_p90_ms={} done_p50_ms={} done_p90_ms={} \
                 audio_bytes_p50={}",
                show(&self.audio, 50),
                show(&self.audio, 90),
                show(&self.done, 50),
                show(&self.done, 90),
                show(&self.audio_bytes, 50),
            ),
        };
        format!("{head} {rest}{closed}")
    }
}

/// `a`'s medians less `b`'s, step by step, after `label`.
pub fn less(label: &str, a: &Kept, b: &Kept, reading: Reading) -> String {
    let steps = a
        .steps(reading)
        .into_iter()
        .zip(b.steps(reading))
        .map(
            |((name, a), (_, b))| match (percentile(a, 50), percentile(b, 50)) {
                (Some(a), Some(b)) => format!(" {name}_ms={:+}", i128::from(a) - i128::from(b)),
                _ => format!(" {name}_ms=none"),
            },
        )
        .collect::<String>();
    format!("wire {label}{steps}")
}

/// One kind of request each target is asked: a chat's two sizes
/// ([`Size`]), or a piece of speech.
struct Kind {
    /// Its words in each line, before `target=` and after it.
    before: &'static str,
    after: String,
    reading: Reading,
    /// Its request in a run, each with its own changing part or piece.
    request: Box<dyn Fn(u32) -> io::Result<Wired>>,
}

impl Kind {
    /// `what` with the kind's words around it, after `mode`.
    fn named(&self, mode: &str, what: &str) -> String {
        format!("{mode}{}{what}{}", self.before, self.after)
    }
}

/// What both modes ask with: each target's kinds, the same kinds on each,
/// and the TLS setup.
struct Prepared {
    targets: Vec<(&'static str, Vec<Kind>)>,
    tls: Arc<rustls::ClientConfig>,
}

/// How `--wire` asks: on one kept connection (`--kept`), and the route's
/// speech instead of its chat (`--speech`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mode {
    pub kept: bool,
    pub speech: bool,
}

/// The probe, as `lamp-voice probe-cascade … --wire [--kept] [--speech]`
/// runs it.
pub fn run(provider_config: &Path, second_config: &Path, runs: u32, mode: Mode) -> io::Result<()> {
    let prepared = prepare(provider_config, second_config, runs, mode)?;
    if mode.kept {
        run_kept(&prepared, runs)
    } else {
        run_cold(&prepared, runs)
    }
}

fn prepare(
    provider_config: &Path,
    second_config: &Path,
    runs: u32,
    mode: Mode,
) -> io::Result<Prepared> {
    let config = crate::second_config::load_from_path(second_config)?
        .with_environment(|name| std::env::var_os(name))?;
    let credentials = std::env::var_os(CREDENTIALS_ENV);
    let tls = Arc::new(net::tls().map_err(io::Error::other)?);
    let how = |each: &str| {
        if mode.kept {
            format!(
                "{each} on one kept connection: a first request, one straight after, one after {} s idle",
                IDLE.as_secs()
            )
        } else {
            "each request on a connection of its own".to_owned()
        }
    };
    let targets = if mode.speech {
        speech_targets(&config, credentials.as_deref(), runs, &how("each target"))?
    } else {
        chat_targets(
            provider_config,
            &config,
            credentials.as_deref(),
            runs,
            &how("each target and size"),
        )?
    };
    Ok(Prepared { targets, tls })
}

/// The chat's targets, the relay and DeepSeek's own API, each asked the
/// lamp's request and the small one.
fn chat_targets(
    provider_config: &Path,
    config: &SecondConfig,
    credentials: Option<&OsStr>,
    runs: u32,
    how: &str,
) -> io::Result<Vec<(&'static str, Vec<Kind>)>> {
    let provider = crate::config::ProviderConfig::load(provider_config)?;
    let character: Arc<str> = provider.instruction(false)?.into();
    let actions = std::env::var_os(crate::tools::ACTIONS_ENVIRONMENT)
        .filter(|dir| !dir.is_empty())
        .map_or_else(|| "/run/lampos-actions".into(), std::path::PathBuf::from);
    let declared = super::tools::functions(Some(actions));
    let count = declared.len();
    let all: Declare = Arc::new(move || declared.clone());
    let none: Declare = Arc::new(Vec::new);
    let model = &config.hosted.deepseek_model;
    let kinds = |base: &str, key: &str| -> io::Result<Vec<Kind>> {
        Size::ALL
            .into_iter()
            .map(|size| {
                let declare = match size {
                    Size::Lamp => all.clone(),
                    Size::Small => none.clone(),
                };
                let chat = Chat::deepseek_at(base, model, key, declare, false)
                    .map_err(io::Error::other)?;
                let character = character.clone();
                Ok(Kind {
                    before: "",
                    after: format!(" size={}", size.name()),
                    reading: Reading::Events,
                    request: Box::new(move |run| {
                        Wired::chat(&chat, &size.messages(&character, run))
                    }),
                })
            })
            .collect()
    };
    let mut targets = Vec::new();
    match &config.hosted.deepseek_base_url {
        Some(base) => targets.push(("relay", kinds(base, config.llm_key.expose())?)),
        None => println!(
            "wire target=relay: left out: no deepseek_base_url (systemd-run does not carry the voice unit's LAMP_SECOND_DEEPSEEK_BASE_URL: give it with -E)"
        ),
    }
    match unit_credential(credentials, DEEPSEEK_CREDENTIAL) {
        Ok(key) => targets.push((
            "direct",
            kinds(crate::second_config::DEEPSEEK_BASE_URL, key.expose())?,
        )),
        Err(error) => println!("wire target=direct: left out: {error}"),
    }
    println!(
        "wire: DeepSeek's {model}, thinking off, {count} functions on the lamp's request, {runs} runs, {how}"
    );
    Ok(targets)
}

/// The speech's targets: the route's own (`tts_base_url`: the relay,
/// unless that is ElevenLabs' own API) and, with an
/// [`ELEVENLABS_CREDENTIAL`] unit credential, ElevenLabs' own API, each
/// asked [`PIECES`] with the route's voice and model.
fn speech_targets(
    config: &SecondConfig,
    credentials: Option<&OsStr>,
    runs: u32,
    how: &str,
) -> io::Result<Vec<(&'static str, Vec<Kind>)>> {
    if config.tts_provider != TtsProvider::ElevenLabs {
        println!(
            "wire speech: left out: the route's speech is OpenAI's, and only ElevenLabs' is timed"
        );
        return Ok(Vec::new());
    }
    let model = config
        .tts_model
        .clone()
        .unwrap_or_else(|| net::default_model(TtsProvider::ElevenLabs).to_owned());
    // The kind, and whether it is on ElevenLabs' own API.
    let kind = |base: &str, key: &str| -> io::Result<(Kind, bool)> {
        let (base, key) = (base.to_owned(), key.to_owned());
        let (voice, model) = (config.tts_voice.clone(), model.clone());
        let request =
            move |run: u32| Wired::speech(piece(&base, &key, voice.as_deref(), &model, run)?);
        let own = on_elevenlabs(&request(0)?.url);
        let kind = Kind {
            before: "speech ",
            after: String::new(),
            reading: Reading::Audio,
            request: Box::new(request),
        };
        Ok((kind, own))
    };
    let mut targets = Vec::new();
    let (route, own) = kind(&config.tts_base_url, config.tts_key.expose())?;
    if own {
        println!(
            "wire speech target=relay: left out: the route's speech goes to ElevenLabs' own API"
        );
        targets.push(("direct", vec![route]));
    } else {
        targets.push(("relay", vec![route]));
        match unit_credential(credentials, ELEVENLABS_CREDENTIAL) {
            Ok(key) => targets.push(("direct", vec![kind(ELEVENLABS_BASE_URL, key.expose())?.0])),
            Err(error) => println!("wire speech target=direct: left out: {error}"),
        }
    }
    let characters = PIECES.map(|piece| piece.chars().count());
    println!(
        "wire speech: ElevenLabs' {model}, pieces of {} to {} characters, {runs} runs, {how}",
        characters.iter().min().unwrap_or(&0),
        characters.iter().max().unwrap_or(&0),
    );
    Ok(targets)
}

/// Each request on a connection of its own.
fn run_cold(prepared: &Prepared, runs: u32) -> io::Result<()> {
    let Prepared { targets, tls } = prepared;
    let kinds = targets.first().map_or(0, |(_, kinds)| kinds.len());
    let mut kept: Vec<Vec<Kept>> = targets
        .iter()
        .map(|_| (0..kinds).map(|_| Kept::default()).collect())
        .collect();
    for run in 1..=runs {
        for index in 0..kinds {
            for ((name, kinds), kept) in targets.iter().zip(&mut kept) {
                let kind = &kinds[index];
                let label = kind.named("", &format!("target={name}"));
                let wired = (kind.request)(run)?;
                let kept = &mut kept[index];
                kept.bytes = wired.body.len();
                match ask(&wired, Some(tls.clone())) {
                    Ok(times) => {
                        println!(
                            "wire {label} run={run} {}",
                            times.shown(wired.body.len(), kind.reading)
                        );
                        kept.answered(&times);
                    }
                    Err(error) => {
                        println!("wire {label} run={run} failed: {error}");
                        kept.failed += 1;
                    }
                }
            }
        }
    }
    for ((name, kinds), kept) in targets.iter().zip(&kept) {
        for (kind, kept) in kinds.iter().zip(kept) {
            let label = kind.named("", &format!("target={name}"));
            println!("{}", kept.summary(&label, kind.reading));
        }
    }
    if let [(_, kinds), _] = targets.as_slice() {
        for (index, kind) in kinds.iter().enumerate() {
            println!(
                "{}",
                less(
                    &kind.named("", "relay-less-direct"),
                    &kept[0][index],
                    &kept[1][index],
                    kind.reading,
                )
            );
        }
    }
    Ok(())
}

/// What became of one arm's request on a kept connection ([`kept_arms`]).
#[derive(Debug)]
pub enum Asked {
    /// Answered 200: its times, and the request's size.
    Timed(Times, usize),
    /// Not timed: refused (its status; 0 when nothing came), or not made
    /// because the connection was no longer kept for it (`None`).
    Closed(Option<u16>),
    /// A step failed, told by its name and kind.
    Failed(String),
}

/// One run's three arms ([`Arm::ALL`]) on `connection`, whose making took
/// `connected`: `wired(at)` is each arm's request, and the idle one waits
/// `idle` first. Only a 200 is timed, so a refused answer never mixes into
/// an arm's medians. Once an answer doesn't keep the connection, or a post
/// fails, the arms after it are [`Asked::Closed`]: a real turn would then pay
/// a new connection (the reviewer on #2044).
pub fn kept_arms(
    connection: Connection,
    connected: Times,
    wired: impl Fn(usize) -> io::Result<Wired>,
    idle: Duration,
) -> io::Result<Vec<Asked>> {
    let mut connection = Some(connection);
    let mut asked = Vec::with_capacity(Arm::ALL.len());
    for (at, arm) in Arm::ALL.into_iter().enumerate() {
        let Some(open) = connection.as_mut() else {
            asked.push(Asked::Closed(None));
            continue;
        };
        if arm == Arm::Idle {
            std::thread::sleep(idle);
        }
        let wired = wired(at)?;
        let posted = open.post(&wired, true);
        match posted {
            Ok((send, answer)) => {
                if !answer.kept {
                    connection = None;
                }
                if answer.status == 200 {
                    let mut times = if arm == Arm::First {
                        connected
                    } else {
                        Times::default()
                    };
                    times.send = send;
                    times.answer = answer;
                    asked.push(Asked::Timed(times, wired.body.len()));
                } else {
                    asked.push(Asked::Closed(Some(answer.status)));
                }
            }
            Err(error) => {
                connection = None;
                asked.push(Asked::Failed(error.to_string()));
            }
        }
    }
    Ok(asked)
}

/// Three requests a target and kind on one kept connection ([`Arm`]).
fn run_kept(prepared: &Prepared, runs: u32) -> io::Result<()> {
    let Prepared { targets, tls } = prepared;
    let kinds = targets.first().map_or(0, |(_, kinds)| kinds.len());
    // Target, then kind, then arm.
    let mut kept: Vec<Vec<[Kept; 3]>> = targets
        .iter()
        .map(|_| (0..kinds).map(|_| Default::default()).collect())
        .collect();
    for run in 1..=runs {
        for index in 0..kinds {
            for ((name, kinds), kept) in targets.iter().zip(&mut kept) {
                let kind = &kinds[index];
                let kept = &mut kept[index];
                let label = kind.named("kept ", &format!("target={name}"));
                let line = |arm: Arm| format!("wire {label} run={run} arm={}", arm.name());
                // Each request its own changing part, as turns are.
                let wired = |at: usize| (kind.request)(run * 3 + at as u32);
                let url = wired(0)?.url;
                let mut times = Times::default();
                let connection = match Connection::open(&url, Some(tls.clone()), &mut times) {
                    Ok(connection) => connection,
                    Err(error) => {
                        println!("{} failed: {error}", line(Arm::First));
                        kept.iter_mut().for_each(|arm| arm.failed += 1);
                        continue;
                    }
                };
                let asked = kept_arms(connection, times, wired, IDLE)?;
                for ((at, arm), asked) in Arm::ALL.into_iter().enumerate().zip(asked) {
                    match asked {
                        Asked::Timed(times, bytes) => {
                            println!("{} {}", line(arm), times.shown(bytes, kind.reading));
                            kept[at].bytes = bytes;
                            kept[at].keep(&times);
                            if !times.answer.kept {
                                println!("{}: the service did not keep the connection", line(arm));
                            }
                        }
                        Asked::Closed(Some(status)) => {
                            println!("{} not timed: answered {status}", line(arm));
                            kept[at].closed += 1;
                        }
                        Asked::Closed(None) => kept[at].closed += 1,
                        Asked::Failed(error) => {
                            println!("{} failed: {error}", line(arm));
                            kept[at].failed += 1;
                        }
                    }
                }
            }
        }
    }
    for ((name, kinds), kept) in targets.iter().zip(&kept) {
        for (kind, kept) in kinds.iter().zip(kept) {
            let label = kind.named("kept ", &format!("target={name}"));
            for (at, arm) in Arm::ALL.into_iter().enumerate() {
                println!(
                    "{}",
                    kept[at].summary(&format!("{label} arm={}", arm.name()), kind.reading)
                );
            }
            // What restarting slow start after idle costs a turn.
            println!(
                "{}",
                less(
                    &kind.named("kept ", &format!("idle-less-straight target={name}")),
                    &kept[2],
                    &kept[1],
                    kind.reading,
                )
            );
        }
    }
    if let [(_, kinds), _] = targets.as_slice() {
        for (index, kind) in kinds.iter().enumerate() {
            for (at, arm) in Arm::ALL.into_iter().enumerate() {
                println!(
                    "{}",
                    less(
                        &format!(
                            "{} arm={}",
                            kind.named("kept ", "relay-less-direct"),
                            arm.name()
                        ),
                        &kept[0][index][at],
                        &kept[1][index][at],
                        kind.reading,
                    )
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// A chat request of `body` to `url`, as the route's key header gives it.
    fn wired(url: &reqwest::Url, body: &[u8]) -> Wired {
        Wired {
            url: url.clone(),
            header: "Authorization",
            value: "Bearer k".to_owned(),
            body: body.to_vec(),
            reading: Reading::Events,
        }
    }

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
        let answer = read_answer(&mut reader, Instant::now(), Reading::Events).unwrap();
        assert_eq!(answer.status, 200);
        assert_eq!(answer.events, 3, "{answer:?}");
        let (first, second) = (answer.first_event.unwrap(), answer.second_event.unwrap());
        assert_eq!(answer.first_byte, Some(first), "the head came with it");
        // The second event came whole only with the third read after it.
        assert!(second >= first + 45, "{answer:?}");
        assert_eq!(answer.words, Some(second));
        assert_eq!(answer.gap(), Some(second - first));
        assert!(reader.parts.is_empty(), "read to the body's end");
        assert!(answer.kept, "chunked, read whole, not told to close");
    }

    /// `--kept`: a chunked body ends with the empty line after its last
    /// chunk and any trailers, so the next answer starts clean.
    #[test]
    fn a_chunked_body_ends_with_the_line_after_its_last_chunk() {
        let mut chunks = Chunked::default();
        let mut out = Vec::new();
        chunks.feed(b"5\r\nhello\r\n0\r\n", &mut out).unwrap();
        assert!(!chunks.ended);
        chunks.feed(b"\r\n", &mut out).unwrap();
        assert!(chunks.ended);
        assert_eq!(out, b"hello");
        let mut trailed = Chunked::default();
        trailed
            .feed(b"0\r\nX-Trailer: 1\r\n\r\n", &mut Vec::new())
            .unwrap();
        assert!(trailed.ended);
    }

    /// `--kept`: an answer read to its end keeps its connection, unless it
    /// said to close; a body by its length is read to that length.
    #[test]
    fn an_answer_read_to_its_end_keeps_its_connection_unless_told_to_close() {
        let read = |parts: Vec<Vec<u8>>| {
            let mut reader = Parts {
                parts: parts.into(),
                pause: Duration::ZERO,
            };
            let answer = read_answer(&mut reader, Instant::now(), Reading::Events).unwrap();
            (answer, reader.parts.len())
        };
        let mut closing = chunked(&[WORDS, "data: [DONE]"]);
        closing[0] =
            b"HTTP/1.1 200 OK\r\nConnection: close\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
        let (answer, left) = read(closing);
        assert_eq!((answer.events, answer.kept, left), (1, false, 0));
        let body = format!("{WORDS}\n\ndata: [DONE]\n\n");
        let (answer, left) = read(vec![
            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes(),
            body.into_bytes(),
            b"not this answer's".to_vec(),
        ]);
        assert_eq!((answer.events, answer.kept, left), (1, true, 1));
        let (answer, _) = read(vec![
            b"HTTP/1.1 429 Too Many\r\nContent-Length: 2\r\n\r\n{}".to_vec(),
        ]);
        assert_eq!((answer.status, answer.kept), (429, false));
    }

    /// A local server on one connection: each of `answers` in turn, a status
    /// and whether it keeps the connection. Gives its port, and on join
    /// whether no second connection was asked for.
    fn serving(answers: Vec<(u16, bool)>) -> (u16, std::thread::JoinHandle<bool>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut asked = Vec::new();
            let mut buffer = [0u8; 4096];
            for (status, keep) in answers {
                let end = loop {
                    if let Some(end) = asked.windows(4).position(|four| four == b"\r\n\r\n") {
                        break end;
                    }
                    let n = socket.read(&mut buffer).unwrap();
                    asked.extend_from_slice(&buffer[..n]);
                };
                let head = String::from_utf8(asked[..end].to_vec()).unwrap();
                let length: usize = head
                    .lines()
                    .find_map(|line| line.strip_prefix("Content-Length: "))
                    .unwrap()
                    .parse()
                    .unwrap();
                while asked.len() < end + 4 + length {
                    let n = socket.read(&mut buffer).unwrap();
                    asked.extend_from_slice(&buffer[..n]);
                }
                asked.drain(..end + 4 + length);
                let close = if keep { "" } else { "Connection: close\r\n" };
                let answer = if status == 200 {
                    let mut parts = chunked(&[ROLE, WORDS, "data: [DONE]"]);
                    parts[0] =
                        format!("HTTP/1.1 200 OK\r\n{close}Transfer-Encoding: chunked\r\n\r\n")
                            .into_bytes();
                    parts.concat()
                } else {
                    format!("HTTP/1.1 {status} No\r\n{close}Content-Length: 2\r\n\r\n{{}}")
                        .into_bytes()
                };
                socket.write_all(&answer).unwrap();
                if !keep {
                    break;
                }
            }
            listener.set_nonblocking(true).unwrap();
            listener.accept().is_err()
        });
        (port, server)
    }

    /// The reviewer's P3s on #2044: only a 200 is timed, and once the
    /// connection isn't kept the arms after it are closed, never made on a
    /// new one.
    #[test]
    fn the_arms_time_only_200s_and_stop_where_the_connection_is_not_kept() {
        let arms = |answers: Vec<(u16, bool)>| {
            let (port, server) = serving(answers);
            let url = reqwest::Url::parse(&format!("http://127.0.0.1:{port}/v1/chat/completions"))
                .unwrap();
            let mut connected = Times::default();
            let connection = Connection::open(&url, None, &mut connected).unwrap();
            let asked = kept_arms(
                connection,
                connected,
                |_| Ok(wired(&url, b"{}")),
                Duration::ZERO,
            )
            .unwrap();
            assert!(server.join().unwrap(), "one connection");
            asked
                .into_iter()
                .map(|asked| match asked {
                    Asked::Timed(times, _) => format!("timed kept={}", times.answer.kept),
                    Asked::Closed(status) => format!("closed {status:?}"),
                    Asked::Failed(error) => format!("failed {error}"),
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            arms(vec![(200, true), (200, true), (200, true)]),
            ["timed kept=true", "timed kept=true", "timed kept=true"]
        );
        assert_eq!(
            arms(vec![(200, true), (200, false)]),
            ["timed kept=true", "timed kept=false", "closed None"]
        );
        assert_eq!(
            arms(vec![(401, true)]),
            ["closed Some(401)", "closed None", "closed None"]
        );
    }

    /// `--kept`: two requests go on one connection, each answered whole.
    #[test]
    fn a_kept_connection_carries_a_second_request() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut heads = Vec::new();
            let mut asked = Vec::new();
            let mut buffer = [0u8; 4096];
            while heads.len() < 2 {
                let end = loop {
                    if let Some(end) = asked.windows(4).position(|four| four == b"\r\n\r\n") {
                        break end;
                    }
                    let n = socket.read(&mut buffer).unwrap();
                    asked.extend_from_slice(&buffer[..n]);
                };
                let head = String::from_utf8(asked[..end].to_vec()).unwrap();
                let length: usize = head
                    .lines()
                    .find_map(|line| line.strip_prefix("Content-Length: "))
                    .unwrap()
                    .parse()
                    .unwrap();
                while asked.len() < end + 4 + length {
                    let n = socket.read(&mut buffer).unwrap();
                    asked.extend_from_slice(&buffer[..n]);
                }
                asked.drain(..end + 4 + length);
                heads.push(head);
                let answer: Vec<u8> = chunked(&[ROLE, WORDS, "data: [DONE]"]).concat();
                socket.write_all(&answer).unwrap();
            }
            // A third connection is never asked for.
            listener.set_nonblocking(true).unwrap();
            (heads, listener.accept().is_err())
        });
        let url =
            reqwest::Url::parse(&format!("http://127.0.0.1:{port}/v1/chat/completions")).unwrap();
        let mut times = Times::default();
        let mut connection = Connection::open(&url, None, &mut times).unwrap();
        for _ in 0..2 {
            let (_, answer) = connection.post(&wired(&url, b"{}"), true).unwrap();
            assert_eq!((answer.status, answer.events, answer.kept), (200, 2, true));
        }
        let (heads, alone) = server.join().unwrap();
        assert!(
            heads
                .iter()
                .all(|head| head.contains("Connection: keep-alive")),
            "{heads:?}"
        );
        assert!(alone, "both on one connection");
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
        let answer = read_answer(&mut reader, Instant::now(), Reading::Events).unwrap();
        assert_eq!((answer.status, answer.events), (401, 0));
        assert_eq!(reader.parts.len(), 1);
        let mut garbled = Parts {
            parts: [b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n".to_vec()].into(),
            pause: Duration::ZERO,
        };
        assert!(read_answer(&mut garbled, Instant::now(), Reading::Events).is_err());
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
        let times = ask(&wired(&url, body), None).unwrap();
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
                .shown(body.len(), Reading::Events)
                .starts_with("bytes=27 status=200 dns_ms=")
        );
    }

    #[test]
    fn a_failed_step_is_told_by_its_kind_without_the_host() {
        let url = reqwest::Url::parse("https://127.0.0.1:9/v1/chat/completions").unwrap();
        let error = ask(&wired(&url, b"{}"), None).unwrap_err();
        // Port 9 refuses, or there is no TLS configuration: either way no
        // address is in the words.
        assert!(!error.to_string().contains("127.0.0.1"), "{error}");
    }

    /// The reviewer's P3 on #2024: an error's own words can name the host
    /// (rustls's name mismatch does), and only the step and kind are told.
    #[test]
    fn a_failed_steps_words_are_its_name_and_kind_alone() {
        let named = io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid peer certificate: certificate not valid for name \"relay.example.invalid\"",
        );
        let told = step("tls")(named);
        assert_eq!(told.to_string(), "tls: InvalidData");
        assert_eq!(told.kind(), io::ErrorKind::InvalidData);
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
                kept: true,
                ..Answer::default()
            },
        };
        let mut relay = Kept::default();
        let mut direct = Kept::default();
        for (tcp, first_byte) in [(30, 900), (40, 950), (35, 1_000)] {
            relay.keep(&times(tcp, first_byte));
            direct.keep(&times(tcp - 20, first_byte - 600));
        }
        relay.failed = 1;
        assert!(relay.summary("target=relay size=lamp", Reading::Events).starts_with(
            "wire target=relay size=lamp runs=4 failed=1 bytes=0 tcp_p50_ms=35 tls_p50_ms=70 "
        ));
        assert_eq!(
            less(
                "relay-less-direct size=lamp",
                &relay,
                &direct,
                Reading::Events
            ),
            "wire relay-less-direct size=lamp tcp_ms=+20 tls_ms=+40 send_ms=+0 first_byte_ms=+600 first_event_ms=+600 gap_ms=+0 words_ms=+600"
        );
        assert!(
            less(
                "relay-less-direct size=small",
                &relay,
                &Kept::default(),
                Reading::Events
            )
            .ends_with("words_ms=none")
        );
        // `--kept`'s lines: an arm's label, and requests a closed
        // connection could not carry counted apart.
        relay.closed = 2;
        let line = relay.summary("kept target=relay size=lamp arm=idle", Reading::Events);
        assert!(
            line.starts_with("wire kept target=relay size=lamp arm=idle runs=6 failed=1 "),
            "{line}"
        );
        assert!(line.ends_with(" closed=2"), "{line}");
        assert!(
            less(
                "kept idle-less-straight target=relay size=lamp",
                &relay,
                &direct,
                Reading::Events,
            )
            .starts_with("wire kept idle-less-straight target=relay size=lamp tcp_ms=+20 ")
        );
    }

    /// `--speech`: a piece's answer is timed to its first audio byte and
    /// read to its body's end; audio that happens to read like an event is
    /// never taken for one.
    #[test]
    fn a_speech_answer_is_timed_to_its_first_audio_and_read_to_its_end() {
        let audio = |bytes: &[u8]| {
            let mut chunk = format!("{:x}\r\n", bytes.len()).into_bytes();
            chunk.extend_from_slice(bytes);
            chunk.extend_from_slice(b"\r\n");
            chunk
        };
        // The head at once, then the audio, as a voice is made.
        let mut reader = Parts {
            parts: [
                b"HTTP/1.1 200 OK\r\nContent-Type: audio/pcm\r\nTransfer-Encoding: chunked\r\n\r\n"
                    .to_vec(),
                audio(&[0u8; 960]),
                audio(b"\ndata: [DONE]\n"),
                audio(&[1u8; 480]),
                b"0\r\n\r\n".to_vec(),
            ]
            .into(),
            pause: Duration::from_millis(20),
        };
        let answer = read_answer(&mut reader, Instant::now(), Reading::Audio).unwrap();
        assert_eq!((answer.status, answer.events), (200, 0));
        assert_eq!(answer.audio_bytes, 960 + 14 + 480);
        let (first_byte, first_audio) = (answer.first_byte.unwrap(), answer.audio.unwrap());
        assert!(first_audio >= first_byte + 15, "{answer:?}");
        assert!(answer.done >= first_audio + 45, "read past it: {answer:?}");
        assert!(answer.kept && reader.parts.is_empty(), "{answer:?}");
        // With no length and no chunks, the audio ends with the connection.
        let mut closing = Parts {
            parts: [b"HTTP/1.1 200 OK\r\n\r\n".to_vec(), vec![7u8; 100]].into(),
            pause: Duration::ZERO,
        };
        let answer = read_answer(&mut closing, Instant::now(), Reading::Audio).unwrap();
        assert_eq!((answer.audio_bytes, answer.kept), (100, false));
    }

    /// `--speech` asks as the route's `Tts` does: through the relay's
    /// `/elevenlabs` path or on ElevenLabs' own API, the key in `xi-api-key`,
    /// the route's model, one piece a run.
    #[test]
    fn a_piece_of_speech_is_asked_as_the_route_asks_for_it() {
        let relay = piece(
            "https://relay.example.invalid/api/v1/ai/v1",
            "k",
            Some("Rachel"),
            "eleven_v4_turbo",
            1,
        )
        .unwrap();
        let relay = Wired::speech(relay).unwrap();
        assert_eq!(
            relay.url.as_str(),
            "https://relay.example.invalid/api/v1/ai/v1/elevenlabs/text-to-speech/21m00Tcm4TlvDq8ikWAM/stream?output_format=pcm_24000"
        );
        assert!(!on_elevenlabs(&relay.url));
        assert_eq!(
            (relay.header, relay.value.as_str(), relay.reading),
            ("xi-api-key", "k", Reading::Audio)
        );
        let body: Value = serde_json::from_slice(&relay.body).unwrap();
        assert_eq!(body["model_id"], "eleven_v4_turbo");
        assert_eq!(body["text"], PIECES[1]);
        let direct = piece(ELEVENLABS_BASE_URL, "k", None, "eleven_flash_v2_5", 5).unwrap();
        let direct = Wired::speech(direct).unwrap();
        assert!(
            direct
                .url
                .as_str()
                .starts_with("https://api.elevenlabs.io/v1/text-to-speech/"),
            "{}",
            direct.url
        );
        assert!(on_elevenlabs(&direct.url));
        let body: Value = serde_json::from_slice(&direct.body).unwrap();
        assert_eq!(body["text"], PIECES[1], "run 5 is run 1's piece again");
        for text in PIECES {
            assert!((30..=45).contains(&text.chars().count()), "{text}");
        }
    }

    /// The piece goes with its own key header and the audio's `Accept`, on
    /// its path and query, and its answer is counted in audio bytes.
    #[test]
    fn a_speech_request_is_made_by_hand_with_its_own_key_header() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut asked = Vec::new();
            let mut buffer = [0u8; 4096];
            let end = loop {
                if let Some(end) = asked.windows(4).position(|four| four == b"\r\n\r\n") {
                    break end;
                }
                let n = socket.read(&mut buffer).unwrap();
                asked.extend_from_slice(&buffer[..n]);
            };
            // The body, read whole before answering.
            while asked.len() < end + 4 + 2 {
                let n = socket.read(&mut buffer).unwrap();
                asked.extend_from_slice(&buffer[..n]);
            }
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\n\x01\x02\x03\x04")
                .unwrap();
            String::from_utf8(asked[..end].to_vec()).unwrap()
        });
        let url = reqwest::Url::parse(&format!(
            "http://127.0.0.1:{port}/v1/text-to-speech/abc/stream?output_format=pcm_24000"
        ))
        .unwrap();
        let wired = Wired {
            url,
            header: "xi-api-key",
            value: "k".to_owned(),
            body: b"{}".to_vec(),
            reading: Reading::Audio,
        };
        let times = ask(&wired, None).unwrap();
        let head = server.join().unwrap();
        assert!(
            head.starts_with(
                "POST /v1/text-to-speech/abc/stream?output_format=pcm_24000 HTTP/1.1\r\n"
            ),
            "{head}"
        );
        for line in ["xi-api-key: k", "Accept: */*", "Connection: close"] {
            assert!(head.contains(line), "{line} in {head}");
        }
        assert!(!head.contains("Authorization"), "{head}");
        assert_eq!((times.answer.status, times.answer.audio_bytes), (200, 4));
        let shown = times.shown(2, Reading::Audio);
        assert!(shown.starts_with("bytes=2 status=200 dns_ms="), "{shown}");
        assert!(shown.ends_with(" audio_bytes=4"), "{shown}");
        assert!(!shown.contains("events"), "{shown}");
    }

    /// `--speech`'s summary gives the first audio and the end, and the
    /// relay's extra on each, never a chat's events or words.
    #[test]
    fn a_speech_summary_gives_its_first_audio_and_its_end() {
        let times = |first_byte, audio| Times {
            dns: 1,
            tcp: 10,
            tls: 20,
            send: 1,
            answer: Answer {
                status: 200,
                first_byte: Some(first_byte),
                done: audio + 300,
                kept: true,
                audio: Some(audio),
                audio_bytes: 96_000,
                ..Answer::default()
            },
        };
        let mut relay = Kept::default();
        let mut direct = Kept::default();
        for (first_byte, audio) in [(400, 700), (420, 760), (380, 720)] {
            relay.keep(&times(first_byte, audio));
            direct.keep(&times(first_byte - 300, audio - 350));
        }
        let line = relay.summary("speech target=relay", Reading::Audio);
        assert!(
            line.starts_with("wire speech target=relay runs=3 failed=0 bytes=0 tcp_p50_ms=10 "),
            "{line}"
        );
        assert!(
            line.ends_with(
                " first_byte_p50_ms=400 first_byte_p90_ms=420 first_audio_p50_ms=720 \
                 first_audio_p90_ms=760 done_p50_ms=1020 done_p90_ms=1060 audio_bytes_p50=96000"
            ),
            "{line}"
        );
        assert!(!line.contains("words") && !line.contains("event"), "{line}");
        assert_eq!(
            less("speech relay-less-direct", &relay, &direct, Reading::Audio),
            "wire speech relay-less-direct tcp_ms=+0 tls_ms=+0 send_ms=+0 first_byte_ms=+300 first_audio_ms=+350 done_ms=+350"
        );
        // A chat's summary is as before, with no audio in it.
        assert!(
            !relay
                .summary("target=relay size=lamp", Reading::Events)
                .contains("audio")
        );
        // On connections of their own too, only a 200 is timed: a quick
        // refusal is counted apart.
        let mut cold = Kept::default();
        cold.answered(&times(400, 700));
        let mut refused = times(90, 90);
        refused.answer.status = 401;
        cold.answered(&refused);
        let line = cold.summary("speech target=relay", Reading::Audio);
        assert!(line.contains(" runs=2 failed=0 "), "{line}");
        assert!(line.contains(" first_byte_p50_ms=400 "), "{line}");
        assert!(line.ends_with(" closed=1"), "{line}");
    }
}
