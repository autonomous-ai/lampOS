use super::*;
use crate::fetch::{Connect, Resolve, Wire};
use crate::protocol::{JSON, read_frame, write_frame};
use std::io::Cursor;
use std::net::{IpAddr, Ipv4Addr, SocketAddrV4};
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};

const MP3: &[u8] = include_bytes!("../../tests/fixtures/tone-44k-stereo.mp3");

struct Canned(Cursor<Vec<u8>>);
impl Read for Canned {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buffer)
    }
}
impl Write for Canned {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct World {
    body: Vec<u8>,
    connects: Arc<Mutex<u32>>,
}

impl Resolve for World {
    fn resolve(&mut self, _host: &str) -> std::io::Result<Vec<IpAddr>> {
        Ok(vec![IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34))])
    }
}
impl Connect for World {
    fn connect(&mut self, _host: &str, _address: SocketAddrV4) -> std::io::Result<Box<dyn Wire>> {
        *self.connects.lock().unwrap() += 1;
        let mut answer = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\nContent-Length: {}\r\n\r\n",
            self.body.len()
        )
        .into_bytes();
        answer.extend_from_slice(&self.body);
        Ok(Box::new(Canned(Cursor::new(answer))))
    }
}

/// Runs `serve_connection` on one end of a socket pair, `ask` writing the
/// request on the other end; the events and sound read back.
type Frames = Vec<(u8, Vec<u8>)>;

fn run(body: &[u8], url: &str, hosts: &[&str]) -> (Option<Ended>, Frames, u32) {
    let (mut near, mut far) = UnixStream::pair().unwrap();
    let connects = Arc::new(Mutex::new(0));
    let mut world = World {
        body: body.to_vec(),
        connects: Arc::clone(&connects),
    };
    let request = format!(
        r#"{{"op":"play","id":9,"url":"{url}","hosts":[{}]}}"#,
        hosts
            .iter()
            .map(|host| format!("\"{host}\""))
            .collect::<Vec<_>>()
            .join(",")
    );
    write_frame(&mut far, JSON, request.as_bytes()).unwrap();
    let server = std::thread::spawn(move || {
        let ended = serve_connection(&mut near, &mut world, Limits::default());
        drop(near);
        ended
    });
    let mut frames = Vec::new();
    while let Ok(frame) = read_frame(&mut far, 70_000) {
        frames.push(frame);
    }
    let ended = server.join().unwrap();
    let connected = *connects.lock().unwrap();
    (ended, frames, connected)
}

fn text(frame: &(u8, Vec<u8>)) -> String {
    String::from_utf8_lossy(&frame.1).into_owned()
}

#[test]
fn a_good_play_sends_started_then_sound_in_door_sized_frames_then_ended() {
    let (ended, frames, connected) =
        run(MP3, "https://cdn.example.com/a.mp3", &["cdn.example.com"]);
    let ended = ended.unwrap();
    assert_eq!((ended.id, ended.outcome), (9, "done"));
    assert_eq!(ended.seconds, 1);
    assert!(ended.bytes > 8_000);
    assert_eq!(connected, 1);
    assert_eq!(text(&frames[0]), r#"{"ev":"started","id":9}"#);
    let last = frames.last().unwrap();
    assert!(
        text(last).starts_with(r#"{"ev":"ended","id":9,"outcome":"done","seconds":1"#),
        "{}",
        text(last)
    );
    let sound: Vec<&Vec<u8>> = frames
        .iter()
        .filter(|(kind, _)| *kind == PCM)
        .map(|(_, bytes)| bytes)
        .collect();
    assert!(
        sound
            .iter()
            .all(|bytes| bytes.len() <= MOST_PCM && bytes.len() % 2 == 0)
    );
    let samples: usize = sound.iter().map(|bytes| bytes.len() / 2).sum();
    assert!((22_000..=27_500).contains(&samples), "{samples}");
}

#[test]
fn a_host_not_named_ends_refused_before_any_connect_and_plays_nothing() {
    let (ended, frames, connected) =
        run(MP3, "https://evil.example.org/a.mp3", &["cdn.example.com"]);
    assert_eq!(ended.unwrap().outcome, "refused_host");
    assert_eq!(connected, 0);
    assert_eq!(frames.len(), 1, "only the ended event");
    assert!(text(&frames[0]).contains(r#""outcome":"refused_host""#));
    for url in [
        "http://cdn.example.com/a",
        "https://cdn.example.com:81/a",
        "https://10.0.0.1/a",
    ] {
        let (ended, _, connected) = run(MP3, url, &["cdn.example.com"]);
        assert_eq!(ended.unwrap().outcome, "refused_host", "{url}");
        assert_eq!(connected, 0, "{url}");
    }
    // A set that is not plain names is refused whole.
    let (ended, _, connected) = run(
        MP3,
        "https://cdn.example.com/a",
        &["cdn.example.com", "10.0.0.1"],
    );
    assert_eq!(ended.unwrap().outcome, "refused_host");
    assert_eq!(connected, 0);
}

#[test]
fn a_file_that_is_not_an_mp3_ends_bad_audio_with_no_sound_and_no_start() {
    let (ended, frames, _) = run(
        b"<html>not audio</html>",
        "https://cdn.example.com/a.mp3",
        &["cdn.example.com"],
    );
    let ended = ended.unwrap();
    assert_eq!(ended.outcome, "bad_audio");
    assert_eq!(ended.seconds, 0);
    assert_eq!(frames.len(), 1);
    assert!(text(&frames[0]).contains("bad_audio"));
}

#[test]
fn a_peer_that_goes_away_stops_the_play() {
    let (mut near, mut far) = UnixStream::pair().unwrap();
    let mut long = MP3.to_vec();
    // The same file again and again: several seconds of sound.
    for _ in 0..40 {
        long.extend_from_slice(MP3);
    }
    let mut world = World {
        body: long,
        connects: Arc::new(Mutex::new(0)),
    };
    write_frame(
        &mut far,
        JSON,
        br#"{"op":"play","id":4,"url":"https://cdn.example.com/a.mp3","hosts":["cdn.example.com"]}"#,
    )
    .unwrap();
    let server =
        std::thread::spawn(move || serve_connection(&mut near, &mut world, Limits::default()));
    // Read the start and one block of sound, then leave.
    let (_, started) = read_frame(&mut far, 70_000).unwrap();
    assert!(String::from_utf8_lossy(&started).contains("started"));
    let (kind, _) = read_frame(&mut far, 70_000).unwrap();
    assert_eq!(kind, PCM);
    drop(far);
    let ended = server.join().unwrap().unwrap();
    assert_eq!(ended.outcome, "stopped");
}

#[test]
fn a_connection_that_does_not_speak_the_protocol_gets_no_answer() {
    for bytes in [vec![], vec![0, 0, 0, 1], b"GET / HTTP/1.1\r\n\r\n".to_vec()] {
        let (mut near, mut far) = UnixStream::pair().unwrap();
        std::io::Write::write_all(&mut far, &bytes).unwrap();
        drop(far);
        let mut world = World {
            body: MP3.to_vec(),
            connects: Arc::new(Mutex::new(0)),
        };
        assert_eq!(
            serve_connection(&mut near, &mut world, Limits::default()),
            None
        );
        assert_eq!(*world.connects.lock().unwrap(), 0);
    }
}

#[test]
fn nothing_the_journal_may_keep_holds_the_url() {
    let (ended, frames, _) = run(
        MP3,
        "https://cdn.example.com/secret/path.mp3?token=abc",
        &["cdn.example.com"],
    );
    let ended = ended.unwrap();
    let kept = format!("{ended:?}");
    assert!(
        !kept.contains("secret") && !kept.contains("token") && !kept.contains("cdn.example.com"),
        "{kept}"
    );
    for frame in frames.iter().filter(|(kind, _)| *kind == JSON) {
        let json = text(frame);
        assert!(
            !json.contains("secret") && !json.contains("token") && !json.contains("cdn"),
            "{json}"
        );
    }
}
