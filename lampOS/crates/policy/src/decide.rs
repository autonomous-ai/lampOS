//! Asking a decision model.
//!
//! A decision model is shown a `state`, one JSON value that says how things
//! stand, and questions about it. It answers each question with
//! probabilities and never with text: a yes-or-no question with the
//! probability of yes, a choice with a probability for each option. That is
//! what lets the policy hold an answer to a threshold, combine several, and
//! fit the combining again later from what was written down.
//!
//! [`Decider`] is the one way of asking, whatever the model: the policy
//! asks every judge through it. [`Jev`] is the first, TypeSafe's Jev reached
//! through OpenRouter over HTTP.
//!
//! Nothing here knows what the questions are about. Nothing here keeps or
//! prints what a state says, either: a failure is a kind and, at most, a
//! status or the name of a check, never anything the service sent.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fmt,
    io::{self, Read, Write},
    net::{IpAddr, Ipv6Addr, SocketAddr, TcpStream, ToSocketAddrs},
    path::Path,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

/// Where Jev is asked unless the caller names another place.
pub const DEFAULT_ENDPOINT: &str = "https://openrouter.ai/api/alpha/decisions";
/// The model asked for unless the caller names another.
pub const DEFAULT_MODEL: &str = "typesafe/jev-1.13";
/// How long one asking may take, from looking the name up to the last
/// byte read. Measured from the office's network, outside this program,
/// the service answered in 0.66 s in the middle and under 0.76 s nine
/// times in ten, and now and then took seconds.
pub const DEFAULT_DEADLINE: Duration = Duration::from_secs(3);
/// The largest answer that is read. A real one is a few hundred bytes.
pub const MOST_ANSWER_BYTES: usize = 256 * 1024;

/// The status line and the headers of an answer are at most this long.
const MOST_HEAD_BYTES: usize = 16 * 1024;
const MOST_HEADERS: usize = 96;
/// A chunk's length is a short line: more than this without its end is not
/// one.
const MOST_CHUNK_LINE_BYTES: usize = 1024;
/// A model's name, as its service reports it, is no longer than this.
const MOST_NAME_BYTES: usize = 200;
/// A choice's probabilities add up to 1, give or take this.
const SUM_WITHIN: f64 = 0.02;
/// How long the addresses a name was looked up to are used before it is
/// looked up again.
const KNOWN_FOR: Duration = Duration::from_secs(300);
/// No more addresses of one name than this are tried in one asking.
const MOST_ADDRESSES: usize = 4;

const LOOKING_UP: &str = "looking up the name";
const CONNECTING: &str = "connecting";
const SECURING: &str = "making the connection secure";
const SENDING: &str = "sending";
const READING: &str = "reading";
const TOO_LARGE: &str = "it is larger than 256 KiB";
const ENDED_EARLY: &str = "it ended before it was whole";
const NOT_A_PROBABILITY: &str = "a probability is not a number from 0 to 1";
const ANOTHER_TYPE: &str = "an answer is of another type than its question";

/// The header TypeSafe asks for with a picture (#871); sent only then.
pub const IMAGES_HEADER: &str = "TypeSafe-Danger-Zone-Images: true";

/// A picture sent with a question (#871), as TypeSafe's `assets` take one:
/// `key` is a string the state or a question holds where the picture
/// belongs, and the service puts the picture there. A JPEG's bytes. It is
/// never shown: its `Debug` says its key and size only.
#[derive(Clone, PartialEq, Eq)]
pub struct Picture {
    pub key: String,
    pub jpeg: Vec<u8>,
}

impl fmt::Debug for Picture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Picture {{ key: {:?}, bytes: {} }}",
            self.key,
            self.jpeg.len()
        )
    }
}

/// One question put to a decision model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Question {
    /// Yes or no. It is answered with the probability of yes.
    Noul { instructions: String },
    /// One of several options. `criteria` are the options, each with what
    /// it means, in the order the model is shown them.
    Choice {
        instructions: String,
        criteria: Vec<(String, String)>,
    },
}

/// What a decision model answered to one question.
#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    /// The probability of yes, from 0 to 1.
    Noul(f32),
    /// The option the model holds most likely, and the probability of each
    /// option it gave one for.
    Choice {
        choice: String,
        probabilities: BTreeMap<String, f32>,
    },
}

/// What an asking cost, as far as the service said.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    /// In the service's own unit of money.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
}

/// What an asking was sent on, for a judge that is reached over one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Connection {
    /// Made for this asking. Making it is part of how long the asking took.
    New,
    /// Made ahead of the question ([`Decider::ready`]), and not used before.
    Ahead,
    /// Kept from an asking before.
    Kept,
    /// A kept one had ended: that was found out and another made, all
    /// within this asking.
    Replaced,
}

/// Every question answered.
#[derive(Clone, Debug, PartialEq)]
pub struct Asked {
    /// The answer to each question that was asked, by the question's name.
    pub answers: BTreeMap<String, Answer>,
    /// The model that answered, as the service names it: it can be a more
    /// exact name than the one asked for.
    pub model: String,
    /// From the start of the asking to the answers checked.
    pub took: Duration,
    pub usage: Option<Usage>,
    /// What it was sent on, if the judge is reached over a connection.
    pub connection: Option<Connection>,
}

/// Why there are no answers. None of these carries anything the service
/// sent or anything of the key, so any of them can be printed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failed {
    /// The service was not reached, or the connection failed: while doing
    /// what, and the kind of error.
    Unreachable {
        during: &'static str,
        kind: io::ErrorKind,
    },
    /// The deadline passed first.
    TimedOut,
    /// The service answered with this HTTP status and not 200.
    Refused(u16),
    /// The service sent something that is not a set of answers: which
    /// check it failed.
    Unreadable(&'static str),
    /// The answers leave out this question.
    Missing(String),
    /// What was to be sent was no longer permitted when it was to be
    /// written (a picture, #871): nothing was sent.
    Withdrawn,
}

impl Failed {
    /// The kind as one word, for a log and a count.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Unreachable { .. } => "unreachable",
            Self::TimedOut => "timed_out",
            Self::Refused(_) => "refused",
            Self::Unreadable(_) => "unreadable",
            Self::Missing(_) => "missing",
            Self::Withdrawn => "withdrawn",
        }
    }
}

impl fmt::Display for Failed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreachable { during, kind } => {
                write!(
                    f,
                    "the decision model was not reached while {during}: {kind}"
                )
            }
            Self::TimedOut => f.write_str("the decision model did not answer before the deadline"),
            Self::Refused(status) => {
                write!(f, "the decision model's service refused with HTTP {status}")
            }
            Self::Unreadable(why) => {
                write!(f, "the decision model's answer cannot be used: {why}")
            }
            Self::Missing(question) => {
                write!(f, "the decision model's answer leaves out {question}")
            }
            Self::Withdrawn => {
                f.write_str("what was to be sent was no longer permitted: nothing was sent")
            }
        }
    }
}

impl std::error::Error for Failed {}

/// A decision model, or anything that judges as one does.
pub trait Decider {
    /// Ask `questions` about `state`, and return within `deadline`.
    ///
    /// `state` is JSON, as the text that is sent: the order of an object's
    /// fields is part of what a model reads, and weights fitted to a
    /// model's answers belong to the text it was shown. A question's name
    /// is the caller's own: the model is not shown it.
    ///
    /// Every question is answered, each as its type, or the whole asking
    /// has failed: a caller is never handed half of what it asked.
    fn ask(
        &mut self,
        state: &str,
        questions: &[(String, Question)],
        deadline: Duration,
    ) -> Result<Asked, Failed>;

    /// A question is likely soon: do now, within `within`, whatever would
    /// otherwise be done when it is asked. Nothing is promised and nothing
    /// is told of it; an asking that follows finds out what it finds.
    fn ready(&mut self, within: Duration) {
        let _ = within;
    }

    /// What this judge is called, for a log.
    fn name(&self) -> &str;
}

/// Where a decision model's service is: `https://host[:port]/path`, or
/// `http://` for a server on this machine in a test.
///
/// Plain `http` to any other machine is refused. Without `https` the key
/// and the state would cross the network for anyone on it to read, and
/// one letter left out of an option must not be enough for that.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    secure: bool,
    /// A name, or an address without the brackets an IPv6 one has in a URL.
    host: String,
    port: u16,
    path: String,
}

impl Endpoint {
    pub fn parse(url: &str) -> Result<Self, String> {
        let refused = |why: &str| format!("endpoint {url:?} {why}");
        let (secure, rest) = match (url.strip_prefix("https://"), url.strip_prefix("http://")) {
            (Some(rest), _) => (true, rest),
            (_, Some(rest)) => (false, rest),
            _ => return Err(refused("does not begin with https:// or http://")),
        };
        if url
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '#')
        {
            return Err(refused("has a space, a control character or a fragment"));
        }
        let (authority, path) = match rest.find('/') {
            Some(at) => rest.split_at(at),
            None => (rest, "/"),
        };
        if authority.contains('@') {
            return Err(refused("names a user, which is not taken"));
        }
        let (host, port) = match authority.strip_prefix('[') {
            // An IPv6 address in brackets, and after them a port or nothing.
            Some(bracketed) => {
                let (host, after) = bracketed
                    .split_once(']')
                    .filter(|(host, _)| host.parse::<Ipv6Addr>().is_ok())
                    .ok_or_else(|| refused("has brackets that do not hold an IPv6 address"))?;
                match (after, after.strip_prefix(':')) {
                    ("", _) => (host, None),
                    (_, Some(port)) => (host, Some(port)),
                    _ => return Err(refused("has something after its address that is no port")),
                }
            }
            None => match authority.rsplit_once(':') {
                Some((host, port)) => (host, Some(port)),
                None => (authority, None),
            },
        };
        let port = match port {
            None if secure => 443,
            None => 80,
            Some(port) => port
                .parse::<u16>()
                .ok()
                .filter(|port| *port != 0)
                .ok_or_else(|| refused("has a port that is not a number from 1 to 65535"))?,
        };
        // A name, or an address: of letters, digits, dots and hyphens, or
        // the IPv6 address the brackets held.
        let named = !host.is_empty()
            && host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'));
        if !named && host.parse::<Ipv6Addr>().is_err() {
            return Err(refused("names no host"));
        }
        let this_machine = host == "localhost"
            || host
                .parse::<IpAddr>()
                .is_ok_and(|address| address.is_loopback());
        if !secure && !this_machine {
            return Err(refused(
                "is plain http to another machine, which would send the key and the state in the clear: use https, or http only to 127.0.0.1, ::1 or localhost",
            ));
        }
        Ok(Self {
            secure,
            host: host.to_owned(),
            port,
            path: path.to_owned(),
        })
    }

    /// The host as the `Host` header has it.
    fn authority(&self) -> String {
        let host = if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        match (self.secure, self.port) {
            (true, 443) | (false, 80) => host,
            (_, port) => format!("{host}:{port}"),
        }
    }
}

type Resolve = fn(&str, u16) -> io::Result<Vec<SocketAddr>>;

fn system_lookup(host: &str, port: u16) -> io::Result<Vec<SocketAddr>> {
    (host, port).to_socket_addrs().map(Iterator::collect)
}

/// A name's addresses, looked up on a thread of its own.
///
/// The system's lookup takes no deadline, and with the network gone it can
/// take many seconds. So it is waited for only as long as the asking has
/// left, and a lookup that has not come back is left to finish and picked
/// up by the next asking: there is never more than one outstanding.
struct Lookup {
    host: String,
    port: u16,
    resolve: Resolve,
    known: Option<(Vec<SocketAddr>, Instant)>,
    pending: Option<mpsc::Receiver<io::Result<Vec<SocketAddr>>>>,
}

impl Lookup {
    fn addresses(&mut self, until: Instant) -> Result<Vec<SocketAddr>, Failed> {
        if let Ok(address) = self.host.parse::<IpAddr>() {
            return Ok(vec![SocketAddr::new(address, self.port)]);
        }
        if let Some((addresses, at)) = &self.known
            && at.elapsed() < KNOWN_FOR
        {
            return Ok(addresses.clone());
        }
        let not_found = |kind| Failed::Unreachable {
            during: LOOKING_UP,
            kind,
        };
        let pending = match self.pending.take() {
            Some(pending) => pending,
            None => {
                let (answer, pending) = mpsc::channel();
                let (host, port, resolve) = (self.host.clone(), self.port, self.resolve);
                std::thread::Builder::new()
                    .name("lookup".into())
                    .spawn(move || {
                        // Nobody may be waiting any more.
                        let _ = answer.send(resolve(&host, port));
                    })
                    .map_err(|error| not_found(error.kind()))?;
                pending
            }
        };
        match pending.recv_timeout(until.saturating_duration_since(Instant::now())) {
            Ok(Ok(mut addresses)) if !addresses.is_empty() => {
                addresses.truncate(MOST_ADDRESSES);
                self.known = Some((addresses.clone(), Instant::now()));
                Ok(addresses)
            }
            Ok(Ok(_)) => Err(not_found(io::ErrorKind::NotFound)),
            Ok(Err(error)) => Err(not_found(error.kind())),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.pending = Some(pending);
                Err(Failed::TimedOut)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(not_found(io::ErrorKind::Other)),
        }
    }

    /// The addresses did not work: the name is looked up again next time.
    fn forget(&mut self) {
        self.known = None;
    }
}

/// A read or a write that ran into the deadline, or failed otherwise.
fn failed(during: &'static str, error: &io::Error) -> Failed {
    match error.kind() {
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => Failed::TimedOut,
        kind => Failed::Unreachable { during, kind },
    }
}

/// Each address in turn, each given an even share of what time is left.
fn connect(addresses: &[SocketAddr], until: Instant) -> Result<TcpStream, Failed> {
    let mut last = io::ErrorKind::AddrNotAvailable;
    for (index, address) in addresses.iter().enumerate() {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(Failed::TimedOut);
        }
        let share = left / (addresses.len() - index) as u32;
        match TcpStream::connect_timeout(address, share.max(Duration::from_millis(1))) {
            Ok(socket) => return Ok(socket),
            Err(error) => last = error.kind(),
        }
    }
    Err(failed(CONNECTING, &io::Error::from(last)))
}

/// A connection on which no read and no write outlasts the deadline. Each
/// is given what time is left, so many small ones cannot add up to more.
struct Timed {
    socket: TcpStream,
    until: Instant,
}

impl Timed {
    fn left(&self) -> io::Result<Duration> {
        let left = self.until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        Ok(left)
    }
}

impl Read for Timed {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.socket.set_read_timeout(Some(self.left()?))?;
        self.socket.read(buffer)
    }
}

impl Write for Timed {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.socket.set_write_timeout(Some(self.left()?))?;
        self.socket.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.socket.flush()
    }
}

/// How an answer says where its body ends.
enum Framing {
    Length(usize),
    Chunked,
}

/// The status line and the headers, once they are whole: where the body
/// begins and how its end is told. Anything but 200 is a refusal, and its
/// body is not read.
fn head(held: &[u8]) -> Result<Option<(usize, Framing, bool)>, Failed> {
    let mut headers = [httparse::EMPTY_HEADER; MOST_HEADERS];
    let mut answer = httparse::Response::new(&mut headers);
    let start = match answer.parse(held) {
        Ok(httparse::Status::Complete(start)) => start,
        Ok(httparse::Status::Partial) => return Ok(None),
        Err(_) => return Err(Failed::Unreadable("it is not an HTTP answer")),
    };
    match answer.code {
        Some(200) => {}
        status => return Err(Failed::Refused(status.unwrap_or(0))),
    }
    let (mut length, mut chunked, mut closing) = (None, false, false);
    for header in answer.headers.iter() {
        let value = std::str::from_utf8(header.value).unwrap_or("").trim();
        if header.name.eq_ignore_ascii_case("connection") {
            closing |= value
                .split(',')
                .any(|token| token.trim().eq_ignore_ascii_case("close"));
        } else if header.name.eq_ignore_ascii_case("transfer-encoding") {
            if !value.eq_ignore_ascii_case("chunked") {
                return Err(Failed::Unreadable(
                    "it is sent in a coding this cannot read",
                ));
            }
            chunked = true;
        } else if header.name.eq_ignore_ascii_case("content-encoding") {
            if !value.eq_ignore_ascii_case("identity") {
                return Err(Failed::Unreadable("it is compressed"));
            }
        } else if header.name.eq_ignore_ascii_case("content-length") {
            let said = value
                .parse::<usize>()
                .map_err(|_| Failed::Unreadable("its length is not a number"))?;
            if length.is_some_and(|before| before != said) {
                return Err(Failed::Unreadable("it gives two lengths"));
            }
            length = Some(said);
        }
    }
    match (chunked, length) {
        (true, _) => Ok(Some((start, Framing::Chunked, closing))),
        (false, Some(length)) if length > MOST_ANSWER_BYTES => Err(Failed::Unreadable(TOO_LARGE)),
        (false, Some(length)) => Ok(Some((start, Framing::Length(length), closing))),
        (false, None) => Err(Failed::Unreadable("it does not say how long it is")),
    }
}

/// Read some more. Nothing more means the other end has closed.
fn more<S: Read>(stream: &mut S, held: &mut Vec<u8>) -> Result<usize, Failed> {
    let mut piece = [0u8; 8192];
    loop {
        match stream.read(&mut piece) {
            Ok(read) => {
                held.extend_from_slice(&piece[..read]);
                return Ok(read);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            // A secure connection closed without the closing message. For
            // an answer whose length is known that is an end like any
            // other: what was read is whole or it is not.
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(0),
            Err(error) => return Err(failed(READING, &error)),
        }
    }
}

/// A body sent in chunks, each with its length before it, to the chunk of
/// length nothing, and whether the empty line that ends it all had come
/// with that chunk: only then is the connection known to be at the start
/// of whatever comes next.
fn chunked<S: Read>(stream: &mut S, mut held: Vec<u8>) -> Result<(Vec<u8>, bool), Failed> {
    let mut body = Vec::new();
    loop {
        let (after, size) = loop {
            match httparse::parse_chunk_size(&held) {
                Ok(httparse::Status::Complete(found)) => break found,
                Ok(httparse::Status::Partial) if held.len() < MOST_CHUNK_LINE_BYTES => {
                    if more(stream, &mut held)? == 0 {
                        return Err(Failed::Unreadable(ENDED_EARLY));
                    }
                }
                _ => return Err(Failed::Unreadable("a chunk's length cannot be read")),
            }
        };
        if size == 0 {
            // The body is whole, and nothing more is read for it. Lines
            // after the last chunk (trailers), or an end that has not
            // come yet, only mean the connection is not used again.
            let clean = &held[after..] == b"\r\n";
            return Ok((body, clean));
        }
        let size = usize::try_from(size)
            .ok()
            .filter(|size| *size <= MOST_ANSWER_BYTES - body.len())
            .ok_or(Failed::Unreadable(TOO_LARGE))?;
        while held.len() < after + size + 2 {
            if more(stream, &mut held)? == 0 {
                return Err(Failed::Unreadable(ENDED_EARLY));
            }
        }
        if &held[after + size..after + size + 2] != b"\r\n" {
            return Err(Failed::Unreadable("a chunk does not end where it says"));
        }
        body.extend_from_slice(&held[after..after + size]);
        held.drain(..after + size + 2);
    }
}

/// How an exchange on a connection went wrong.
enum Lost {
    /// The request could not be written whole: `written` of its bytes were
    /// taken by the connection before it failed.
    Unsent {
        failed: Failed,
        written: usize,
    },
    /// Written whole, and the other end closed before a byte of an answer
    /// came. On a connection kept from an earlier asking that, like the
    /// above, is the ordinary end of its life, not a failure.
    Unanswered(Failed),
    Failed(Failed),
}

/// What became of one request as it was written (#871, the reviewer on
/// #873), told by [`Jev::ask_with_pictures`] after each try: so that a
/// request with a picture that went and was not answered is never taken
/// for one that never went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    /// Not written whole: this many of its bytes were taken by the
    /// connection before it failed, none if 0.
    Partly(usize),
    /// Written whole, and the connection ended with no answer: whether it
    /// arrived is not known.
    Uncertain,
    /// An answer to it came, whole or not: it arrived.
    Answered,
}

impl Delivery {
    fn of<T>(talked: &Result<T, Lost>) -> Self {
        match talked {
            Ok(_) | Err(Lost::Failed(_)) => Self::Answered,
            Err(Lost::Unanswered(_)) => Self::Uncertain,
            Err(Lost::Unsent { written, .. }) => Self::Partly(*written),
        }
    }

    /// Whether any of the request left this side.
    pub fn went(self) -> bool {
        self != Self::Partly(0)
    }
}

/// Send the request and read the answer's body. The second value says
/// whether the connection is at the start of whatever comes next and the
/// other end has not said it will close.
fn talk<S: Read + Write>(stream: &mut S, request: &[u8]) -> Result<(Vec<u8>, bool), Lost> {
    // As `write_all`, counting what was taken before a failure.
    let mut written = 0;
    let unsent = |error: &io::Error, written| Lost::Unsent {
        failed: failed(SENDING, error),
        written,
    };
    while written < request.len() {
        match stream.write(&request[written..]) {
            Ok(0) => return Err(unsent(&io::ErrorKind::WriteZero.into(), written)),
            Ok(taken) => written += taken,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(unsent(&error, written)),
        }
    }
    stream.flush().map_err(|error| unsent(&error, written))?;
    let mut held = Vec::with_capacity(4096);
    let (start, framing, closing) = loop {
        if let Some(found) = head(&held).map_err(Lost::Failed)? {
            break found;
        }
        if held.len() >= MOST_HEAD_BYTES {
            return Err(Lost::Failed(Failed::Unreadable(
                "its headers are longer than 16 KiB",
            )));
        }
        let read = match more(stream, &mut held) {
            Ok(read) => read,
            Err(lost) if held.is_empty() => return Err(Lost::Unanswered(lost)),
            Err(lost) => return Err(Lost::Failed(lost)),
        };
        if read == 0 {
            // Closed with nothing said at all is not an answer of any kind.
            return Err(if held.is_empty() {
                Lost::Unanswered(Failed::Unreachable {
                    during: READING,
                    kind: io::ErrorKind::UnexpectedEof,
                })
            } else {
                Lost::Failed(Failed::Unreadable("it ended before its headers were whole"))
            });
        }
    };
    held.drain(..start);
    match framing {
        Framing::Length(length) => {
            while held.len() < length {
                if more(stream, &mut held).map_err(Lost::Failed)? == 0 {
                    return Err(Lost::Failed(Failed::Unreadable(ENDED_EARLY)));
                }
            }
            // More than it said it would send: not a place to begin again.
            let exact = held.len() == length;
            held.truncate(length);
            Ok((held, exact && !closing))
        }
        Framing::Chunked => chunked(stream, held)
            .map(|(body, clean)| (body, clean && !closing))
            .map_err(Lost::Failed),
    }
}

/// A connection, plain or secure, that can be kept from one asking to the
/// next.
enum Line {
    Plain(Timed),
    Secure(Box<rustls::StreamOwned<rustls::ClientConnection, Timed>>),
}

impl Line {
    fn until(&mut self, until: Instant) {
        match self {
            Self::Plain(wire) => wire.until = until,
            Self::Secure(stream) => stream.sock.until = until,
        }
    }

    fn talk(&mut self, request: &[u8]) -> Result<(Vec<u8>, bool), Lost> {
        match self {
            Self::Plain(wire) => talk(wire, request),
            Self::Secure(stream) => talk(stream.as_mut(), request),
        }
    }
}

/// How long a connection is kept, and so how long this side takes the
/// other to keep it. Making one is a large part of a quick decision: from
/// lamp-4ace on 2026-10-14 the name, the connection and the handshake took
/// 0.20 to 0.29 s of a decision that took 0.57 to 0.79 s.
///
/// Measured against openrouter.ai from the office on 2026-10-14, one
/// connection for each figure: a connection that had answered a request
/// answered another after 20, 45, 90 and 180 s left alone; one that had
/// carried no request yet answered after 10, 12 and 14 s and was found
/// closed after 16, 18, 20 and 45 s. Both figures here are inside what
/// was seen to work, and a connection the other end has closed sooner is
/// replaced when that is found out.
#[derive(Clone, Copy)]
struct Keeping {
    /// After an answer.
    answered: Duration,
    /// After it was made, with nothing asked on it yet.
    unused: Duration,
    /// [`Decider::ready`] leaves a kept connection be if it has at least
    /// this long to go: about the length of something said.
    ready_margin: Duration,
}

const KEEPING: Keeping = Keeping {
    answered: Duration::from_secs(150),
    unused: Duration::from_secs(12),
    ready_margin: Duration::from_secs(6),
};

/// `text` as a JSON string.
fn quoted(text: &str) -> String {
    serde_json::Value::from(text).to_string()
}

/// The request's body. Written by hand so that the state goes as the text
/// it was given as and the questions in the order they were asked in: the
/// library's own map would put both in the order of the alphabet.
fn body(
    model: &str,
    state: &str,
    questions: &[(String, Question)],
    pictures: &[Picture],
) -> String {
    let mut body = format!(
        "{{\"model\":{},\"state\":{state},\"questions\":{{",
        quoted(model)
    );
    for (index, (name, question)) in questions.iter().enumerate() {
        if index > 0 {
            body.push(',');
        }
        body.push_str(&quoted(name));
        match question {
            Question::Noul { instructions } => {
                body.push_str(":{\"type\":\"noul\",\"instructions\":");
                body.push_str(&quoted(instructions));
            }
            Question::Choice {
                instructions,
                criteria,
            } => {
                body.push_str(":{\"type\":\"choice\",\"instructions\":");
                body.push_str(&quoted(instructions));
                body.push_str(",\"criteria\":{");
                for (index, (option, meaning)) in criteria.iter().enumerate() {
                    if index > 0 {
                        body.push(',');
                    }
                    body.push_str(&quoted(option));
                    body.push(':');
                    body.push_str(&quoted(meaning));
                }
                body.push('}');
            }
        }
        body.push('}');
    }
    body.push('}');
    // Without a picture the body is as it always was.
    if !pictures.is_empty() {
        use base64::Engine as _;
        body.push_str(",\"assets\":{");
        for (index, picture) in pictures.iter().enumerate() {
            if index > 0 {
                body.push(',');
            }
            body.push_str(&quoted(&picture.key));
            body.push_str(":{\"type\":\"image\",\"format\":\"jpg\",\"data_base64\":\"");
            body.push_str(&base64::engine::general_purpose::STANDARD.encode(&picture.jpeg));
            body.push_str("\"}");
        }
        body.push('}');
    }
    body.push('}');
    body
}

/// The service's answer, as far as its shape goes.
#[derive(Deserialize)]
struct Reply {
    #[serde(default)]
    model: Option<String>,
    answers: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    usage: Option<serde_json::Value>,
}

fn probability(value: Option<&serde_json::Value>) -> Result<f64, Failed> {
    value
        .and_then(serde_json::Value::as_f64)
        .filter(|probability| (0.0..=1.0).contains(probability))
        .ok_or(Failed::Unreadable(NOT_A_PROBABILITY))
}

/// One answer held to its question.
fn answer(given: &serde_json::Value, question: &Question) -> Result<Answer, Failed> {
    let kind = given.get("type").and_then(serde_json::Value::as_str);
    match question {
        Question::Noul { .. } => {
            if kind != Some("noul") {
                return Err(Failed::Unreadable(ANOTHER_TYPE));
            }
            Ok(Answer::Noul(probability(given.get("noul"))? as f32))
        }
        Question::Choice { criteria, .. } => {
            if kind != Some("choice") {
                return Err(Failed::Unreadable(ANOTHER_TYPE));
            }
            let offered = |option: &str| criteria.iter().any(|(name, _)| name == option);
            let choice = given
                .get("choice")
                .and_then(serde_json::Value::as_str)
                .filter(|choice| offered(choice))
                .ok_or(Failed::Unreadable("a choice is not one of the options"))?;
            let given = given
                .get("probabilities")
                .and_then(serde_json::Value::as_object)
                .ok_or(Failed::Unreadable("a choice has no probabilities"))?;
            let mut probabilities = BTreeMap::new();
            let mut sum = 0.0;
            for (option, value) in given {
                if !offered(option) {
                    return Err(Failed::Unreadable(
                        "a probability is for something that is not an option",
                    ));
                }
                let value = probability(Some(value))?;
                sum += value;
                probabilities.insert(option.clone(), value as f32);
            }
            if (sum - 1.0).abs() > SUM_WITHIN {
                return Err(Failed::Unreadable(
                    "a choice's probabilities do not add up to 1",
                ));
            }
            Ok(Answer::Choice {
                choice: choice.to_owned(),
                probabilities,
            })
        }
    }
}

/// What an answer's body came to.
struct Answered {
    model: Option<String>,
    answers: BTreeMap<String, Answer>,
    usage: Option<Usage>,
}

/// The answers in `body`, held strictly to what was asked: every question
/// answered, as its type, with probabilities that are probabilities. An
/// answer to a question nobody asked is passed over.
fn answers(body: &[u8], questions: &[(String, Question)]) -> Result<Answered, Failed> {
    let value: serde_json::Value =
        serde_json::from_slice(body).map_err(|_| Failed::Unreadable("it is not JSON"))?;
    let reply: Reply = serde_json::from_value(value)
        .map_err(|_| Failed::Unreadable("it does not have the shape of an answer"))?;
    // The name goes on into messages and logs, so it has to be one.
    if reply.model.as_ref().is_some_and(|model| {
        model.is_empty() || model.len() > MOST_NAME_BYTES || model.chars().any(char::is_control)
    }) {
        return Err(Failed::Unreadable("the model it names has no usable name"));
    }
    let mut held = BTreeMap::new();
    for (name, question) in questions {
        let given = reply
            .answers
            .get(name)
            .ok_or_else(|| Failed::Missing(name.clone()))?;
        held.insert(name.clone(), answer(given, question)?);
    }
    // What it cost is worth knowing and not worth failing for.
    let usage = reply
        .usage
        .and_then(|usage| serde_json::from_value(usage).ok());
    Ok(Answered {
        model: reply.model,
        answers: held,
        usage,
    })
}

/// TypeSafe's Jev, asked over HTTP/1.1, one request at a time.
///
/// The connection of an asking is kept for the next ([`Keeping`]), and
/// [`Decider::ready`] makes one ahead of a question. A kept connection the
/// other end has closed is found out when it is used and replaced then.
pub struct Jev {
    endpoint: Endpoint,
    model: String,
    /// The `Authorization` header's value. It is sent and never shown:
    /// this type has no `Debug`, and no error is made from it.
    authorization: String,
    /// How to check the service's certificate, for an `https` endpoint.
    secure: Option<Arc<rustls::ClientConfig>>,
    lookup: Lookup,
    /// A connection that can be used for the next asking, until when it
    /// is taken to be there, and what an asking on it would be sent on:
    /// one made ahead, or one kept.
    kept: Option<(Line, Instant, Connection)>,
    keeping: Keeping,
}

impl Jev {
    /// Lets go of the connection kept for the next asking, so that the next
    /// makes its own (`lamp-policy-replay --probe-connections`, #1522).
    pub fn forget(&mut self) {
        self.kept = None;
    }

    /// Keeps a connection that has answered for `keep` in place of
    /// [`KEEPING`]'s, to find out whether the other end keeps it that long
    /// (`lamp-policy-replay --probe-connections`, #1522).
    pub fn keep_answered_for(&mut self, keep: Duration) {
        self.keeping.answered = keep;
    }

    /// Jev at `endpoint`, asked for `model`, with the key that is the one
    /// line of `key_file`. A certificate is checked against the roots this
    /// program is built with.
    pub fn new(endpoint: &str, model: &str, key_file: &Path) -> io::Result<Self> {
        Self::trusting(endpoint, model, key_file, || rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        })
    }

    fn trusting(
        endpoint: &str,
        model: &str,
        key_file: &Path,
        roots: impl FnOnce() -> rustls::RootCertStore,
    ) -> io::Result<Self> {
        let invalid = |why: String| io::Error::new(io::ErrorKind::InvalidInput, why);
        let endpoint = Endpoint::parse(endpoint).map_err(invalid)?;
        if model.is_empty() || model.chars().any(char::is_control) {
            return Err(invalid("the model's name is empty or not one line".into()));
        }
        let key = std::fs::read_to_string(key_file).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!("key file {}: {error}", key_file.display()),
            )
        })?;
        let key = key.trim();
        // What is wrong with it is said without any of it.
        if key.is_empty() || !key.bytes().all(|byte| byte.is_ascii_graphic()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "key file {} does not hold one key: one line of printable characters, no spaces",
                    key_file.display()
                ),
            ));
        }
        let secure = if endpoint.secure {
            Some(Arc::new(secure(roots())?))
        } else {
            None
        };
        Ok(Self {
            lookup: Lookup {
                host: endpoint.host.clone(),
                port: endpoint.port,
                resolve: system_lookup,
                known: None,
                pending: None,
            },
            authorization: format!("Bearer {key}"),
            model: model.to_owned(),
            endpoint,
            secure,
            kept: None,
            keeping: KEEPING,
        })
    }

    /// One request and its answer's body, all of it before `until`, on
    /// the connection kept from the last asking if there is one. A kept
    /// connection that turns out to have ended before a byte of an answer
    /// came is replaced once: the request has no effect to repeat.
    /// `told` hears what became of each request written ([`Delivery`]).
    fn exchange(
        &mut self,
        make: &dyn Fn() -> Result<Vec<u8>, Failed>,
        told: &dyn Fn(Delivery),
        until: Instant,
    ) -> Result<(Vec<u8>, Connection), Failed> {
        let mut on = Connection::New;
        if let Some((mut line, there_until, kind)) = self.kept.take()
            && Instant::now() < there_until
        {
            line.until(until);
            // Made at the last moment before anything is written.
            let request = make()?;
            on = Connection::Replaced;
            let talked = line.talk(&request);
            told(Delivery::of(&talked));
            match talked {
                Ok((body, again)) => {
                    if again {
                        let there_until = Instant::now() + self.keeping.answered;
                        self.kept = Some((line, there_until, Connection::Kept));
                    }
                    return Ok((body, kind));
                }
                // It had ended, which is how a kept connection's life
                // ordinarily ends. With no time left there is nothing to
                // try again with, and what it failed of is the failure.
                Err(Lost::Unanswered(failed) | Lost::Unsent { failed, .. })
                    if Instant::now() >= until =>
                {
                    return Err(failed);
                }
                Err(Lost::Unanswered(_) | Lost::Unsent { .. }) => {}
                Err(Lost::Failed(failed)) => return Err(failed),
            }
        }
        let mut line = self.open(until)?;
        // After the looking up and the connecting, however long they took.
        let request = make()?;
        let talked = line.talk(&request);
        told(Delivery::of(&talked));
        match talked {
            Ok((body, again)) => {
                if again {
                    let there_until = Instant::now() + self.keeping.answered;
                    self.kept = Some((line, there_until, Connection::Kept));
                }
                Ok((body, on))
            }
            Err(Lost::Unanswered(failed) | Lost::Unsent { failed, .. } | Lost::Failed(failed)) => {
                Err(failed)
            }
        }
    }

    /// A new connection, made and if need be secured before `until`.
    fn open(&mut self, until: Instant) -> Result<Line, Failed> {
        let mut addresses = self.lookup.addresses(until)?;
        if self.secure.is_none() {
            // Plain http is for this machine only, whatever the name
            // `localhost` was looked up to.
            addresses.retain(|address| address.ip().is_loopback());
        }
        let socket = connect(&addresses, until).inspect_err(|_| self.lookup.forget())?;
        // One small request: nothing is gained by waiting to fill a packet.
        let _ = socket.set_nodelay(true);
        let mut wire = Timed { socket, until };
        let Some(config) = &self.secure else {
            return Ok(Line::Plain(wire));
        };
        // A host that cannot be named to the other end cannot be checked.
        let unnamed = Failed::Unreachable {
            during: SECURING,
            kind: io::ErrorKind::InvalidInput,
        };
        let Ok(name) = rustls::pki_types::ServerName::try_from(self.endpoint.host.clone()) else {
            return Err(unnamed);
        };
        let Ok(mut session) = rustls::ClientConnection::new(Arc::clone(config), name) else {
            return Err(unnamed);
        };
        // The handshake by itself first, so that a certificate that is not
        // trusted is told apart from a request that could not be sent.
        while session.is_handshaking() {
            session
                .complete_io(&mut wire)
                .map_err(|error| failed(SECURING, &error))?;
        }
        Ok(Line::Secure(Box::new(rustls::StreamOwned::new(
            session, wire,
        ))))
    }
}

/// TLS 1.2 and 1.3 with the `ring` provider, speaking HTTP/1.1 only.
fn secure(roots: rustls::RootCertStore) -> io::Result<rustls::ClientConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|error| io::Error::other(format!("tls: {error}")))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(config)
}

impl Jev {
    /// [`Decider::ask`] with pictures, which the state or the questions
    /// name by their keys (#871). Sent with [`IMAGES_HEADER`].
    ///
    /// `take` gives the pictures, and it is called only at the moment the
    /// request is to be written: after any looking up and connecting, and
    /// again if it has to go on another connection. So what is sent is what
    /// `take` gives then, never something it gave before a slow connection
    /// (the reviewer on #873). If it fails, nothing is written and the asking
    /// has failed as it says ([`Failed::Withdrawn`] for a picture no longer
    /// permitted). `told` hears, after each request `take` gave pictures
    /// for, what became of it: so a request that went, and was not
    /// answered, before one withdrawn is known to have gone.
    pub fn ask_with_pictures(
        &mut self,
        state: &str,
        questions: &[(String, Question)],
        deadline: Duration,
        take: &dyn Fn() -> Result<Vec<Picture>, Failed>,
        told: &dyn Fn(Delivery),
    ) -> Result<Asked, Failed> {
        let started = Instant::now();
        let request = self.request_maker();
        let model = self.model.clone();
        let make = || -> Result<Vec<u8>, Failed> {
            let pictures = take()?;
            Ok(request(
                &body(&model, state, questions, &pictures),
                !pictures.is_empty(),
            ))
        };
        self.answered(&make, told, questions, started, deadline)
    }

    /// What puts a request together from its body, with the images header
    /// or not: owned, so that it can be called while the connection is in
    /// hand.
    fn request_maker(&self) -> impl Fn(&str, bool) -> Vec<u8> + use<> {
        let (path, authority, authorization) = (
            self.endpoint.path.clone(),
            self.endpoint.authority(),
            self.authorization.clone(),
        );
        move |body: &str, images: bool| {
            let images = if images {
                format!("{IMAGES_HEADER}\r\n")
            } else {
                String::new()
            };
            // `identity`: this reads no compressed answer.
            let mut request = format!(
                "POST {path} HTTP/1.1\r\nHost: {authority}\r\nAuthorization: {authorization}\r\n\
                 Content-Type: application/json\r\nAccept: application/json\r\n\
                 Accept-Encoding: identity\r\nUser-Agent: lamp-policy\r\n{images}\
                 Content-Length: {}\r\nConnection: keep-alive\r\n\r\n",
                body.len()
            )
            .into_bytes();
            request.extend_from_slice(body.as_bytes());
            request
        }
    }

    /// The request `make` gives, sent, and its answer read and checked.
    fn answered(
        &mut self,
        make: &dyn Fn() -> Result<Vec<u8>, Failed>,
        told: &dyn Fn(Delivery),
        questions: &[(String, Question)],
        started: Instant,
        deadline: Duration,
    ) -> Result<Asked, Failed> {
        let (answer, connection) = self.exchange(make, told, started + deadline)?;
        let answered = answers(&answer, questions)?;
        Ok(Asked {
            answers: answered.answers,
            // A service that does not say which model answered is taken to
            // have used the one it was asked for.
            model: answered.model.unwrap_or_else(|| self.model.clone()),
            took: started.elapsed(),
            usage: answered.usage,
            connection: Some(connection),
        })
    }
}

impl Decider for Jev {
    fn ask(
        &mut self,
        state: &str,
        questions: &[(String, Question)],
        deadline: Duration,
    ) -> Result<Asked, Failed> {
        let started = Instant::now();
        let request = self.request_maker()(&body(&self.model, state, questions, &[]), false);
        self.answered(
            &|| Ok(request.clone()),
            &|_| {},
            questions,
            started,
            deadline,
        )
    }

    fn ready(&mut self, within: Duration) {
        let needed = Instant::now() + self.keeping.ready_margin;
        if self
            .kept
            .as_ref()
            .is_some_and(|(_, there_until, _)| *there_until >= needed)
        {
            return;
        }
        // The old one is let go first: the other end is told before it is
        // asked for another. A failure here is not told: the asking will
        // meet it and say so.
        self.kept = None;
        self.kept = self.open(Instant::now() + within).ok().map(|line| {
            (
                line,
                Instant::now() + self.keeping.unused,
                Connection::Ahead,
            )
        });
    }

    fn name(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests;
