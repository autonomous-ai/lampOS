use std::{io::{self, Read, Write},os::unix::net::UnixStream,time::{Duration,Instant}};
fn remaining(deadline: Instant) -> io::Result<Duration> {
    deadline
        .checked_duration_since(Instant::now())
        .filter(|d| !d.is_zero())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "absolute reply deadline"))
}
fn reply_line(stream: &mut UnixStream, deadline: Instant) -> io::Result<Vec<u8>> {
    let mut line = Vec::new();
    loop {
        stream.set_read_timeout(Some(remaining(deadline)?))?;
        let mut chunk = [0; 1024];
        let n = match stream.read(&mut chunk) {
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            other => other?,
        };
        remaining(deadline)?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "incomplete reply",
            ));
        }
        if let Some(end) = chunk[..n].iter().position(|b| *b == b'\n') {
            line.extend_from_slice(&chunk[..=end]);
            if line.len() > 16 * 1024 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "oversized reply",
                ));
            }
            return Ok(line);
        }
        line.extend_from_slice(&chunk[..n]);
        if line.len() >= 16 * 1024 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "oversized reply",
            ));
        }
    }
}
#[test]
fn complete_reply_finishes_inside_absolute_deadline() {
    let (mut reader, mut peer) = UnixStream::pair().unwrap();
    peer.write_all(b"{\"ok\":true}\n").unwrap();
    let line = reply_line(&mut reader, Instant::now() + Duration::from_secs(1)).unwrap();
    assert_eq!(line, b"{\"ok\":true}\n");
}
#[test]
fn dribbling_reply_cannot_renew_absolute_deadline() {
    let (mut reader, mut peer) = UnixStream::pair().unwrap();
    peer.set_write_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    let writer = std::thread::spawn(move || {
        for _ in 0..100 {
            if peer.write_all(b" ").is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    });
    let began = Instant::now();
    let error = reply_line(&mut reader, began + Duration::from_millis(100)).unwrap_err();
    assert!(matches!(
        error.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    ));
    assert!(
        began.elapsed() < Duration::from_millis(500),
        "dribbling renewed the budget"
    );
    drop(reader);
    writer.join().unwrap();
}
