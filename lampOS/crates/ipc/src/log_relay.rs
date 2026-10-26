//! A lampOS process's log lines never block it and never end it.
//!
//! Every lampOS part writes its log to stderr, which systemd connects to the
//! journal. A write there blocks when the journal does not keep up, and
//! fails when its stream breaks, and `eprintln!` panics on a failed write.
//! On lamp-4ace on 2026-10-15 the failure ended the ring worker (journald
//! restarted at 01:52, #744), and the block is the likeliest reason the
//! capture worker overran its read budget at 09:00:01, when the RAM log was
//! full and journald could not write. So each lampOS process puts a pipe of
//! its own on stderr when it starts ([`install`], or [`guard`] for one that
//! also waits for its last lines when `main` returns). One thread drains that
//! pipe at once and copies what it reads to the real stderr without ever
//! waiting on it: what the journal will not take is dropped, counted, and
//! the count said once the journal takes lines again. A writer only ever
//! writes into a pipe that is being emptied.
//!
//! The real stderr is never made non-blocking: `O_NONBLOCK` belongs to the
//! open file, which every process holding it shares. A worker's stderr is
//! its parent's relay pipe; a worker's relay that set it there made the
//! parent's own stderr non-blocking, and the parent's next `eprintln!` into
//! a full pipe would have panicked (the eighth on #1178). The relay instead
//! writes in a way of its own that never waits ([`Journal`]): on Linux it
//! sends to a socket (the journal's stream) with `MSG_DONTWAIT`, and writes
//! to a pipe (a worker's stderr) through a file of its own on that pipe,
//! opened non-blocking from `/proc`; to anything else only when `poll` says
//! there is room.
#[cfg(target_os = "linux")]
use rustix::fd::AsRawFd;
use rustix::{
    event::{PollFd, PollFlags, Timespec},
    fd::{AsFd, OwnedFd},
    io::Errno,
};
use std::{
    io,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

/// What [`finish`] needs: the pipe's reading end, to see what is still in
/// it, and how much the relay holds.
static RELAY: OnceLock<(OwnedFd, Arc<AtomicUsize>)> = OnceLock::new();

/// Kept for the journal while it does not take lines: at most this much.
pub const MOST_HELD: usize = 64 * 1024;
/// A line longer than this goes out in pieces rather than waiting for its
/// end.
const LONGEST_LINE: usize = 4096;
/// What the relay holds while it is between reading and writing.
const BUSY: usize = usize::MAX;
/// How often held lines are offered again while nothing new arrives.
const RETRY: Timespec = Timespec {
    tv_sec: 0,
    tv_nsec: 100_000_000,
};

/// Put this process's stderr through a relay. Lines the journal would not
/// take are counted in the value returned. An error leaves stderr as it was.
pub fn install() -> io::Result<Arc<AtomicU64>> {
    let real = rustix::io::fcntl_dupfd_cloexec(io::stderr().as_fd(), 3)?;
    let (read, write) = rustix::pipe::pipe()?;
    rustix::io::fcntl_setfd(&read, rustix::io::FdFlags::CLOEXEC)?;
    let dropped = Arc::new(AtomicU64::new(0));
    let counted = dropped.clone();
    let held = Arc::new(AtomicUsize::new(0));
    let holding = held.clone();
    let watched = rustix::io::fcntl_dupfd_cloexec(&read, 3)?;
    // A signal is never the relay thread's. A part that blocks SIGTERM in
    // `main` to read it from a signalfd blocks it only there and in threads
    // made after; this one is made first, so the kernel gave it SIGTERM and
    // its default ended the process: on lamp-52e6 on 2026-10-15 a standing
    // body stopped 45 ms after "Stopping", without folding. Made with every
    // signal blocked, it inherits that mask from its first instruction.
    use nix::sys::signal::SigSet;
    let before = SigSet::thread_get_mask().map_err(io::Error::from)?;
    SigSet::all().thread_block().map_err(io::Error::from)?;
    let spawned = std::thread::Builder::new()
        .name("lamp-log-relay".into())
        .spawn(move || relay_holding(read, real, &counted, &holding));
    before.thread_set_mask().map_err(io::Error::from)?;
    spawned?;
    rustix::stdio::dup2_stderr(&write)?;
    let _ = RELAY.set((watched, held));
    // A panic's message is a process's last words: give the relay the
    // moment it needs to pass them on.
    let before = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        before(info);
        finish(Duration::from_millis(300));
    }));
    Ok(dropped)
}

/// This process's name, for the line that says what was dropped.
fn program() -> String {
    std::env::args_os()
        .next()
        .and_then(|path| {
            std::path::Path::new(&path)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "lamp".into())
}

/// Held for as long as `main` runs: the relay is installed when it is made,
/// and when it goes the last lines are given half a second to reach the
/// journal. `let _log = lamp_ipc::log_relay::guard();` as the first line of
/// a part's `main`.
pub struct Guard {
    installed: bool,
}

impl Drop for Guard {
    fn drop(&mut self) {
        if self.installed {
            finish(Duration::from_millis(500));
        }
    }
}

/// [`install`], held as a [`Guard`]. A relay that cannot be made leaves
/// stderr as it was, and says so there.
pub fn guard() -> Guard {
    let installed = install()
        .map_err(|error| {
            eprintln!(
                "{}: log lines go straight to the journal: {error}",
                program()
            )
        })
        .is_ok();
    Guard { installed }
}

/// Wait, at most `longest`, until every line written so far has gone to
/// the journal: before a process ends, so that its last lines are not lost.
pub fn finish(longest: Duration) {
    let Some((read, held)) = RELAY.get() else {
        return;
    };
    finish_on(read, held, longest);
}

/// [`finish`] for the relay reading `read` and saying what it holds in
/// `held`: true once both are empty, false when `longest` has gone first.
fn finish_on(read: &OwnedFd, held: &AtomicUsize, longest: Duration) -> bool {
    let until = Instant::now() + longest;
    while Instant::now() < until {
        // The pipe first, then the relay: bytes gone from the pipe were
        // taken by a read made after the relay said it was busy (#837).
        let waiting = rustix::io::ioctl_fionread(read).unwrap_or(0);
        if waiting == 0 && held.load(Ordering::SeqCst) == 0 {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

/// Where a test may stop the relay (#837): just after a read took bytes out
/// of the pipe, and just after a whole line went to the journal. A running
/// relay passes both without stopping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Seam {
    Read,
    Wrote,
}

/// Drain `from` until every writer has closed it, and pass what it holds to
/// `to`: never wait on `to` ([`write_now`]), never fail.
pub fn relay(from: OwnedFd, to: OwnedFd, dropped: &AtomicU64) {
    relay_holding(from, to, dropped, &AtomicUsize::new(0));
}

fn relay_holding(from: OwnedFd, to: OwnedFd, dropped: &AtomicU64, holding: &AtomicUsize) {
    relay_seamed(from, to, dropped, holding, &mut |_| {});
}

fn relay_seamed(
    from: OwnedFd,
    to: OwnedFd,
    dropped: &AtomicU64,
    holding: &AtomicUsize,
    seam: &mut dyn FnMut(Seam),
) {
    let to = Journal::new(to);
    let mut held: Vec<u8> = Vec::new();
    // How much of the first held line has gone already: a line once begun
    // is finished, never cut, so the journal never gets half of one.
    let mut begun = 0usize;
    let mut buffer = [0u8; 8192];
    let mut told = 0u64;
    let lines = |bytes: &[u8]| bytes.iter().filter(|byte| **byte == b'\n').count().max(1) as u64;
    loop {
        let mut fds = [PollFd::new(&from, PollFlags::IN)];
        let ready = matches!(rustix::event::poll(&mut fds, Some(&RETRY)), Ok(n) if n > 0);
        let mut ended = false;
        if ready {
            // Busy until what is read is written or counted as held: a
            // pipe found empty is not yet a line delivered (#837).
            holding.store(BUSY, Ordering::SeqCst);
            match rustix::io::read(&from, &mut buffer) {
                // Every writer has gone: say what is held, if the journal
                // takes it, and stop.
                Ok(0) => ended = true,
                Ok(n) => {
                    held.extend_from_slice(&buffer[..n]);
                    seam(Seam::Read);
                }
                Err(Errno::INTR | Errno::AGAIN) => {}
                Err(_) => {
                    holding.store(0, Ordering::SeqCst);
                    return;
                }
            }
        }
        // Too much held: whole lines go, oldest first, but never the one
        // being written.
        if held.len() > MOST_HELD {
            let keep_from = if begun > 0 {
                held.iter()
                    .position(|byte| *byte == b'\n')
                    .map_or(held.len(), |at| at + 1)
            } else {
                0
            };
            let over = held.len() - MOST_HELD;
            let cut = held[keep_from + over.min(held.len() - keep_from)..]
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(held.len(), |at| {
                    keep_from + over.min(held.len() - keep_from) + at + 1
                });
            if cut > keep_from {
                dropped.fetch_add(lines(&held[keep_from..cut]), Ordering::Relaxed);
                held.drain(keep_from..cut);
            }
        }
        // Line by line, as much as the journal takes now and no more.
        while !held.is_empty() {
            // Only a whole line goes: the rest of one may still be in the
            // pipe. A very long one, or the last when every writer has
            // gone, goes as it is.
            let end = match held.iter().position(|byte| *byte == b'\n') {
                Some(at) => at + 1,
                None if ended || held.len() >= LONGEST_LINE => held.len(),
                None => break,
            };
            match write_now(&to, &held[begun..end]) {
                Ok(0) => break,
                Ok(n) => {
                    begun += n;
                    if begun == end {
                        held.drain(..end);
                        begun = 0;
                        seam(Seam::Wrote);
                    }
                }
                Err(Errno::INTR) => {}
                Err(Errno::AGAIN) => break,
                // A broken stream: what is held cannot go anywhere.
                Err(_) => {
                    dropped.fetch_add(lines(&held), Ordering::Relaxed);
                    held.clear();
                    begun = 0;
                }
            }
        }
        let now = dropped.load(Ordering::Relaxed);
        if held.is_empty() && begun == 0 && now != told {
            let line = format!(
                "{}: {} log lines dropped because the journal did not take them\n",
                program(),
                now - told
            );
            match write_now(&to, line.as_bytes()) {
                Ok(0) | Err(_) => {}
                // Told. Part of it taken: the rest is held, as a line begun.
                Ok(n) => {
                    told = now;
                    if n < line.len() {
                        held.extend_from_slice(line.as_bytes());
                        begun = n;
                    }
                }
            }
        }
        holding.store(held.len(), Ordering::SeqCst);
        if ended {
            return;
        }
    }
}

/// The real stderr, as the relay writes it: never waiting on it, and
/// never making it non-blocking for anyone else (see the module's doc).
struct Journal {
    fd: OwnedFd,
    way: Way,
}

enum Way {
    /// Linux: a socket, sent to with `MSG_DONTWAIT`, which Linux keeps
    /// whatever the socket's room, so all of it can be used.
    #[cfg(target_os = "linux")]
    Sent,
    /// Linux: a pipe, through a file of the relay's own on it, opened
    /// non-blocking: its flag is no one else's.
    #[cfg(target_os = "linux")]
    Own,
    /// Anything else: only when `poll` says there is room, and at most
    /// `PIPE_BUF` at a time, which a pipe takes whole or not at all and a
    /// socket below its low-water mark; a socket also with `MSG_DONTWAIT`.
    /// Another writer can still take a pipe's room between the two calls,
    /// and the write then waits until the pipe is read.
    Polled { socket: bool },
}

impl Journal {
    fn new(fd: OwnedFd) -> Self {
        let socket = rustix::net::sockopt::socket_type(&fd).is_ok();
        #[cfg(target_os = "linux")]
        {
            if socket {
                return Journal { fd, way: Way::Sent };
            }
            if let Some(own) = reopened(&fd) {
                return Journal {
                    fd: own,
                    way: Way::Own,
                };
            }
        }
        Journal {
            fd,
            way: Way::Polled { socket },
        }
    }
}

/// A file of this process's own on the pipe `fd` is, opened write-only and
/// non-blocking through `/proc`. Only a pipe: a regular file opened so would
/// be written from its start. None where that cannot be done.
#[cfg(target_os = "linux")]
fn reopened(fd: &OwnedFd) -> Option<OwnedFd> {
    use std::os::unix::fs::{FileTypeExt, OpenOptionsExt};
    let path = format!("/proc/self/fd/{}", fd.as_raw_fd());
    if !std::fs::metadata(&path).ok()?.file_type().is_fifo() {
        return None;
    }
    let own = std::fs::OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(&path)
        .ok()?;
    Some(own.into())
}

/// Write what `to` takes now ([`Journal`]): `AGAIN` when it has no room.
fn write_now(to: &Journal, bytes: &[u8]) -> Result<usize, Errno> {
    let socket = match to.way {
        #[cfg(target_os = "linux")]
        Way::Sent => return rustix::net::send(&to.fd, bytes, rustix::net::SendFlags::DONTWAIT),
        #[cfg(target_os = "linux")]
        Way::Own => return rustix::io::write(&to.fd, bytes),
        Way::Polled { socket } => socket,
    };
    let mut fds = [PollFd::new(&to.fd, PollFlags::OUT)];
    let now = Timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    match rustix::event::poll(&mut fds, Some(&now)) {
        // Room, or a broken stream whose write says why.
        Ok(n) if n > 0 => {}
        Ok(_) => return Err(Errno::AGAIN),
        Err(error) => return Err(error),
    }
    let piece = &bytes[..bytes.len().min(rustix::pipe::PIPE_BUF)];
    if socket {
        rustix::net::send(&to.fd, piece, rustix::net::SendFlags::DONTWAIT)
    } else {
        rustix::io::write(&to.fd, piece)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// Whether an ordinary 1 MiB write to `to`, full, waits, as on a
    /// blocking file, rather than returning at once, as on a non-blocking
    /// one; reading `other_end` (the test's own, a file of its own) then
    /// empties it. More than either buffer holds: a socket the relay polls
    /// (macOS) is full for `poll` before its buffer is.
    fn waits_when_full(to: &OwnedFd, other_end: &OwnedFd) -> bool {
        const SIZE: usize = 1 << 20;
        rustix::io::ioctl_fionbio(other_end, true).unwrap();
        let writer = rustix::io::dup(to).unwrap();
        let blocked = std::thread::spawn(move || {
            let bytes = vec![b'z'; SIZE];
            rustix::io::write(&writer, &bytes)
        });
        std::thread::sleep(Duration::from_millis(100));
        let waited = !blocked.is_finished();
        let mut buffer = vec![0u8; 65_536];
        let started = Instant::now();
        while !blocked.is_finished() {
            let _ = rustix::io::read(other_end, &mut buffer);
            assert!(started.elapsed() < Duration::from_secs(5), "never drained");
        }
        while rustix::io::read(other_end, &mut buffer).is_ok_and(|n| n > 0) {}
        waited && blocked.join().unwrap() == Ok(SIZE)
    }

    /// Fills `to` with [`write_now`] until it takes no more.
    fn filled(to: &Journal) -> usize {
        let mut written = 0;
        let started = Instant::now();
        loop {
            match write_now(to, &[b'x'; 4096]) {
                Ok(n) => written += n,
                Err(Errno::AGAIN) => return written,
                Err(error) => panic!("{error}"),
            }
            assert!(started.elapsed() < Duration::from_secs(5), "never full");
        }
    }

    /// The eighth on #1178: a worker's stderr is its parent's relay pipe, and
    /// `O_NONBLOCK` set there was set on the parent's stderr too. A blocking
    /// pipe and a blocking socket, full, are never waited on, and never made
    /// non-blocking, whichever way the relay writes them.
    #[test]
    fn a_full_blocking_pipe_or_socket_is_never_waited_on_nor_made_nonblocking() {
        const CHILD: &str = "LAMP_IPC_FULL_JOURNAL_TEST_CHILD";
        const CASE: &str = "log_relay::tests::a_full_blocking_pipe_or_socket_is_never_waited_on_nor_made_nonblocking";
        if std::env::var_os(CHILD).as_deref() != Some(std::ffi::OsStr::new("1")) {
            // Nothing reads the full pipe/socket until write_now returns.
            // A regression therefore blocks for real. Isolate it so the
            // diagnostic watchdog can stop and reap it without a stuck thread.
            struct Probe(std::process::Child);
            impl Drop for Probe {
                fn drop(&mut self) {
                    if !matches!(self.0.try_wait(), Ok(Some(_))) {
                        let _ = self.0.kill();
                    }
                    let _ = self.0.wait();
                }
            }
            let mut probe = Probe(
                std::process::Command::new(std::env::current_exe().unwrap())
                    .args([CASE, "--exact", "--test-threads=1", "--color=never"])
                    .env(CHILD, "1")
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::piped())
                    .spawn()
                    .unwrap(),
            );
            let until = Instant::now() + Duration::from_secs(15);
            let status = loop {
                if let Some(status) = probe.0.try_wait().unwrap() {
                    break status;
                }
                assert!(
                    Instant::now() < until,
                    "journal write did not return while its pipe/socket stayed full"
                );
                std::thread::sleep(Duration::from_millis(5));
            };
            use std::io::Read;
            let mut output = String::new();
            probe
                .0
                .stdout
                .take()
                .unwrap()
                .take(65_537)
                .read_to_string(&mut output)
                .unwrap();
            assert!(output.len() <= 65_536, "unexpected child output volume");
            assert!(status.success(), "full-journal child failed: {output}");
            // Exit zero alone would also accept a misspelled, unselected test.
            assert!(output.contains(&format!("test {CASE} ... ok")), "{output}");
            assert!(output.contains("1 passed; 0 failed"), "{output}");
            return;
        }
        let (read, pipe) = rustix::pipe::pipe().unwrap();
        let own = Journal::new(rustix::io::dup(&pipe).unwrap());
        #[cfg(target_os = "linux")]
        assert!(matches!(own.way, Way::Own), "a pipe of its own from /proc");
        let polled = Journal {
            fd: rustix::io::dup(&pipe).unwrap(),
            way: Way::Polled { socket: false },
        };
        for journal in [own, polled] {
            assert!(filled(&journal) > 0);
            assert_eq!(write_now(&journal, b"one more line\n"), Err(Errno::AGAIN));
            // The pipe stays as its owner made it: a parent's `eprintln!`
            // into it waits for room, never fails.
            assert!(waits_when_full(&pipe, &read));
        }
        let (socket, peer) = rustix::net::socketpair(
            rustix::net::AddressFamily::UNIX,
            rustix::net::SocketType::STREAM,
            rustix::net::SocketFlags::empty(),
            None,
        )
        .unwrap();
        let journal = Journal::new(rustix::io::dup(&socket).unwrap());
        #[cfg(target_os = "linux")]
        assert!(matches!(journal.way, Way::Sent), "sent to");
        assert!(filled(&journal) > 0);
        assert_eq!(write_now(&journal, b"one more line\n"), Err(Errno::AGAIN));
        assert!(waits_when_full(&socket, &peer));
    }

    /// A relay whose stderr is a full blocking pipe, as a worker's is when
    /// its parent's relay falls behind, still reads its own pipe and ends
    /// when every writer has gone: what it could not pass on is counted.
    #[test]
    fn a_relay_into_a_full_blocking_pipe_goes_on_and_counts_what_it_drops() {
        let (from, into) = rustix::pipe::pipe().unwrap();
        let (_parent_read, parent) = rustix::pipe::pipe().unwrap();
        filled(&Journal::new(rustix::io::dup(&parent).unwrap()));
        let dropped = Arc::new(AtomicU64::new(0));
        let counted = dropped.clone();
        let relay = std::thread::spawn(move || super::relay(from, parent, &counted));
        for _ in 0..40 {
            rustix::io::write(&into, &[b'y'; 4095]).unwrap();
            rustix::io::write(&into, b"\n").unwrap();
        }
        drop(into);
        let started = Instant::now();
        while !relay.is_finished() {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "the relay waited on a full pipe"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        relay.join().unwrap();
        assert!(dropped.load(Ordering::Relaxed) > 0);
    }

    fn nonblocking_pipe() -> (OwnedFd, OwnedFd) {
        let (read, write) = rustix::pipe::pipe().unwrap();
        rustix::io::ioctl_fionbio(&write, true).unwrap();
        (read, write)
    }

    /// A part's last words: its error and its usage, in one write.
    const SAID: &[u8] = b"lamp-test: unknown option --x\nusage: lamp-test --dir DIR [--y N]\n";

    /// #837: the relay is stopped at `at` once, with the writer still open.
    /// The pipe is then empty, yet `finish` must not say done while the relay
    /// holds the words; once it goes on, the journal gets every byte.
    fn finish_waits_while_stopped_at(at: Seam) {
        let (journal_read, journal_write) = nonblocking_pipe();
        let (from, into) = rustix::pipe::pipe().unwrap();
        let watched = rustix::io::dup(&from).unwrap();
        let holding = Arc::new(AtomicUsize::new(0));
        let (stopped_tx, stopped) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel::<()>();
        let held = holding.clone();
        let relay = std::thread::spawn(move || {
            let mut once = true;
            relay_seamed(
                from,
                journal_write,
                &AtomicU64::new(0),
                &held,
                &mut |seam| {
                    if seam == at && std::mem::take(&mut once) {
                        stopped_tx.send(()).unwrap();
                        released.recv().unwrap();
                    }
                },
            );
        });
        assert_eq!(rustix::io::write(&into, SAID).unwrap(), SAID.len());
        stopped.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(rustix::io::ioctl_fionread(&watched).unwrap(), 0, "all read");
        assert!(
            !finish_on(&watched, &holding, Duration::from_millis(50)),
            "{at:?}: done while the relay still holds the usage line"
        );
        release.send(()).unwrap();
        assert!(finish_on(&watched, &holding, Duration::from_millis(500)));
        let mut got = [0u8; 256];
        let n = rustix::io::read(&journal_read, &mut got).unwrap();
        assert_eq!(&got[..n], SAID);
        drop(into);
        relay.join().unwrap();
    }

    #[test]
    fn finish_waits_for_words_read_from_the_pipe_and_not_yet_written() {
        finish_waits_while_stopped_at(Seam::Read);
    }

    #[test]
    fn finish_waits_for_the_usage_line_held_after_the_error_line_went() {
        finish_waits_while_stopped_at(Seam::Wrote);
    }

    /// The bounds stay: a journal that takes nothing holds `finish` no
    /// longer than it is given, and a broken one does not hold it at all.
    #[test]
    fn finish_is_bounded_by_a_full_journal_and_freed_by_a_broken_one() {
        for broken in [false, true] {
            let (journal_read, journal_write) = nonblocking_pipe();
            // Fill the journal so nothing more goes in.
            while rustix::io::write(&journal_write, &[b'x'; 4096]).is_ok() {}
            if broken {
                drop(journal_read);
            } else {
                std::mem::forget(journal_read);
            }
            let (from, into) = rustix::pipe::pipe().unwrap();
            let watched = rustix::io::dup(&from).unwrap();
            let holding = Arc::new(AtomicUsize::new(0));
            let held = holding.clone();
            let relay = std::thread::spawn(move || {
                relay_holding(from, journal_write, &AtomicU64::new(0), &held)
            });
            rustix::io::write(&into, SAID).unwrap();
            let began = Instant::now();
            let done = finish_on(&watched, &holding, Duration::from_millis(200));
            let took = began.elapsed();
            assert_eq!(done, broken, "broken: {broken}");
            assert!(took < Duration::from_millis(400), "{took:?}");
            drop(into);
            relay.join().unwrap();
        }
    }

    #[test]
    fn a_journal_that_takes_nothing_never_holds_a_writer_and_the_loss_is_counted_then_said() {
        // The journal: a pipe nobody reads until the end.
        let (journal_read, journal_write) = nonblocking_pipe();
        let (from, into) = rustix::pipe::pipe().unwrap();
        let dropped = Arc::new(AtomicU64::new(0));
        let counted = dropped.clone();
        let relay = std::thread::spawn(move || relay(from, journal_write, &counted));
        // Far more than the journal's pipe and the relay together hold.
        let line = [b'x'; 99]
            .iter()
            .copied()
            .chain([b'\n'])
            .collect::<Vec<u8>>();
        let started = Instant::now();
        for _ in 0..20_000 {
            let mut left = &line[..];
            while !left.is_empty() {
                let n = rustix::io::write(&into, left).unwrap();
                left = &left[n..];
            }
        }
        // Two megabytes went in without the writer waiting on the journal.
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
        assert!(dropped.load(Ordering::Relaxed) > 0);
        // The journal reads again while the writer is still there: what is
        // held goes, then the count is said.
        let mut said = Vec::new();
        let mut buffer = [0u8; 65536];
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline
            && !String::from_utf8_lossy(&said).contains("log lines dropped")
        {
            match rustix::io::read(&journal_read, &mut buffer) {
                Ok(n) if n > 0 => said.extend_from_slice(&buffer[..n]),
                _ => std::thread::sleep(Duration::from_millis(5)),
            }
        }
        // The last writer goes, and the relay with it.
        drop(into);
        relay.join().unwrap();
        let said = String::from_utf8_lossy(&said);
        assert!(
            said.contains("log lines dropped because the journal did not take them"),
            "{}",
            &said[said.len().saturating_sub(300)..]
        );
        // Whole lines only: every line kept is a whole one.
        let broken: Vec<&str> = said
            .lines()
            .filter(|text| {
                text.len() != 99
                    && !text.ends_with("log lines dropped because the journal did not take them")
            })
            .collect();
        assert!(broken.is_empty(), "{} broken: {:?}", broken.len(), broken);
    }

    #[test]
    fn a_broken_journal_stream_ends_nothing() {
        let (journal_read, journal_write) = nonblocking_pipe();
        drop(journal_read);
        let (from, into) = rustix::pipe::pipe().unwrap();
        let dropped = Arc::new(AtomicU64::new(0));
        let counted = dropped.clone();
        let relay = std::thread::spawn(move || relay(from, journal_write, &counted));
        for _ in 0..100 {
            rustix::io::write(&into, b"a line the journal will never see\n").unwrap();
        }
        drop(into);
        relay.join().unwrap();
        assert!(dropped.load(Ordering::Relaxed) >= 1);
    }
}
