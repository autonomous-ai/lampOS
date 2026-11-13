//! A local speech-to-text service for the real voice's SECOND route on a
//! desktop (sim-3-P4 step 2; the first builder on #6811, 13 Nov 18:44). It
//! speaks the part of Deepgram's streaming protocol the route uses
//! (`crates/voice/src/second_provider/net/stt.rs`): a WebSocket on
//! `/ws/audio/transcriptions` (the route's path when `stt_base_url` is set)
//! or `/v1/listen`, binary linear16 16 kHz mono audio in, the text messages
//! `Finalize`, `CloseStream` and `KeepAlive`, and `Results` and
//! `SpeechStarted` out.
//!
//! The route's coordinator ends a line itself and then sends `Finalize`,
//! waiting 1 s for the words. So the audio of the line is held, and at
//! `Finalize` it goes to the developer's own `whisper-server` (whisper.cpp,
//! a program of theirs, as `say` is) and the words come back as one final
//! `Results` with `from_finalize`. No interim results in this first cut.
//! Measured on macOS 26 (arm64) with whisper.cpp 1.9.2 kept warm: 44 ms for
//! a 1.3 s line and 79 ms for a 6 s one with `ggml-base.en` (#6811, 13 Nov).
//!
//! Loopback only, and only for a caller holding this run's token (the route
//! signs the socket `Authorization: Token <key>`; `Bearer` is taken too).
use crate::speech::same;
use std::{
    io::{self, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tungstenite::{
    Message,
    handshake::server::{Callback, ErrorResponse, Request, Response},
    http,
};

/// The route's audio: 16 kHz mono S16LE.
pub const SAMPLE_RATE: u32 = 16_000;
/// The longest line held, in seconds of audio; older audio is let go.
const MOST_LINE_SECONDS: usize = 60;
/// Lines heard at once; more are refused at once.
const MOST_AT_ONCE: usize = 4;
/// The energy gate for `SpeechStarted`: a 20 ms frame over this RMS, after
/// at least `QUIET_FRAMES` frames under it.
const SPEECH_RMS: f64 = 600.0;
const QUIET_FRAMES: usize = 15;
const FRAME_SAMPLES: usize = 320;
/// How long whisper-server may take for one line.
const WHISPER_WAIT: Duration = Duration::from_secs(5);

/// What turns a line's audio into words.
pub trait Recognizer: Send + Sync {
    /// The words in `samples` (16 kHz mono), or why not.
    fn words(&self, samples: &[i16]) -> io::Result<String>;
}

/// The address to listen on, refused unless it is loopback.
pub fn bind(address: SocketAddr) -> io::Result<TcpListener> {
    if !address.ip().is_loopback() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("the listening service listens on loopback only, not {address}"),
        ));
    }
    TcpListener::bind(address)
}

/// Serves `listener` until it fails: each socket on a thread of its own, at
/// most [`MOST_AT_ONCE`] at a time.
pub fn serve(
    listener: TcpListener,
    token: String,
    recognizer: Arc<dyn Recognizer>,
) -> io::Result<()> {
    let token: Arc<str> = token.into();
    let busy = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        if busy.fetch_add(1, Ordering::SeqCst) >= MOST_AT_ONCE {
            busy.fetch_sub(1, Ordering::SeqCst);
            refuse_busy(stream);
            continue;
        }
        let (token, recognizer, busy) = (token.clone(), recognizer.clone(), busy.clone());
        std::thread::spawn(move || {
            if let Err(error) = one(stream, &token, recognizer.as_ref()) {
                eprintln!("lamp-sim listen: {error}");
            }
            busy.fetch_sub(1, Ordering::SeqCst);
        });
    }
    Ok(())
}

fn refuse_busy(mut stream: TcpStream) {
    let _ = stream.write_all(
        b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
}

/// Whether the opening request may have a socket: the path, the audio it
/// says it sends, and the token.
fn admit(request: &Request, token: &str) -> Result<(), (u16, String)> {
    let path = request.uri().path();
    if path != "/ws/audio/transcriptions" && path != "/v1/listen" {
        return Err((404, format!("no socket at {path}")));
    }
    let query = request.uri().query().unwrap_or("");
    let param = |name: &str| {
        query
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value)
    };
    if param("encoding").is_some_and(|e| e != "linear16")
        || param("sample_rate").is_some_and(|r| r != "16000")
        || param("channels").is_some_and(|c| c != "1")
    {
        return Err((
            400,
            "only linear16, 16000 Hz, 1 channel is heard here".to_owned(),
        ));
    }
    let given = request
        .headers()
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            value
                .strip_prefix("Token ")
                .or_else(|| value.strip_prefix("Bearer "))
        })
        .unwrap_or("")
        .trim();
    if !same(given, token) {
        return Err((401, "this run's token is needed".to_owned()));
    }
    Ok(())
}

/// The opening request's check, as tungstenite asks for it.
struct Admit<'a>(&'a str);

impl Callback for Admit<'_> {
    fn on_request(self, request: &Request, response: Response) -> Result<Response, ErrorResponse> {
        match admit(request, self.0) {
            Ok(()) => Ok(response),
            Err((status, why)) => Err(http::Response::builder()
                .status(status)
                .body(Some(why))
                .unwrap_or_else(|_| http::Response::new(None))),
        }
    }
}

/// One socket: its audio held, a line's words at each `Finalize`.
fn one(stream: TcpStream, token: &str, recognizer: &dyn Recognizer) -> io::Result<()> {
    let mut socket = tungstenite::accept_hdr(stream, Admit(token))
        .map_err(|error| io::Error::other(format!("opening: {error}")))?;
    let mut line = Line::default();
    loop {
        let message = match socket.read() {
            Ok(message) => message,
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                return Ok(());
            }
            Err(error) => return Err(io::Error::other(format!("reading: {error}"))),
        };
        match message {
            Message::Binary(bytes) => {
                if let Some(at) = line.hear(&bytes) {
                    let started = serde_json::json!({ "type": "SpeechStarted", "timestamp": at });
                    send(&mut socket, started)?;
                }
            }
            Message::Text(text) => {
                let kind = serde_json::from_str::<serde_json::Value>(&text)
                    .ok()
                    .and_then(|value| value["type"].as_str().map(str::to_owned))
                    .unwrap_or_default();
                match kind.as_str() {
                    "Finalize" => {
                        let results = line.finish(recognizer);
                        send(&mut socket, results)?;
                    }
                    "CloseStream" => {
                        if line.held() {
                            let results = line.finish(recognizer);
                            send(&mut socket, results)?;
                        }
                        let _ = socket.close(None);
                        let _ = socket.flush();
                        return Ok(());
                    }
                    // `KeepAlive` and anything else: nothing to answer.
                    _ => {}
                }
            }
            Message::Close(_) => return Ok(()),
            _ => {}
        }
    }
}

fn send(
    socket: &mut tungstenite::WebSocket<TcpStream>,
    value: serde_json::Value,
) -> io::Result<()> {
    socket
        .send(Message::text(value.to_string()))
        .map_err(|error| io::Error::other(format!("sending: {error}")))
}

/// The audio of the line being heard, and where it sits in the socket's audio.
#[derive(Default)]
struct Line {
    samples: Vec<i16>,
    /// Seconds of audio before this line, since the socket opened.
    start: f64,
    /// A byte left over from an odd-length message.
    odd: Option<u8>,
    /// Frames under the speech level in a row, and the frame being filled.
    quiet: usize,
    frame: Vec<i16>,
    /// Samples heard since the socket opened.
    heard: usize,
}

impl Line {
    /// Takes in audio; the time speech began, if a frame of it just did.
    fn hear(&mut self, bytes: &[u8]) -> Option<f64> {
        let mut started = None;
        let mut bytes = bytes;
        let mut pair = [0u8; 2];
        if let Some(low) = self.odd.take() {
            if let Some((&high, rest)) = bytes.split_first() {
                pair = [low, high];
                started = self.sample(i16::from_le_bytes(pair)).or(started);
                bytes = rest;
            } else {
                self.odd = Some(low);
            }
        }
        let mut chunks = bytes.chunks_exact(2);
        for chunk in &mut chunks {
            pair.copy_from_slice(chunk);
            started = started.or(self.sample(i16::from_le_bytes(pair)));
        }
        if let [last] = chunks.remainder() {
            self.odd = Some(*last);
        }
        let most = MOST_LINE_SECONDS * SAMPLE_RATE as usize;
        if self.samples.len() > most {
            let extra = self.samples.len() - most;
            self.samples.drain(..extra);
            self.start += extra as f64 / f64::from(SAMPLE_RATE);
        }
        started
    }

    fn sample(&mut self, sample: i16) -> Option<f64> {
        self.samples.push(sample);
        self.heard += 1;
        self.frame.push(sample);
        if self.frame.len() < FRAME_SAMPLES {
            return None;
        }
        let energy = self
            .frame
            .iter()
            .map(|s| f64::from(*s).powi(2))
            .sum::<f64>();
        let rms = (energy / self.frame.len() as f64).sqrt();
        self.frame.clear();
        if rms < SPEECH_RMS {
            self.quiet += 1;
            return None;
        }
        let was_quiet = self.quiet >= QUIET_FRAMES;
        self.quiet = 0;
        was_quiet.then(|| (self.heard - FRAME_SAMPLES) as f64 / f64::from(SAMPLE_RATE))
    }

    fn held(&self) -> bool {
        !self.samples.is_empty()
    }

    /// The line's words as the route's final `Results`, and a new line.
    fn finish(&mut self, recognizer: &dyn Recognizer) -> serde_json::Value {
        let asked = Instant::now();
        let duration = self.samples.len() as f64 / f64::from(SAMPLE_RATE);
        let words = if self.samples.is_empty() {
            String::new()
        } else {
            recognizer.words(&self.samples).unwrap_or_else(|error| {
                eprintln!("lamp-sim listen: no words for a {duration:.1} s line: {error}");
                String::new()
            })
        };
        eprintln!(
            "lamp-sim listen: a {duration:.1} s line, words after {} ms",
            asked.elapsed().as_millis()
        );
        let results = serde_json::json!({
            "type": "Results",
            "is_final": true,
            "speech_final": true,
            "from_finalize": true,
            "start": self.start,
            "duration": duration,
            "channel": { "alternatives": [ { "transcript": words } ] },
        });
        self.start += duration;
        self.samples.clear();
        results
    }
}

/// whisper.cpp's `whisper-server` at a loopback address: the line as a
/// 16 kHz WAV to `/inference`, the words from its JSON `text`.
pub struct Whisper {
    pub address: SocketAddr,
}

impl Whisper {
    /// `http://127.0.0.1:PORT` (a loopback address only: the audio of a room
    /// goes nowhere else).
    pub fn at(url: &str) -> io::Result<Self> {
        let address: SocketAddr = url
            .strip_prefix("http://")
            .map(|rest| rest.trim_end_matches('/'))
            .and_then(|rest| rest.parse().ok())
            .ok_or_else(|| io::Error::other(format!("--whisper {url}: http://127.0.0.1:PORT")))?;
        if !address.ip().is_loopback() {
            return Err(io::Error::other(format!(
                "--whisper {url}: a loopback address only"
            )));
        }
        Ok(Self { address })
    }
}

impl Recognizer for Whisper {
    fn words(&self, samples: &[i16]) -> io::Result<String> {
        let boundary = "lampsim0boundary";
        let mut body = Vec::with_capacity(samples.len() * 2 + 512);
        write!(
            body,
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"line.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
        )?;
        body.extend_from_slice(&wav(samples));
        for (name, value) in [("response_format", "json"), ("temperature", "0")] {
            write!(
                body,
                "\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}"
            )?;
        }
        write!(body, "\r\n--{boundary}--\r\n")?;
        let mut stream = TcpStream::connect_timeout(&self.address, WHISPER_WAIT)?;
        stream.set_read_timeout(Some(WHISPER_WAIT))?;
        stream.set_write_timeout(Some(WHISPER_WAIT))?;
        write!(
            stream,
            "POST /inference HTTP/1.1\r\nHost: {}\r\nContent-Type: multipart/form-data; boundary={boundary}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            self.address,
            body.len()
        )?;
        stream.write_all(&body)?;
        let mut answer = Vec::new();
        stream.take(1 << 20).read_to_end(&mut answer)?;
        let split = answer
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .ok_or_else(|| io::Error::other("whisper-server: no answer head"))?;
        let head = String::from_utf8_lossy(&answer[..split]);
        if !head.starts_with("HTTP/1.1 200") && !head.starts_with("HTTP/1.0 200") {
            return Err(io::Error::other(format!(
                "whisper-server: {}",
                head.lines().next().unwrap_or("")
            )));
        }
        let json: serde_json::Value = serde_json::from_slice(&answer[split + 4..])
            .map_err(|error| io::Error::other(format!("whisper-server's answer: {error}")))?;
        Ok(json["text"]
            .as_str()
            .unwrap_or("")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" "))
    }
}

/// 16 kHz mono S16LE samples as a WAV file.
fn wav(samples: &[i16]) -> Vec<u8> {
    let data = samples.len() as u32 * 2;
    let mut out = Vec::with_capacity(44 + data as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tungstenite::client::IntoClientRequest;

    /// Gives fixed words and keeps what it was given.
    struct Fixed(&'static str, Mutex<Vec<usize>>);
    impl Recognizer for Fixed {
        fn words(&self, samples: &[i16]) -> io::Result<String> {
            self.1.lock().unwrap().push(samples.len());
            Ok(self.0.to_owned())
        }
    }

    fn start(recognizer: Arc<dyn Recognizer>) -> SocketAddr {
        let listener = bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || serve(listener, "t0ken".to_owned(), recognizer));
        address
    }

    type Client = tungstenite::WebSocket<TcpStream>;

    fn open(address: SocketAddr, path: &str, auth: Option<&str>) -> Result<Client, u16> {
        let mut request = format!("ws://{address}{path}")
            .into_client_request()
            .unwrap();
        if let Some(auth) = auth {
            request
                .headers_mut()
                .insert("authorization", auth.parse().unwrap());
        }
        let stream = TcpStream::connect(address).unwrap();
        match tungstenite::client(request, stream) {
            Ok((socket, _)) => Ok(socket),
            Err(tungstenite::HandshakeError::Failure(tungstenite::Error::Http(response))) => {
                Err(response.status().as_u16())
            }
            Err(error) => panic!("{error}"),
        }
    }

    fn next(socket: &mut Client) -> serde_json::Value {
        loop {
            if let Message::Text(text) = socket.read().unwrap() {
                return serde_json::from_str(&text).unwrap();
            }
        }
    }

    /// `seconds` of a tone at `level` (an RMS of about level / 1.4), as bytes.
    fn tone(seconds: f64, level: i16) -> Vec<u8> {
        (0..(seconds * f64::from(SAMPLE_RATE)) as usize)
            .flat_map(|i| {
                let v = (f64::from(level) * (i as f64 * 0.2).sin()) as i16;
                v.to_le_bytes()
            })
            .collect()
    }

    const PATH: &str = "/ws/audio/transcriptions?model=nova-3-general&encoding=linear16&sample_rate=16000&channels=1&interim_results=true&endpointing=300&vad_events=true";

    #[test]
    fn a_finalize_answers_the_held_line_as_one_final_result_from_finalize() {
        let recognizer = Arc::new(Fixed("turn on the light", Mutex::default()));
        let address = start(recognizer.clone());
        let mut socket = open(address, PATH, Some("Token t0ken")).unwrap();
        // Half a second of quiet, then a second of speech, sent in odd-sized pieces.
        let audio = [tone(0.5, 0), tone(1.0, 8_000)].concat();
        for piece in audio.chunks(777) {
            socket.send(Message::binary(piece.to_vec())).unwrap();
        }
        let started = next(&mut socket);
        assert_eq!(started["type"], "SpeechStarted");
        let at = started["timestamp"].as_f64().unwrap();
        assert!((0.45..=0.55).contains(&at), "{at}");
        socket
            .send(Message::text(r#"{"type":"KeepAlive"}"#))
            .unwrap();
        socket
            .send(Message::text(r#"{"type":"Finalize"}"#))
            .unwrap();
        let results = next(&mut socket);
        assert_eq!(results["type"], "Results");
        assert_eq!(results["is_final"], true);
        assert_eq!(results["speech_final"], true);
        assert_eq!(results["from_finalize"], true);
        assert_eq!(
            results["channel"]["alternatives"][0]["transcript"],
            "turn on the light"
        );
        assert_eq!(results["start"], 0.0);
        assert_eq!(results["duration"], 1.5);
        assert_eq!(*recognizer.1.lock().unwrap(), [24_000]);
        // The next line starts where that one ended; CloseStream answers what is held, then closes.
        socket.send(Message::binary(tone(0.25, 8_000))).unwrap();
        socket
            .send(Message::text(r#"{"type":"CloseStream"}"#))
            .unwrap();
        let last = next(&mut socket);
        assert_eq!(last["start"], 1.5);
        assert_eq!(last["duration"], 0.25);
        assert!(matches!(socket.read(), Ok(Message::Close(_)) | Err(_)));
    }

    #[test]
    fn only_the_runs_token_on_the_routes_paths_with_its_audio_gets_a_socket() {
        let recognizer = Arc::new(Fixed("x", Mutex::default()));
        let address = start(recognizer.clone());
        assert!(
            open(
                address,
                "/v1/listen?encoding=linear16",
                Some("Bearer t0ken")
            )
            .is_ok()
        );
        for (path, auth, status) in [
            (PATH, None, 401),
            (PATH, Some("Token wrong"), 401),
            (PATH, Some("t0ken"), 401),
            ("/v1/speak", Some("Token t0ken"), 404),
            ("/v1/listen?encoding=opus", Some("Token t0ken"), 400),
            ("/v1/listen?sample_rate=48000", Some("Token t0ken"), 400),
        ] {
            assert_eq!(
                open(address, path, auth).err(),
                Some(status),
                "{path} {auth:?}"
            );
        }
        assert!(recognizer.1.lock().unwrap().is_empty());
    }

    #[test]
    fn a_failed_recognizer_answers_an_empty_final_not_nothing() {
        struct Broken;
        impl Recognizer for Broken {
            fn words(&self, _: &[i16]) -> io::Result<String> {
                Err(io::Error::other("connection refused"))
            }
        }
        let address = start(Arc::new(Broken));
        let mut socket = open(address, PATH, Some("Token t0ken")).unwrap();
        socket.send(Message::binary(tone(0.2, 8_000))).unwrap();
        socket
            .send(Message::text(r#"{"type":"Finalize"}"#))
            .unwrap();
        let results = next(&mut socket);
        assert_eq!(results["from_finalize"], true);
        assert_eq!(results["channel"]["alternatives"][0]["transcript"], "");
    }

    #[test]
    fn whisper_is_asked_with_a_wav_and_its_text_comes_back_as_words() {
        let fake = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = fake.local_addr().unwrap();
        let seen = std::thread::spawn(move || {
            let (mut stream, _) = fake.accept().unwrap();
            let mut got = Vec::new();
            let mut buffer = [0u8; 65_536];
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            while let Ok(n) = stream.read(&mut buffer) {
                if n == 0 {
                    break;
                }
                got.extend_from_slice(&buffer[..n]);
                if got.ends_with(b"--\r\n") {
                    break;
                }
            }
            let body = r#"{"text":" Turn on\n the light. "}"#;
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            got
        });
        let words = Whisper::at(&format!("http://{address}"))
            .unwrap()
            .words(&[100; 1_600])
            .unwrap();
        assert_eq!(words, "Turn on the light.");
        let request = seen.join().unwrap();
        let text = String::from_utf8_lossy(&request);
        assert!(text.starts_with("POST /inference HTTP/1.1"), "{text}");
        assert!(text.contains("name=\"response_format\"\r\n\r\njson"));
        let riff = request.windows(4).position(|w| w == b"RIFF").unwrap();
        assert_eq!(&request[riff + 8..riff + 16], b"WAVEfmt ");
        assert!(Whisper::at("http://192.168.1.5:8080").is_err());
        assert!(Whisper::at("https://127.0.0.1:8080").is_err());
    }
}
