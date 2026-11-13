//! A local text-to-speech service for the real voice's SECOND route on a
//! desktop (sim-3-P4 step 1; the first builder on #6811, 13 Nov 18:44). It
//! answers the OpenAI-compatible `POST {base}/audio/speech` the route already
//! calls (`crates/voice/src/second_provider/net/tts.rs`): a JSON body with
//! `input` and `response_format` `pcm`, answered with 24 kHz mono S16LE PCM,
//! streamed in chunks as the engine makes it. `model` and `voice` are read
//! past: the host's own voice speaks.
//!
//! Loopback only, and only for a caller holding this run's token (the route
//! sends its TTS credential as `Authorization: Bearer`), so no other machine
//! and no other local program without the token can make the lamp speak.
//!
//! The engine here is the host's `say` (macOS), one process a line: about
//! 0.7 s before the first bytes of a short line, measured on macOS 26 (#6811,
//! 13 Nov 19:4x); a resident synthesizer is the next step. The real voice
//! reaches this only once it may use a loopback service (step 3, ten's).
use std::{
    io::{self, BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

/// The route's sample rate for `pcm` (OpenAI's: 24 kHz mono S16LE).
pub const SAMPLE_RATE: u32 = 24_000;
/// The longest request head read, and the longest body.
const MOST_HEAD_BYTES: usize = 16 * 1024;
const MOST_BODY_BYTES: usize = 64 * 1024;
/// The longest line spoken, in characters (the route sends a sentence).
const MOST_INPUT_CHARS: usize = 4_096;
/// Requests served at once; more are answered 503 at once.
const MOST_AT_ONCE: usize = 4;
/// How long a caller may take to send its whole request (not each read: a
/// caller dripping a byte at a time is cut off too).
const READ_WAIT: Duration = Duration::from_secs(5);
/// How long one write of the answer may wait for the caller to read.
const WRITE_WAIT: Duration = Duration::from_secs(10);

/// What speaks a line: 24 kHz mono S16LE bytes, handed to `out` piece by
/// piece as they are made.
pub trait Engine: Send + Sync {
    fn speak(&self, text: &str, out: &mut dyn FnMut(&[u8]) -> io::Result<()>) -> io::Result<()>;
}

/// The address to listen on, refused unless it is loopback.
pub fn bind(address: SocketAddr) -> io::Result<TcpListener> {
    if !address.ip().is_loopback() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("the speech service listens on loopback only, not {address}"),
        ));
    }
    TcpListener::bind(address)
}

/// Serves `listener` until it fails: each connection on a thread of its own,
/// at most [`MOST_AT_ONCE`] at a time.
pub fn serve(listener: TcpListener, token: String, engine: Arc<dyn Engine>) -> io::Result<()> {
    let token: Arc<str> = token.into();
    let busy = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        if busy.fetch_add(1, Ordering::SeqCst) >= MOST_AT_ONCE {
            busy.fetch_sub(1, Ordering::SeqCst);
            let _ = answer(&mut stream, 503, "busy: too many lines at once");
            continue;
        }
        let (token, engine, busy) = (token.clone(), engine.clone(), busy.clone());
        std::thread::spawn(move || {
            if let Err(error) = one(&mut stream, &token, engine.as_ref()) {
                eprintln!("lamp-sim speech: {error}");
            }
            busy.fetch_sub(1, Ordering::SeqCst);
        });
    }
    Ok(())
}

/// One request: checked, then spoken as a chunked answer.
fn one(stream: &mut TcpStream, token: &str, engine: &dyn Engine) -> io::Result<()> {
    stream.set_write_timeout(Some(WRITE_WAIT))?;
    let asked = Instant::now();
    let request = match read_request(stream, asked + READ_WAIT) {
        Ok(request) => request,
        Err(why) => return answer(stream, 400, &why),
    };
    if request.path != "/audio/speech" && request.path != "/v1/audio/speech" {
        return answer(stream, 404, "not found");
    }
    if request.method != "POST" {
        return answer(stream, 405, "only POST");
    }
    if !same(request.bearer.as_deref().unwrap_or(""), token) {
        return answer(stream, 401, "this run's token is needed");
    }
    let text = match line_of(&request.body) {
        Ok(text) => text,
        Err(why) => return answer(stream, 400, &why),
    };
    // The 200 goes with the first sound, so an engine that fails before any
    // (none on this system, say) is a 503 with its reason, not a broken stream.
    let mut first: Option<Duration> = None;
    let mut bytes = 0usize;
    let spoken = engine.speak(&text, &mut |piece| {
        if piece.is_empty() {
            return Ok(());
        }
        if first.is_none() {
            first = Some(asked.elapsed());
            stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: audio/pcm\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
            )?;
        }
        bytes += piece.len();
        write!(stream, "{:x}\r\n", piece.len())?;
        stream.write_all(piece)?;
        stream.write_all(b"\r\n")
    });
    match (spoken, first) {
        (Err(error), None) => {
            return answer(
                stream,
                503,
                &format!("no speech from this host's engine: {error}"),
            );
        }
        // A failure after some sound ends the answer short (no last chunk):
        // the caller sees a broken stream, not a whole line of silence.
        (Err(error), Some(_)) => return Err(error),
        (Ok(()), None) => return answer(stream, 503, "the engine made no sound for this line"),
        (Ok(()), Some(_)) => {}
    }
    stream.write_all(b"0\r\n\r\n")?;
    stream.flush()?;
    eprintln!(
        "lamp-sim speech: {} characters, first bytes after {} ms, {} ms of sound, done after {} ms",
        text.chars().count(),
        first.map_or(0, |at| at.as_millis()),
        bytes as u64 * 1000 / (2 * u64::from(SAMPLE_RATE)),
        asked.elapsed().as_millis()
    );
    Ok(())
}

struct Request {
    method: String,
    path: String,
    bearer: Option<String>,
    body: Vec<u8>,
}

/// A stream read only until `until`: each read waits at most the time left.
struct Until<'a> {
    stream: &'a TcpStream,
    until: Instant,
}

impl Read for Until<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let left = self.until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "the request took too long",
            ));
        }
        self.stream.set_read_timeout(Some(left))?;
        let mut stream = self.stream;
        stream.read(buffer)
    }
}

/// A request head and its body, within the bounds above and by `until`; why
/// not, in words.
fn read_request(stream: &TcpStream, until: Instant) -> Result<Request, String> {
    let reader = Until { stream, until };
    let mut reader = BufReader::new(reader.take((MOST_HEAD_BYTES + MOST_BODY_BYTES) as u64));
    let mut line = String::new();
    let mut head_bytes = 0usize;
    let mut read_line = |reader: &mut BufReader<_>, line: &mut String| -> Result<(), String> {
        line.clear();
        let n = reader.read_line(line).map_err(|e| format!("read: {e}"))?;
        head_bytes += n;
        if n == 0 || head_bytes > MOST_HEAD_BYTES {
            return Err("a request head that ends early or is too long".to_owned());
        }
        Ok(())
    };
    read_line(&mut reader, &mut line)?;
    let mut parts = line.split_whitespace();
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else {
        return Err("no request line".to_owned());
    };
    let (method, path) = (
        method.to_owned(),
        target.split('?').next().unwrap_or("").to_owned(),
    );
    let (mut length, mut bearer) = (0usize, None);
    loop {
        read_line(&mut reader, &mut line)?;
        let header = line.trim_end_matches(['\r', '\n']);
        if header.is_empty() {
            break;
        }
        let Some((name, value)) = header.split_once(':') else {
            return Err("a header with no colon".to_owned());
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            length = value
                .parse()
                .map_err(|_| "a bad Content-Length".to_owned())?;
        } else if name.eq_ignore_ascii_case("authorization") {
            bearer = value
                .strip_prefix("Bearer ")
                .map(|token| token.trim().to_owned());
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err("a chunked request body is not read here".to_owned());
        }
    }
    if length > MOST_BODY_BYTES {
        return Err(format!("a body over {MOST_BODY_BYTES} bytes"));
    }
    let mut body = vec![0; length];
    reader
        .read_exact(&mut body)
        .map_err(|e| format!("body: {e}"))?;
    Ok(Request {
        method,
        path,
        bearer,
        body,
    })
}

/// The line to speak, from the route's JSON; why not, in words.
fn line_of(body: &[u8]) -> Result<String, String> {
    let json: serde_json::Value =
        serde_json::from_slice(body).map_err(|e| format!("not JSON: {e}"))?;
    if json["response_format"].as_str() != Some("pcm") {
        return Err("only response_format \"pcm\" (24 kHz mono S16LE) is made here".to_owned());
    }
    let text = json["input"].as_str().unwrap_or("").trim();
    if text.is_empty() {
        return Err("no input to speak".to_owned());
    }
    if text.chars().count() > MOST_INPUT_CHARS {
        return Err(format!("an input over {MOST_INPUT_CHARS} characters"));
    }
    Ok(text.to_owned())
}

/// A short answer that is not speech.
fn answer(stream: &mut TcpStream, status: u16, why: &str) -> io::Result<()> {
    let reason = match status {
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Service Unavailable",
    };
    let body = serde_json::json!({ "error": { "message": why } }).to_string();
    // One write: `write!` on a bare stream sends each piece as its own write,
    // and a caller's first read could then hold only "HTTP/1.1 " (nine's P3).
    let text = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(text.as_bytes())?;
    stream.flush()
}

/// Whether `given` is `token`, in a time that does not depend on where they
/// first differ.
fn same(given: &str, token: &str) -> bool {
    let (a, b) = (given.as_bytes(), token.as_bytes());
    if a.len() != b.len() || b.is_empty() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The host's `say` (macOS), one process a line, each started before its line
/// comes: a `say` waiting on its stdin has paid its ~0.4 s start, so a line
/// costs only its rendering (measured on macOS 26, 5 runs each, medians, line
/// handed over to rendered: 400, 526 and 869 ms for 2, 8 and 20 words, against
/// 700, 839 and 1,149 ms started with the line). It renders the whole line to
/// a WAV file at 24 kHz mono S16LE, which is then handed on.
pub struct Say {
    /// A private folder for the rendered files.
    folder: PathBuf,
    /// The `say` started for the next line, waiting on its stdin.
    ready: std::sync::Mutex<Option<Started>>,
}

/// A `say` waiting for its line, and the file it will write.
struct Started {
    child: std::process::Child,
    file: PathBuf,
}

impl Say {
    /// A `say` engine writing under `folder`, with one `say` started at once.
    pub fn new(folder: PathBuf) -> Self {
        let say = Self {
            folder,
            ready: std::sync::Mutex::new(None),
        };
        *say.ready.lock().unwrap_or_else(|e| e.into_inner()) = say.start().ok();
        say
    }

    fn start(&self) -> io::Result<Started> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let file = self.folder.join(format!(
            "say-{}-{}.wav",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let child = crate::keys::command("/usr/bin/say")
            .arg("-o")
            .arg(&file)
            .args(["--file-format=WAVE", "--data-format=LEI16@24000", "-f", "-"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        Ok(Started { child, file })
    }

    /// The ready `say` if it is still waiting, else one started now; and the
    /// next one is started for the line after.
    fn take(&self) -> io::Result<Started> {
        let mut ready = self.ready.lock().unwrap_or_else(|e| e.into_inner());
        let waiting = ready.take().and_then(|mut started| {
            matches!(started.child.try_wait(), Ok(None)).then_some(started)
        });
        let this = match waiting {
            Some(started) => started,
            None => self.start()?,
        };
        *ready = self.start().ok();
        Ok(this)
    }
}

impl Engine for Say {
    fn speak(&self, text: &str, out: &mut dyn FnMut(&[u8]) -> io::Result<()>) -> io::Result<()> {
        let Started { mut child, file } = self.take()?;
        let result = (|| {
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(text.as_bytes())?;
            }
            let status = child.wait()?;
            if !status.success() {
                return Err(io::Error::other(format!("say ended: {status}")));
            }
            let wav = std::fs::read(&file)?;
            out(pcm_of_wav(&wav)?)
        })();
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_file(&file);
        result
    }
}

impl Drop for Say {
    fn drop(&mut self) {
        if let Some(mut started) = self.ready.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = started.child.kill();
            let _ = started.child.wait();
            let _ = std::fs::remove_file(&started.file);
        }
    }
}

/// The samples of a WAV file: the bytes of its `data` chunk.
pub fn pcm_of_wav(wav: &[u8]) -> io::Result<&[u8]> {
    let bad = |why: &str| io::Error::new(io::ErrorKind::InvalidData, format!("WAV: {why}"));
    if wav.len() < 12 || &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return Err(bad("no RIFF/WAVE header"));
    }
    let mut at = 12;
    while at + 8 <= wav.len() {
        let id = &wav[at..at + 4];
        let size =
            u32::from_le_bytes([wav[at + 4], wav[at + 5], wav[at + 6], wav[at + 7]]) as usize;
        let body = at + 8;
        if id == b"data" {
            let end = body.saturating_add(size).min(wav.len());
            return Ok(&wav[body..end - (end - body) % 2]);
        }
        at = body.saturating_add(size + size % 2);
    }
    Err(bad("no data chunk"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Speaks every line as the same bytes, in two pieces.
    struct Fixed(Vec<u8>, std::sync::Mutex<Vec<String>>);
    impl Engine for Fixed {
        fn speak(
            &self,
            text: &str,
            out: &mut dyn FnMut(&[u8]) -> io::Result<()>,
        ) -> io::Result<()> {
            self.1.lock().unwrap().push(text.to_owned());
            let (a, b) = self.0.split_at(self.0.len() / 2);
            out(a)?;
            out(b)
        }
    }

    fn start(engine: Arc<Fixed>) -> SocketAddr {
        let listener = bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || serve(listener, "t0ken".to_owned(), engine));
        address
    }

    /// The whole raw answer to one request.
    fn ask(address: SocketAddr, request: &str) -> Vec<u8> {
        let mut stream = TcpStream::connect(address).unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut answer = Vec::new();
        stream.read_to_end(&mut answer).unwrap();
        answer
    }

    fn post(path: &str, token: Option<&str>, body: &str) -> String {
        let auth = token.map_or(String::new(), |t| format!("Authorization: Bearer {t}\r\n"));
        format!(
            "POST {path} HTTP/1.1\r\nHost: x\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
    }

    /// The body of a chunked answer, put back together.
    fn unchunk(mut rest: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let end = rest.windows(2).position(|w| w == b"\r\n").unwrap();
            let size =
                usize::from_str_radix(std::str::from_utf8(&rest[..end]).unwrap(), 16).unwrap();
            rest = &rest[end + 2..];
            if size == 0 {
                return out;
            }
            out.extend_from_slice(&rest[..size]);
            rest = &rest[size + 2..];
        }
    }

    #[test]
    fn a_line_with_the_token_is_answered_as_chunked_pcm_the_engine_made() {
        let pcm: Vec<u8> = (0..4_800u32).map(|i| i as u8).collect();
        let engine = Arc::new(Fixed(pcm.clone(), Default::default()));
        let address = start(engine.clone());
        let raw = ask(
            address,
            &post(
                "/v1/audio/speech",
                Some("t0ken"),
                r#"{"model":"tts-1","input":"  Hi there.  ","voice":"nova","response_format":"pcm"}"#,
            ),
        );
        let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let head = std::str::from_utf8(&raw[..split]).unwrap();
        assert!(head.starts_with("HTTP/1.1 200 OK"), "{head}");
        assert!(head.contains("Transfer-Encoding: chunked"), "{head}");
        assert!(head.contains("Content-Type: audio/pcm"), "{head}");
        assert_eq!(unchunk(&raw[split + 4..]), pcm);
        assert_eq!(*engine.1.lock().unwrap(), ["Hi there."]);
    }

    #[test]
    fn without_this_runs_token_nothing_is_spoken() {
        let engine = Arc::new(Fixed(vec![1, 2], Default::default()));
        let address = start(engine.clone());
        let body = r#"{"input":"Hi.","response_format":"pcm"}"#;
        for token in [None, Some("wrong"), Some("t0ke"), Some("")] {
            let raw = ask(address, &post("/audio/speech", token, body));
            assert!(raw.starts_with(b"HTTP/1.1 401"), "{token:?}");
        }
        assert!(engine.1.lock().unwrap().is_empty());
    }

    #[test]
    fn only_pcm_with_an_input_on_the_speech_path_by_post_is_spoken() {
        let engine = Arc::new(Fixed(vec![1, 2], Default::default()));
        let address = start(engine.clone());
        for (request, status) in [
            (
                post("/audio/speech", Some("t0ken"), r#"{"input":"Hi."}"#),
                "400",
            ),
            (
                post(
                    "/audio/speech",
                    Some("t0ken"),
                    r#"{"input":"Hi.","response_format":"mp3"}"#,
                ),
                "400",
            ),
            (
                post(
                    "/audio/speech",
                    Some("t0ken"),
                    r#"{"input":"  ","response_format":"pcm"}"#,
                ),
                "400",
            ),
            (post("/audio/speech", Some("t0ken"), "not json"), "400"),
            (post("/v1/chat/completions", Some("t0ken"), "{}"), "404"),
            (
                "GET /audio/speech HTTP/1.1\r\nAuthorization: Bearer t0ken\r\n\r\n".to_owned(),
                "405",
            ),
        ] {
            let raw = ask(address, &request);
            assert!(
                raw.starts_with(format!("HTTP/1.1 {status}").as_bytes()),
                "{request}: {}",
                String::from_utf8_lossy(&raw)
            );
        }
        assert!(engine.1.lock().unwrap().is_empty());
    }

    /// An engine that fails before any sound, as `say` does where there is none.
    struct Absent;
    impl Engine for Absent {
        fn speak(&self, _: &str, _: &mut dyn FnMut(&[u8]) -> io::Result<()>) -> io::Result<()> {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "No such file or directory",
            ))
        }
    }

    #[test]
    fn an_engine_that_fails_before_any_sound_is_a_503_with_its_reason() {
        let listener = bind("127.0.0.1:0".parse().unwrap()).unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || serve(listener, "t0ken".to_owned(), Arc::new(Absent)));
        let raw = ask(
            address,
            &post(
                "/audio/speech",
                Some("t0ken"),
                r#"{"input":"Hi.","response_format":"pcm"}"#,
            ),
        );
        let text = String::from_utf8_lossy(&raw);
        assert!(text.starts_with("HTTP/1.1 503"), "{text}");
        assert!(text.contains("No such file or directory"), "{text}");
    }

    #[test]
    fn a_request_sent_a_byte_at_a_time_is_cut_off_at_the_whole_requests_wait() {
        let engine = Arc::new(Fixed(vec![1, 2], Default::default()));
        let address = start(engine.clone());
        let mut stream = TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_millis(50)))
            .unwrap();
        let asked = Instant::now();
        let mut answer = Vec::new();
        // A byte every 0.5 s, each well inside a per-read wait, never ending the head.
        for byte in b"POST /audio/speech HTTP/1.1\r\nX-Slow: aaaaaaaaaaaaaaaaaaaaaa".iter() {
            if stream.write_all(&[*byte]).is_err() {
                break;
            }
            let mut buffer = [0u8; 256];
            if let Ok(n) = stream.read(&mut buffer) {
                answer.extend_from_slice(&buffer[..n]);
                if n == 0 || !answer.is_empty() {
                    // The rest of the answer, to its close (the service closes). The
                    // timeout is not changed here: on macOS setting one on a socket
                    // whose peer has closed fails with EINVAL (nine's read, CI run
                    // 43477638713); the 50 ms set at the start is enough to reach EOF.
                    let _ = stream.read_to_end(&mut answer);
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(450));
            if asked.elapsed() > Duration::from_secs(9) {
                break;
            }
        }
        let took = asked.elapsed();
        assert!(
            answer.starts_with(b"HTTP/1.1 400"),
            "{}",
            String::from_utf8_lossy(&answer)
        );
        assert!(
            took >= READ_WAIT - Duration::from_millis(600) && took < Duration::from_secs(8),
            "{took:?}"
        );
        assert!(engine.1.lock().unwrap().is_empty());
    }

    #[test]
    fn it_listens_on_loopback_only() {
        for address in ["0.0.0.0:0", "192.168.1.2:0", "[::]:0"] {
            let error = bind(address.parse().unwrap()).unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{address}");
        }
        assert!(bind("127.0.0.1:0".parse().unwrap()).is_ok());
    }

    #[test]
    fn a_wavs_data_chunk_is_found_past_other_chunks() {
        let mut wav = b"RIFF\0\0\0\0WAVE".to_vec();
        wav.extend_from_slice(b"fmt \x10\0\0\0");
        wav.extend_from_slice(&[0; 16]);
        wav.extend_from_slice(b"FLLR\x03\0\0\0abc\0");
        wav.extend_from_slice(b"data\x04\0\0\0");
        wav.extend_from_slice(&[1, 2, 3, 4]);
        assert_eq!(pcm_of_wav(&wav).unwrap(), [1, 2, 3, 4]);
        assert!(pcm_of_wav(b"RIFF\0\0\0\0WAVE").is_err());
        assert!(pcm_of_wav(b"not a wav").is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_say_speaks_a_line_as_24_khz_pcm() {
        let folder = crate::test_root().join(format!("lsm-say-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let say = Say::new(folder.clone());
        // Two lines: the first on the say started with the engine, the second
        // on the one started for it.
        for _ in 0..2 {
            let mut bytes = Vec::new();
            say.speak("Hi there.", &mut |piece| {
                bytes.extend_from_slice(piece);
                Ok(())
            })
            .unwrap();
            // "Hi there." is about 0.6 s: over 0.2 s and under 3 s of 24 kHz S16LE.
            assert_eq!(bytes.len() % 2, 0);
            assert!(
                bytes.len() > 2 * 4_800 && bytes.len() < 2 * 72_000,
                "{}",
                bytes.len()
            );
        }
        drop(say);
        // Nothing is left in the folder: the spoken files and the ready one's.
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
