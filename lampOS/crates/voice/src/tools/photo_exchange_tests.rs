//! Real local-stream controls. All labels/data are synthetic.
use super::{MAX_ANSWER_BYTES, photo_exchange_if as photo_exchange};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Socket {
    directory: PathBuf,
    path: PathBuf,
    listener: UnixListener,
}
impl Socket {
    fn new() -> Self {
        let directory = PathBuf::from("/tmp").join(format!(
            "lamp-px-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("call.sock");
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        Self {
            directory,
            path,
            listener,
        }
    }
    fn listener(&self) -> UnixListener {
        self.listener.try_clone().unwrap()
    }
}
impl Drop for Socket {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
fn accept(listener: UnixListener) -> UnixStream {
    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    let until = Instant::now() + Duration::from_secs(2);
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                return stream;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                let timeout = Timespec::try_from(
                    until
                        .checked_duration_since(Instant::now())
                        .expect("bounded test accept"),
                )
                .unwrap();
                poll(&mut [PollFd::new(&listener, PollFlags::IN)], Some(&timeout)).unwrap();
            }
            Err(error) => panic!("test accept: {error}"),
        }
    }
}
fn read_call(stream: &mut UnixStream) -> Value {
    let mut bytes = Vec::new();
    BufReader::new(stream)
        .take(1024)
        .read_until(b'\n', &mut bytes)
        .unwrap();
    assert_eq!(bytes.last(), Some(&b'\n'));
    serde_json::from_slice(&bytes).unwrap()
}
fn assert_closed(stream: &mut UnixStream) {
    match stream.read(&mut [0]) {
        Ok(0) => {}
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::BrokenPipe
            ) => {}
        result => panic!("permission loss must close this connection, got {result:?}"),
    }
}
fn yes() -> Value {
    json!({"photo_yes":"fixture", "face":"0123456789abcdef/f1"})
}

#[test]
fn photo_exchange_loss_after_complete_yes_write_closes_without_a_reply() {
    let socket = Socket::new();
    let listener = socket.listener();
    let permission = Arc::new(AtomicBool::new(true));
    let peer_permission = permission.clone();
    let peer = std::thread::spawn(move || {
        let mut stream = accept(listener);
        assert_eq!(read_call(&mut stream), yes());
        // The entire Yes has reached the owner, but no reply exists yet.
        peer_permission.store(false, Ordering::Release);
        stream
            .set_read_timeout(Some(Duration::from_millis(400)))
            .unwrap();
        assert_closed(&mut stream);
    });
    let began = Instant::now();
    assert!(
        photo_exchange(&socket.path, &yes(), Duration::from_secs(1), || permission
            .load(Ordering::Acquire))
        .is_none()
    );
    eprintln!(
        "photo_exchange_cancel elapsed_us={}",
        began.elapsed().as_micros()
    );
    peer.join().unwrap();
}

#[test]
fn photo_exchange_loss_during_fragmented_reply_never_returns_the_answer() {
    let socket = Socket::new();
    let listener = socket.listener();
    let permission = Arc::new(AtomicBool::new(true));
    let peer_permission = permission.clone();
    let peer = std::thread::spawn(move || {
        let mut stream = accept(listener);
        assert_eq!(read_call(&mut stream), yes());
        stream.write_all(b"{\"ok\":").unwrap();
        peer_permission.store(false, Ordering::Release);
        stream
            .set_read_timeout(Some(Duration::from_millis(400)))
            .unwrap();
        assert_closed(&mut stream);
    });
    assert!(
        photo_exchange(&socket.path, &yes(), Duration::from_secs(1), || permission
            .load(Ordering::Acquire))
        .is_none()
    );
    peer.join().unwrap();
}

#[test]
fn photo_exchange_fragmented_reply_is_complete_and_correct() {
    let socket = Socket::new();
    let listener = socket.listener();
    let peer = std::thread::spawn(move || {
        let mut stream = accept(listener);
        assert_eq!(read_call(&mut stream), yes());
        let (_hold, pause) = mpsc::channel::<()>();
        for fragment in [b"{\"ok\":".as_slice(), b"true", b",\"count\":1}\n"] {
            stream.write_all(fragment).unwrap();
            let _ = pause.recv_timeout(Duration::from_millis(10));
        }
    });
    assert_eq!(
        photo_exchange(&socket.path, &yes(), Duration::from_secs(1), || true),
        Some(json!({"ok":true,"count":1}))
    );
    peer.join().unwrap();
}

#[test]
fn photo_exchange_slow_drip_keeps_the_original_deadline() {
    let socket = Socket::new();
    let listener = socket.listener();
    let (stop, stopped) = mpsc::channel::<()>();
    let peer = std::thread::spawn(move || {
        let mut stream = accept(listener);
        assert_eq!(read_call(&mut stream), yes());
        let answer = format!("{{\"ok\":true,\"padding\":\"{}\"}}\n", "x".repeat(80));
        for byte in answer.bytes() {
            if stopped.recv_timeout(Duration::from_millis(15)).is_ok() {
                break;
            }
            if stream.write_all(&[byte]).is_err() {
                break;
            }
        }
    });
    let began = Instant::now();
    let answer = photo_exchange(&socket.path, &yes(), Duration::from_millis(100), || true);
    let elapsed = began.elapsed();
    let _ = stop.send(());
    peer.join().unwrap();
    eprintln!("photo_exchange_drip elapsed_us={}", elapsed.as_micros());
    assert!(
        answer.is_none(),
        "a drip-fed JSON line after the original deadline is not usable"
    );
    assert!(
        elapsed < Duration::from_secs(1),
        "the 100 ms exchange must not wait for the full drip-fed answer"
    );
}

#[test]
fn photo_exchange_oversized_reply_is_never_accepted() {
    let socket = Socket::new();
    let listener = socket.listener();
    let peer = std::thread::spawn(move || {
        let mut stream = accept(listener);
        assert_eq!(read_call(&mut stream), yes());
        let answer = format!(
            "{{\"padding\":\"{}\"}}\n",
            "x".repeat(MAX_ANSWER_BYTES as usize)
        );
        let _ = stream.write_all(answer.as_bytes());
    });
    assert!(photo_exchange(&socket.path, &yes(), Duration::from_secs(1), || true).is_none());
    peer.join().unwrap();
}

#[test]
fn photo_exchange_permission_loss_stops_partial_request_writes() {
    let socket = Socket::new();
    let listener = socket.listener();
    let permission = Arc::new(AtomicBool::new(true));
    let peer_permission = permission.clone();
    // Deliberately larger than a Unix socket's send buffer. Real Photo Yes is tiny.
    let line = json!({"photo_yes":"x".repeat(4 * 1024 * 1024)});
    let complete_length = serde_json::to_vec(&line).unwrap().len() + 1;
    let peer = std::thread::spawn(move || {
        let mut stream = accept(listener);
        let mut bytes = [0; 4096];
        let mut received = stream.read(&mut bytes).unwrap();
        assert!(received > 0);
        peer_permission.store(false, Ordering::Release);
        loop {
            match stream.read(&mut bytes) {
                Ok(0) => return received,
                Ok(count) => received += count,
                Err(error) => panic!("partial-write peer did not close: {error}"),
            }
        }
    });
    assert!(
        photo_exchange(&socket.path, &line, Duration::from_millis(600), || {
            permission.load(Ordering::Acquire)
        })
        .is_none()
    );
    assert!(
        peer.join().unwrap() < complete_length,
        "permission loss must stop the unfinished write"
    );
}

#[test]
fn photo_exchange_initial_denial_never_connects() {
    let socket = Socket::new();
    assert!(photo_exchange(&socket.path, &yes(), Duration::from_secs(1), || false).is_none());
    assert_eq!(
        socket.listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
}

#[test]
fn ordinary_exchange_retains_its_existing_prewrite_only_permission_contract() {
    let socket = Socket::new();
    let listener = socket.listener();
    let permission = Arc::new(AtomicBool::new(true));
    let peer_permission = permission.clone();
    let peer = std::thread::spawn(move || {
        let mut stream = accept(listener);
        assert_eq!(read_call(&mut stream), json!({"ordinary":true}));
        peer_permission.store(false, Ordering::Release);
        stream.write_all(b"{\"ordinary\":true}\n").unwrap();
    });
    assert_eq!(
        super::exchange_if(
            &socket.path,
            &json!({"ordinary":true}),
            Duration::from_secs(1),
            || permission.load(Ordering::Acquire)
        ),
        Some(json!({"ordinary":true}))
    );
    peer.join().unwrap();
}
