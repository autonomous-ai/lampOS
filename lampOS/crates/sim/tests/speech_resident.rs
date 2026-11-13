//! `lamp-sim speech` with the resident synthesizer (macOS): the real binary,
//! as its synthesizer needs a main thread of its own, which a unit test does
//! not have. It must answer a line with the token as 24 kHz PCM, quickly once
//! warm, and refuse one without.
#![cfg(target_os = "macos")]

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Service {
    child: Child,
    folder: std::path::PathBuf,
}

impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}

/// The service on a port of the system's choosing, and its address.
fn start() -> (Service, String) {
    let folder =
        std::path::PathBuf::from("/tmp").join(format!("lsm-resident-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    let mut child = Command::new(env!("CARGO_BIN_EXE_lamp-sim"))
        .args(["speech"])
        .arg(&folder)
        .args(["--listen", "127.0.0.1:0", "--engine", "resident"])
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stderr.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let address = line
        .split("http://")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or_else(|| panic!("no address in {line:?}"))
        .to_owned();
    (Service { child, folder }, address)
}

/// The status and the PCM bytes of one answer (a chunked body put together).
fn ask(address: &str, token: &str, text: &str) -> (u16, Vec<u8>) {
    let body = format!(r#"{{"input":"{text}","response_format":"pcm"}}"#);
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    write!(
        stream,
        "POST /v1/audio/speech HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {token}\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let status = std::str::from_utf8(&raw[9..12]).unwrap().parse().unwrap();
    if status != 200 {
        return (status, Vec::new());
    }
    let (mut rest, mut pcm) = (&raw[split + 4..], Vec::new());
    loop {
        let end = rest.windows(2).position(|w| w == b"\r\n").unwrap();
        let size = usize::from_str_radix(std::str::from_utf8(&rest[..end]).unwrap(), 16).unwrap();
        rest = &rest[end + 2..];
        if size == 0 {
            return (status, pcm);
        }
        pcm.extend_from_slice(&rest[..size]);
        rest = &rest[size + 2..];
    }
}

#[test]
fn the_resident_synthesizer_speaks_a_line_with_the_token_as_24_khz_pcm() {
    let (service, address) = start();
    let token = std::fs::read_to_string(service.folder.join("speech.token")).unwrap();
    let token = token.trim();
    // The first line after start loads the voice.
    let (status, pcm) = ask(&address, token, "Hello.");
    assert_eq!(status, 200);
    assert!(!pcm.is_empty());
    // A warm line: "Hi there." is about 0.6 s, so well over 0.2 s and under
    // 3 s of 24 kHz S16LE, whole; and whole well within a second here (about
    // 30 ms measured; the bound is for a loaded runner).
    let asked = Instant::now();
    let (status, pcm) = ask(&address, token, "Hi there.");
    let took = asked.elapsed();
    assert_eq!(status, 200);
    assert_eq!(pcm.len() % 2, 0);
    assert!(
        pcm.len() > 2 * 4_800 && pcm.len() < 2 * 72_000,
        "{}",
        pcm.len()
    );
    assert!(took < Duration::from_secs(1), "{took:?}");
    // Not every sample is silence.
    assert!(
        pcm.chunks(2)
            .any(|b| i16::from_le_bytes([b[0], b[1]]).abs() > 1_000)
    );
    // Without the token, nothing.
    assert_eq!(ask(&address, "wrong", "Hi.").0, 401);
}
