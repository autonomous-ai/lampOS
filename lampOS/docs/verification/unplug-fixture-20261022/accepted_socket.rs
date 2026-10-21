//! Private Darwin control for the unplug fixture's accepted Unix socket.
//! This uses only the standard library and does not start lamp-skills.

#[cfg(not(target_os = "macos"))]
compile_error!("This control observes Darwin's inherited O_NONBLOCK; run it on macOS.");

use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// Darwin <fcntl.h>: F_GETFL = 3 and O_NONBLOCK = 0x0004.
unsafe extern "C" {
    fn fcntl(fd: std::ffi::c_int, command: std::ffi::c_int, ...) -> std::ffi::c_int;
}

fn flags(stream: &UnixStream) -> i32 {
    // SAFETY: F_GETFL takes only this valid owned descriptor and no third argument.
    let value = unsafe { fcntl(stream.as_raw_fd(), 3) };
    assert!(value >= 0, "F_GETFL: {}", std::io::Error::last_os_error());
    value
}

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn main() {
    let wait = Duration::from_secs(2);
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let scratch = Scratch(PathBuf::from(format!(
        "/tmp/second-unplug-control-{}-{nonce}",
        std::process::id()
    )));
    std::fs::create_dir(&scratch.0).unwrap();
    let address = scratch.0.join("say.sock");
    let listener = UnixListener::bind(&address).unwrap();
    listener.set_nonblocking(true).unwrap();

    // The client has connected but cannot write until the release below.
    let mut client = UnixStream::connect(&address).unwrap();
    client.set_write_timeout(Some(wait)).unwrap();
    let (connection, _) = listener.accept().unwrap();
    let inherited = flags(&connection);
    assert_ne!(inherited & 4, 0, "accepted socket did not inherit O_NONBLOCK");
    connection.set_read_timeout(Some(wait)).unwrap();
    let mut premature = String::new();
    let before = Instant::now();
    let error = BufReader::new(&connection).read_line(&mut premature).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::WouldBlock);
    assert!(premature.is_empty());
    println!(
        "{{\"case\":\"without_normalization\",\"accepted_flags\":{inherited},\"nonblocking\":true,\"kind\":\"WouldBlock\",\"raw_os_error\":{},\"elapsed_us\":{}}}",
        error.raw_os_error().unwrap_or_default(),
        before.elapsed().as_micros()
    );

    // This is the fixture fix; its existing two-second read bound remains.
    connection.set_nonblocking(false).unwrap();
    let normalized = flags(&connection);
    assert_eq!(normalized & 4, 0, "accepted socket is still nonblocking");
    connection.set_read_timeout(Some(wait)).unwrap();

    let (release, released) = mpsc::sync_channel::<()>(0);
    let writer = std::thread::spawn(move || {
        released.recv_timeout(wait).unwrap();
        client.write_all(b"ready after render receipt\n").unwrap();
    });
    let (starting, started) = mpsc::sync_channel::<()>(0);
    let (finished, result) = mpsc::sync_channel(1);
    let reader = std::thread::spawn(move || {
        starting.send(()).unwrap();
        let before = Instant::now();
        let mut line = String::new();
        let read = BufReader::new(connection).read_line(&mut line);
        finished.send((read, line, before.elapsed())).unwrap();
    });
    started.recv_timeout(wait).unwrap();
    assert!(matches!(result.try_recv(), Err(mpsc::TryRecvError::Empty)));
    release.send(()).unwrap();
    let (read, line, elapsed) = result.recv_timeout(wait).unwrap();
    assert_eq!(read.unwrap(), b"ready after render receipt\n".len());
    assert_eq!(line, "ready after render receipt\n");
    writer.join().unwrap();
    reader.join().unwrap();
    println!(
        "{{\"case\":\"with_normalization\",\"accepted_flags\":{normalized},\"nonblocking\":false,\"writer_released_after_read_announcement\":true,\"complete_line\":true,\"elapsed_us\":{}}}",
        elapsed.as_micros()
    );
}
