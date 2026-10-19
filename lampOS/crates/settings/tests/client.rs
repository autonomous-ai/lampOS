//! Scripted private Unix sockets only; no settings owner or physical device.
use lamp_settings::{
    client::{ErrorKind, exchange, validate_socket},
    protocol::{MAX_REPLY_BYTES, MAX_REQUEST_BYTES},
};
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, Read, Write},
    os::{
        fd::AsFd,
        unix::net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const WATCHDOG: Duration = Duration::from_secs(5);

fn fixture() -> (tempfile::TempDir, PathBuf, UnixListener) {
    let root = tempfile::Builder::new()
        .prefix("setc-")
        .tempdir_in(fs::canonicalize(std::env::temp_dir()).unwrap())
        .unwrap();
    let socket = root.path().join("c.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();
    (root, socket, listener)
}

fn until() -> Instant {
    Instant::now() + Duration::from_secs(3)
}

fn ready(fd: &impl AsFd, interest: PollFlags, deadline: Instant) -> io::Result<()> {
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|left| !left.is_zero())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "fixture deadline"))?;
        let timeout = Timespec::try_from(remaining)
            .map_err(|_| io::Error::other("fixture deadline overflow"))?;
        let mut descriptors = [PollFd::new(fd, interest)];
        match poll(&mut descriptors, Some(&timeout)) {
            Ok(0) => {}
            Ok(_) => {
                let events = descriptors[0].revents();
                if events.contains(PollFlags::NVAL) {
                    return Err(io::Error::other("fixture descriptor closed"));
                }
                if events.intersects(interest | PollFlags::HUP | PollFlags::ERR) {
                    return Ok(());
                }
            }
            Err(error) if error == rustix::io::Errno::INTR => {}
            Err(error) => return Err(error.into()),
        }
    }
}

fn incoming(listener: &UnixListener, deadline: Instant) -> io::Result<UnixStream> {
    loop {
        ready(listener, PollFlags::IN, deadline)?;
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(true)?;
                return Ok(stream);
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) => {}
            Err(error) => return Err(error),
        }
    }
}

fn read_request(stream: &mut UnixStream, deadline: Instant) -> io::Result<Value> {
    let mut line = Vec::new();
    let mut buffer = [0; 1024];
    loop {
        ready(stream, PollFlags::IN, deadline)?;
        match stream.read(&mut buffer) {
            Ok(0) => return Err(io::Error::other("fixture request was truncated")),
            Ok(count) => {
                if count > (MAX_REQUEST_BYTES + 1).saturating_sub(line.len()) {
                    return Err(io::Error::other("fixture request exceeded its bound"));
                }
                line.extend_from_slice(&buffer[..count]);
                if line.last() == Some(&b'\n') {
                    return serde_json::from_slice(&line).map_err(io::Error::other);
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) => {}
            Err(error) => return Err(error),
        }
    }
}

fn write_reply(stream: &mut UnixStream, mut bytes: &[u8], deadline: Instant) -> io::Result<()> {
    while !bytes.is_empty() {
        ready(stream, PollFlags::OUT, deadline)?;
        match stream.write(bytes) {
            Ok(0) => return Err(io::Error::other("fixture reply write stopped")),
            Ok(count) => bytes = &bytes[count..],
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn expect_closed_without_more_request(stream: &mut UnixStream, deadline: Instant) {
    loop {
        ready(stream, PollFlags::IN, deadline).unwrap();
        let mut byte = [0];
        match stream.read(&mut byte) {
            Ok(count) => {
                assert_eq!(
                    count, 0,
                    "client sent more bytes after its complete request"
                );
                return;
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                ) => {}
            Err(error) => panic!("fixture waiting for peer close: {error}"),
        }
    }
}

/// Join even when a client assertion unwinds. All peer I/O uses one absolute
/// watchdog, so an unexpected missing connection cannot leave a blocked thread.
struct Peer(Option<JoinHandle<()>>);

impl Peer {
    fn spawn(
        listener: UnixListener,
        script: impl FnOnce(&UnixListener, Instant) + Send + 'static,
    ) -> Self {
        Self(Some(thread::spawn(move || {
            script(&listener, Instant::now() + WATCHDOG);
        })))
    }

    fn join(mut self) {
        self.0.take().unwrap().join().unwrap();
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        if let Some(worker) = self.0.take() {
            let _ = worker.join();
        }
    }
}

#[test]
fn fragmented_reply_preserves_escaped_content_and_accepts_trailing_whitespace() {
    let (_root, socket, listener) = fixture();
    let worker = Peer::spawn(listener, |listener, deadline| {
        let mut stream = incoming(listener, deadline).unwrap();
        assert_eq!(
            read_request(&mut stream, deadline).unwrap(),
            json!({"op":"read"})
        );
        for byte in b"{\"ok\":true,\"text\":\"quote\\\" and \\n newline\"}" {
            write_reply(&mut stream, &[*byte], deadline).unwrap();
        }
        // Send the terminator and allowed whitespace together: the client may
        // close as soon as it receives a complete frame.
        write_reply(&mut stream, b"\n \t\r", deadline).unwrap();
    });
    let got = exchange(&socket, &json!({"op":"read"}), until());
    worker.join();
    assert_eq!(got.unwrap()["text"], "quote\" and \n newline");
}

#[test]
fn invalid_or_oversized_requests_are_refused_before_connecting() {
    let (_root, socket, listener) = fixture();
    for request in [json!(null), json!([]), json!("text"), json!(true)] {
        let error = exchange(&socket, &request, until()).unwrap_err();
        assert_eq!(error.kind, ErrorKind::InvalidRequest);
        assert!(!error.may_have_sent);
    }
    // The second request is short before JSON escaping, but too large on wire.
    for request in [
        json!({"x":"x".repeat(MAX_REQUEST_BYTES)}),
        json!({"x":"\n".repeat(MAX_REQUEST_BYTES / 2)}),
    ] {
        let error = exchange(&socket, &request, until()).unwrap_err();
        assert_eq!(error.kind, ErrorKind::TooLong);
        assert!(!error.may_have_sent);
    }
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
}

#[test]
fn invalid_paths_and_expired_deadline_never_connect() {
    let (_root, socket, listener) = fixture();
    for path in ["relative", "/a/../b", "/a\0b", ""] {
        let error = exchange(Path::new(path), &json!({}), until()).unwrap_err();
        assert_eq!(error.kind, ErrorKind::InvalidSocket);
        assert!(!error.may_have_sent);
    }
    assert_eq!(
        validate_socket(Path::new(&format!("/{}", "a".repeat(103))))
            .unwrap_err()
            .kind,
        ErrorKind::InvalidSocket
    );
    let error = exchange(&socket, &json!({}), Instant::now()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Timeout);
    assert!(!error.may_have_sent);
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
}

#[test]
fn missing_stale_and_nonsocket_endpoints_are_unavailable_without_sending() {
    let (root, socket, listener) = fixture();
    let ordinary = root.path().join("ordinary");
    fs::write(&ordinary, b"private fixture").unwrap();
    drop(listener);
    for path in [socket.with_file_name("missing"), socket, ordinary] {
        let error = exchange(&path, &json!({"op":"status"}), until()).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Unavailable);
        assert!(!error.may_have_sent);
    }
}

#[test]
fn closed_truncated_nonobject_malformed_and_multiple_replies_are_ambiguous() {
    for reply in [
        b"".as_slice(),
        b"{}",
        b"{\"ok\":",
        b"[]\n",
        b"true\n",
        b"null\n",
        b"not json\n",
        b"{} {}\n",
        b"{}\n{}\n",
        b"{\"ok\":\xff}\n",
    ] {
        let (_root, socket, listener) = fixture();
        let reply = reply.to_vec();
        let worker = Peer::spawn(listener, move |listener, deadline| {
            let mut stream = incoming(listener, deadline).unwrap();
            assert_eq!(
                read_request(&mut stream, deadline).unwrap(),
                json!({"op":"set"})
            );
            write_reply(&mut stream, &reply, deadline).unwrap();
        });
        let result = exchange(&socket, &json!({"op":"set"}), until());
        worker.join();
        let error = result.unwrap_err();
        assert_eq!(error.kind, ErrorKind::InvalidResponse);
        assert!(error.may_have_sent);
    }
}

#[test]
fn exact_reply_limit_is_accepted_and_one_more_byte_is_refused() {
    for (size, accepted) in [(MAX_REPLY_BYTES, true), (MAX_REPLY_BYTES + 1, false)] {
        let (_root, socket, listener) = fixture();
        let mut response = json!({"x":"a".repeat(size - 8)}).to_string().into_bytes();
        assert_eq!(response.len(), size);
        response.push(b'\n');
        let worker = Peer::spawn(listener, move |listener, deadline| {
            let mut stream = incoming(listener, deadline).unwrap();
            read_request(&mut stream, deadline).unwrap();
            let sent = write_reply(&mut stream, &response, deadline);
            if accepted {
                sent.unwrap();
            }
        });
        let result = exchange(&socket, &json!({"op":"read"}), until());
        worker.join();
        if accepted {
            assert_eq!(result.unwrap()["x"].as_str().unwrap().len(), size - 8);
        } else {
            let error = result.unwrap_err();
            assert_eq!(error.kind, ErrorKind::TooLong);
            assert!(error.may_have_sent);
        }
    }
}

#[test]
fn complete_request_without_reply_times_out_without_retrying_or_exposing_payload() {
    let (_root, socket, listener) = fixture();
    let worker = Peer::spawn(listener, |listener, deadline| {
        let mut stream = incoming(listener, deadline).unwrap();
        assert_eq!(
            read_request(&mut stream, deadline).unwrap(),
            json!({"op":"set","private":"do-not-log-this-value"})
        );
        expect_closed_without_more_request(&mut stream, deadline);
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
    });
    let error = exchange(
        &socket,
        &json!({"op":"set","private":"do-not-log-this-value"}),
        Instant::now() + Duration::from_secs(1),
    );
    worker.join();
    let error = error.unwrap_err();
    assert_eq!(error.kind, ErrorKind::Timeout);
    assert!(error.may_have_sent);
    assert_eq!(error.to_string(), "timeout");
    let debug = format!("{error:?}");
    assert!(!debug.contains("do-not-log-this-value"));
    assert!(!debug.contains(socket.to_str().unwrap()));
}

#[test]
fn maximum_request_is_sent_once_and_partial_response_cannot_reset_deadline() {
    let (_root, socket, listener) = fixture();
    let request = json!({"x":"a".repeat(MAX_REQUEST_BYTES - 8)});
    assert_eq!(
        serde_json::to_vec(&request).unwrap().len(),
        MAX_REQUEST_BYTES
    );
    let expected = request.clone();
    let worker = Peer::spawn(listener, move |listener, deadline| {
        let mut stream = incoming(listener, deadline).unwrap();
        assert_eq!(read_request(&mut stream, deadline).unwrap(), expected);
        write_reply(&mut stream, b"{\"ok\":", deadline).unwrap();
        expect_closed_without_more_request(&mut stream, deadline);
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
    });
    let result = exchange(&socket, &request, Instant::now() + Duration::from_secs(1));
    worker.join();
    let error = result.unwrap_err();
    assert_eq!(error.kind, ErrorKind::Timeout);
    assert!(error.may_have_sent);
}

#[test]
fn multiple_hops_share_one_absolute_deadline() {
    let (_root, socket, listener) = fixture();
    let worker = Peer::spawn(listener, |listener, deadline| {
        let mut first = incoming(listener, deadline).unwrap();
        assert_eq!(
            read_request(&mut first, deadline).unwrap(),
            json!({"op":"status"})
        );
        // This intentional dependency delay consumes the caller's budget.
        thread::sleep(Duration::from_millis(250));
        write_reply(&mut first, b"{\"ok\":true}\n", deadline).unwrap();
        let mut second = incoming(listener, deadline).unwrap();
        assert_eq!(
            read_request(&mut second, deadline).unwrap(),
            json!({"op":"set"})
        );
        expect_closed_without_more_request(&mut second, deadline);
    });
    let started = Instant::now();
    // 800 ms, over three times the first hop's 250 ms: the first still
    // answers inside it, the second runs into it (#1806: 2 s took 2.0 s on
    // ARM64).
    let deadline = started + Duration::from_millis(800);
    let first = exchange(&socket, &json!({"op":"status"}), deadline);
    let second = exchange(&socket, &json!({"op":"set"}), deadline);
    let completed = Instant::now();
    worker.join();
    first.unwrap();
    let error = second.unwrap_err();
    assert_eq!(error.kind, ErrorKind::Timeout);
    assert!(error.may_have_sent);
    assert!(completed >= deadline);
    // This is a generous host watchdog, not a Lamp latency measurement.
    assert!(completed.duration_since(started) < WATCHDOG);
}

#[cfg(target_os = "linux")]
#[test]
fn full_unix_accept_queue_is_unavailable_without_claiming_a_sent_request() {
    let (_root, socket, listener) = fixture();
    rustix::net::listen(&listener, 1).unwrap();
    let mut held = Vec::new();
    let address = rustix::net::SocketAddrUnix::new(&socket).unwrap();
    let mut full = false;
    for _ in 0..16 {
        let fd = rustix::net::socket(
            rustix::net::AddressFamily::UNIX,
            rustix::net::SocketType::STREAM,
            None,
        )
        .unwrap();
        let stream = UnixStream::from(fd);
        stream.set_nonblocking(true).unwrap();
        match rustix::net::connect(&stream, &address) {
            Ok(()) => held.push(stream),
            Err(error) if error == rustix::io::Errno::AGAIN => {
                full = true;
                break;
            }
            Err(error) => panic!("fixture connect: {error}"),
        }
    }
    assert!(full, "fixture could not fill the finite accept queue");
    let error = exchange(&socket, &json!({"op":"set"}), until()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unavailable);
    assert!(!error.may_have_sent);
}
