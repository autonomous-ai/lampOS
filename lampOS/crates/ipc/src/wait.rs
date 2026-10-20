//! Waiting on a socket without a sleep that a caller has to wait out.

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use std::{io, os::fd::AsFd, time::Duration};

/// Wait until `fd` has something to read, or `timeout` has passed, whichever
/// comes first. True when it has: on a listening socket, a caller waiting to
/// be accepted. A signal that cuts the wait short is false, like the
/// timeout, for the caller to look again on its next round. An error or
/// hang-up on `fd` with nothing to read is an error, never true: a caller
/// that went straight back to waiting would otherwise spin (the reviewer on
/// #1888).
pub fn readable(fd: impl AsFd, timeout: Duration) -> io::Result<bool> {
    let timeout = Timespec::try_from(timeout)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "wait out of range"))?;
    let mut fds = [PollFd::new(&fd, PollFlags::IN)];
    match poll(&mut fds, Some(&timeout)) {
        Ok(0) | Err(rustix::io::Errno::INTR) => Ok(false),
        Ok(_) => {
            let flags = fds[0].revents();
            if flags.contains(PollFlags::IN) {
                Ok(true)
            } else {
                Err(io::Error::other(format!("not readable: {flags:?}")))
            }
        }
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::readable;
    use std::{
        io::Write,
        os::unix::net::{UnixListener, UnixStream},
        time::{Duration, Instant},
    };

    #[test]
    fn a_waiting_caller_ends_the_wait_and_nobody_lasts_the_timeout() {
        let directory = std::env::temp_dir().join(format!("lamp-ipc-wait-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("call.sock");
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();

        let started = Instant::now();
        assert!(!readable(&listener, Duration::from_millis(30)).unwrap());
        assert!(started.elapsed() >= Duration::from_millis(30));

        let caller = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            let mut stream = UnixStream::connect(path).unwrap();
            stream.write_all(b"hello\n").unwrap();
        });
        let started = Instant::now();
        assert!(readable(&listener, Duration::from_secs(10)).unwrap());
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
        assert!(listener.accept().is_ok());
        caller.join().unwrap();
        std::fs::remove_dir_all(directory).unwrap();
    }

    /// Linux says only an error (`POLLERR`) on the write end of a pipe its
    /// reader has left: an error, not "readable", so a caller that goes
    /// straight back to waiting cannot spin on it. macOS says it with
    /// `POLLIN` as well, so this is Linux's case.
    #[cfg(target_os = "linux")]
    #[test]
    fn an_error_with_nothing_to_read_is_an_error() {
        let (read, write) = rustix::pipe::pipe().unwrap();
        drop(read);
        let started = Instant::now();
        assert!(readable(&write, Duration::from_secs(5)).is_err());
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
