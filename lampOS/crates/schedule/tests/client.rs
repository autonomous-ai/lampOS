//! Scripted Unix endpoints only; no lamp or production Schedule service.
use lamp_schedule::client::{ErrorKind, MAX_BYTES, connect, exchange, exchange_guarded};
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

fn fixture() -> (tempfile::TempDir, PathBuf, UnixListener) {
    let root = tempfile::Builder::new()
        .prefix("schc-")
        .tempdir_in(fs::canonicalize(std::env::temp_dir()).unwrap())
        .unwrap();
    let socket = root.path().join("c.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    (root, socket, listener)
}
fn incoming(listener: UnixListener) -> UnixStream {
    let (stream, _) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
}
fn read_request(stream: &mut UnixStream) -> Value {
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}
fn until() -> Instant {
    Instant::now() + Duration::from_secs(3)
}

#[test]
fn standalone_connect_returns_nonblocking_without_sending_a_request() {
    let (_root, socket, listener) = fixture();
    let stream = connect(&socket, until()).unwrap();
    assert!(
        rustix::fs::fcntl_getfl(&stream)
            .unwrap()
            .contains(rustix::fs::OFlags::NONBLOCK)
    );
    let (mut peer, _) = listener.accept().unwrap();
    peer.set_nonblocking(true).unwrap();
    assert_eq!(
        peer.read(&mut [0]).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn guarded_exchange_checks_authority_after_connect_before_sending() {
    use std::sync::{Arc, Mutex, mpsc};

    let (_root, socket, listener) = fixture();
    let allowed = Arc::new(Mutex::new(true));
    let revoked = Arc::clone(&allowed);
    let (send, receive) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        let mut stream = incoming(listener);
        *revoked.lock().unwrap() = false;
        send.send(()).unwrap();
        assert_eq!(stream.read(&mut [0]).unwrap(), 0);
    });
    let error = exchange_guarded(&socket, &json!({"op":"set"}), until(), || {
        receive.recv_timeout(Duration::from_secs(1)).unwrap();
        let guard = allowed.lock().unwrap();
        (*guard).then_some(guard)
    })
    .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unavailable);
    assert!(!error.may_have_sent);
    worker.join().unwrap();
}

#[test]
fn guarded_exchange_releases_authority_before_waiting_for_reply() {
    use std::sync::{Arc, Mutex};

    let (_root, socket, listener) = fixture();
    let allowed = Arc::new(Mutex::new(true));
    let revoked = Arc::clone(&allowed);
    let worker = thread::spawn(move || {
        let mut stream = incoming(listener);
        assert_eq!(read_request(&mut stream), json!({"op":"set"}));
        *revoked.lock().unwrap() = false;
        stream.write_all(b"{\"ok\":true}\n").unwrap();
    });
    let answer = exchange_guarded(&socket, &json!({"op":"set"}), until(), || {
        let guard = allowed.lock().unwrap();
        (*guard).then_some(guard)
    })
    .unwrap();
    assert_eq!(answer, json!({"ok":true}));
    worker.join().unwrap();
}

#[test]
fn fragmented_reply_and_escaped_content_are_one_bounded_object() {
    let (_root, socket, listener) = fixture();
    let worker = thread::spawn(move || {
        let mut stream = incoming(listener);
        assert_eq!(read_request(&mut stream), json!({"op":"get","id":1}));
        for byte in b"{\"ok\":true,\"text\":\"quote\\\" and \\n newline\"}\n" {
            stream.write_all(&[*byte]).unwrap();
        }
    });
    let got = exchange(&socket, &json!({"op":"get","id":1}), until()).unwrap();
    assert_eq!(got["text"], "quote\" and \n newline");
    worker.join().unwrap();
}

#[test]
fn validates_path_object_and_size_before_opening_a_socket() {
    let (_root, socket, listener) = fixture();
    listener.set_nonblocking(true).unwrap();
    for request in [json!(null), json!([])] {
        let error = exchange(&socket, &request, until()).unwrap_err();
        assert_eq!(error.kind, ErrorKind::InvalidRequest);
        assert!(!error.may_have_sent);
    }
    let error = exchange(&socket, &json!({"x":"x".repeat(MAX_BYTES)}), until()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::TooLong);
    assert!(!error.may_have_sent);
    for path in ["relative", "/a/../b", "/a\0b"] {
        let error = exchange(std::path::Path::new(path), &json!({}), until()).unwrap_err();
        assert_eq!(error.kind, ErrorKind::InvalidSocket);
        assert!(!error.may_have_sent);
    }
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn expired_deadline_never_connects_and_missing_socket_is_not_sent() {
    let (_root, socket, listener) = fixture();
    listener.set_nonblocking(true).unwrap();
    let error = connect(&socket, Instant::now()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Timeout);
    assert!(!error.may_have_sent);
    assert!(listener.accept().is_err());
    let error = exchange(&socket, &json!({}), Instant::now()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Timeout);
    assert!(!error.may_have_sent);
    assert!(listener.accept().is_err());
    let error = exchange(&socket.with_file_name("missing"), &json!({}), until()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unavailable);
    assert!(!error.may_have_sent);
}

#[test]
fn absent_reply_after_complete_request_is_uncertain_and_not_retried() {
    let (_root, socket, listener) = fixture();
    let worker = thread::spawn(move || {
        let mut stream = incoming(listener);
        assert_eq!(read_request(&mut stream), json!({"op":"set"}));
        let mut byte = [0];
        assert_eq!(stream.read(&mut byte).unwrap(), 0);
    });
    let error = exchange(
        &socket,
        &json!({"op":"set"}),
        Instant::now() + Duration::from_millis(150),
    )
    .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Timeout);
    assert!(error.may_have_sent);
    worker.join().unwrap();
}

#[test]
fn closed_truncated_nonobject_and_invalid_replies_are_unknown_after_send() {
    for reply in [
        b"".as_slice(),
        b"{}",
        b"[]\n",
        b"not json\n",
        b"{}\n{}\n",
        b"{\"ok\":\xff}\n",
    ] {
        let (_root, socket, listener) = fixture();
        let reply = reply.to_vec();
        let worker = thread::spawn(move || {
            let mut stream = incoming(listener);
            read_request(&mut stream);
            stream.write_all(&reply).unwrap();
        });
        let error = exchange(&socket, &json!({}), until()).unwrap_err();
        assert_eq!(error.kind, ErrorKind::InvalidResponse);
        assert!(error.may_have_sent);
        worker.join().unwrap();
    }
}

#[test]
fn exact_reply_bound_is_accepted_and_one_byte_more_is_rejected() {
    for (size, accepted) in [(MAX_BYTES, true), (MAX_BYTES + 1, false)] {
        let (_root, socket, listener) = fixture();
        let response = json!({"x":"a".repeat(size - 8)}).to_string();
        assert_eq!(response.len(), size);
        let worker = thread::spawn(move || {
            let mut stream = incoming(listener);
            read_request(&mut stream);
            let _ = writeln!(stream, "{response}");
        });
        let got = exchange(&socket, &json!({}), until());
        if accepted {
            assert!(got.is_ok());
        } else {
            let error = got.unwrap_err();
            assert_eq!(error.kind, ErrorKind::TooLong);
            assert!(error.may_have_sent);
        }
        worker.join().unwrap();
    }
}

#[test]
fn a_second_hop_keeps_the_original_absolute_deadline() {
    let (_root, socket, listener) = fixture();
    let worker = thread::spawn(move || {
        for index in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            read_request(&mut stream);
            if index == 0 {
                thread::sleep(Duration::from_millis(100));
                stream.write_all(b"{\"ok\":true}\n").unwrap();
            } else {
                let mut byte = [0];
                assert_eq!(stream.read(&mut byte).unwrap(), 0);
            }
        }
    });
    // 800 ms, eight times the first hop's 100 ms: the first still answers
    // inside it, the second runs into it (#1806: 2 s took 2.0 s on ARM64).
    let deadline = Instant::now() + Duration::from_millis(800);
    exchange(&socket, &json!({"op":"status"}), deadline).unwrap();
    let error = exchange(&socket, &json!({"op":"set"}), deadline).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Timeout);
    assert!(error.may_have_sent);
    worker.join().unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn full_unix_accept_queue_is_unavailable_not_a_false_connection() {
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
            Err(e) if e == rustix::io::Errno::AGAIN => {
                full = true;
                break;
            }
            Err(e) => panic!("unexpected fixture connect: {e}"),
        }
    }
    assert!(full);
    let error = connect(&socket, until()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unavailable);
    assert!(!error.may_have_sent);
    let error = exchange(&socket, &json!({}), until()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unavailable);
    assert!(!error.may_have_sent);
}
