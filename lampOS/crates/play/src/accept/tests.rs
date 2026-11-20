use super::*;
use crate::fetch::{Connect, Resolve, Wire};
use crate::protocol::{JSON, write_frame};
use std::io::{Cursor, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddrV4};
use std::sync::{Arc, Mutex};

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

/// A network that counts every lookup and connect, and serves a body.
struct Counting {
    lookups: Arc<Mutex<u32>>,
    connects: Arc<Mutex<u32>>,
}
impl Resolve for Counting {
    fn resolve(&mut self, _host: &str) -> std::io::Result<Vec<IpAddr>> {
        *self.lookups.lock().unwrap() += 1;
        Ok(vec![IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34))])
    }
}
impl Connect for Counting {
    fn connect(&mut self, _host: &str, _address: SocketAddrV4) -> std::io::Result<Box<dyn Wire>> {
        *self.connects.lock().unwrap() += 1;
        let body = include_bytes!("../../tests/fixtures/tone-44k-stereo.mp3");
        let mut answer = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\nContent-Length: {}\r\n\r\n",
            body.len()
        )
        .into_bytes();
        answer.extend_from_slice(body);
        Ok(Box::new(Canned(Cursor::new(answer))))
    }
}

fn ask(admit: bool) -> (Option<Ended>, u32, u32) {
    let (near, mut far) = UnixStream::pair().unwrap();
    write_frame(
        &mut far,
        JSON,
        br#"{"op":"play","id":5,"url":"https://cdn.example.com/a.mp3","hosts":["cdn.example.com"]}"#,
    )
    .unwrap();
    let (lookups, connects) = (Arc::new(Mutex::new(0)), Arc::new(Mutex::new(0)));
    let mut net = Counting {
        lookups: Arc::clone(&lookups),
        connects: Arc::clone(&connects),
    };
    let ended = handle(near, &move |_| admit, &mut net, Limits::default());
    let counts = (*lookups.lock().unwrap(), *connects.lock().unwrap());
    (ended, counts.0, counts.1)
}

#[test]
fn a_peer_that_is_not_admitted_is_closed_and_nothing_is_looked_up_or_fetched() {
    let (ended, lookups, connects) = ask(false);
    assert_eq!(ended, None);
    assert_eq!((lookups, connects), (0, 0));
}

#[test]
fn an_admitted_peer_is_served_its_play() {
    let (ended, lookups, connects) = ask(true);
    let ended = ended.unwrap();
    assert_eq!((ended.id, ended.outcome), (5, "done"));
    assert_eq!((lookups, connects), (1, 1));
}

#[test]
fn a_same_user_peer_outside_lamp_controls_unit_is_closed_with_nothing_fetched() {
    // The real check, on a real socket pair: its peer is this test process,
    // the same user, and not in lamp-control.service's cgroup.
    let (near, mut far) = UnixStream::pair().unwrap();
    write_frame(
        &mut far,
        JSON,
        br#"{"op":"play","id":6,"url":"https://cdn.example.com/a.mp3","hosts":["cdn.example.com"]}"#,
    )
    .unwrap();
    let (lookups, connects) = (Arc::new(Mutex::new(0)), Arc::new(Mutex::new(0)));
    let mut net = Counting {
        lookups: Arc::clone(&lookups),
        connects: Arc::clone(&connects),
    };
    let admit = |stream: &UnixStream| crate::peer::peer_in_unit(stream, crate::peer::ASKER);
    assert_eq!(handle(near, &admit, &mut net, Limits::default()), None);
    assert_eq!(
        (*lookups.lock().unwrap(), *connects.lock().unwrap()),
        (0, 0)
    );
}
