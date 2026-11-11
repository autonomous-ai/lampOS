//! A pseudo-terminal the real `lamp-body` opens as its servo port, with the
//! simulated bus on the other end.
use crate::servo::Bus;
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::fs::{OFlags, fcntl_setfl};
use rustix::io::Errno;
use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
use rustix::termios::{OptionalActions, tcgetattr, tcsetattr};
use std::{
    fs::File,
    io::{self, Read, Write},
    os::fd::OwnedFd,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

/// The pty's master side and the name of its slave.
pub struct Port {
    master: File,
    pub slave: PathBuf,
}

impl Port {
    pub fn open() -> io::Result<Port> {
        let master: OwnedFd =
            openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).map_err(io::Error::from)?;
        fcntl_setfl(&master, OFlags::NONBLOCK).map_err(io::Error::from)?;
        grantpt(&master).map_err(io::Error::from)?;
        unlockpt(&master).map_err(io::Error::from)?;
        let name = ptsname(&master, Vec::new()).map_err(io::Error::from)?;
        // Raw on this side too, so nothing is echoed back to the body.
        let mut modes = tcgetattr(&master).map_err(io::Error::from)?;
        modes.make_raw();
        tcsetattr(&master, OptionalActions::Now, &modes).map_err(io::Error::from)?;
        Ok(Port {
            master: File::from(master),
            slave: PathBuf::from(name.to_string_lossy().into_owned()),
        })
    }

    /// Serves `bus` on the port until `stop` says so: what the body writes is
    /// fed to the bus, its answers written back, and time passes by the clock.
    pub fn serve(&mut self, bus: &mut Bus, link: &Path, stop: impl Fn() -> bool) -> io::Result<()> {
        let mut last = Instant::now();
        // Whether a body has had the slave open since the port was made.
        let mut opened = false;
        let mut buffer = [0u8; 512];
        while !stop() {
            match self.master.read(&mut buffer) {
                Ok(n) if n > 0 => {
                    let reply = bus.feed(&buffer[..n]);
                    if !reply.is_empty() {
                        write_all(&mut self.master, &reply)?;
                    }
                }
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                // The slave side is closed until the body opens it.
                Err(e) if e.raw_os_error() == Some(Errno::IO.raw_os_error()) => {}
                Err(e) => return Err(e),
            }
            let now = Instant::now();
            bus.step(now.duration_since(last).as_secs_f64());
            last = now;
            // Wake the moment the body writes (a reply must not wait for a
            // timer: under load the body's deadline is missed), else after 2 ms
            // to move the servos. Closed on the other side, nothing wakes it:
            // look again after 5 ms.
            let mut fds = [PollFd::new(&self.master, PollFlags::IN)];
            let waited = poll(
                &mut fds,
                Some(&Timespec {
                    tv_sec: 0,
                    tv_nsec: 2_000_000,
                }),
            );
            let hung_up = fds[0].revents().contains(PollFlags::HUP);
            if waited.is_ok() && !hung_up {
                opened = true;
            }
            if hung_up {
                if opened {
                    // The body let go of the port (it ended). Its exclusive
                    // open stays on this pty, so the next body's open would
                    // fail with EBUSY: serve a fresh pty under the same link.
                    *self = Port::open()?;
                    relink(&self.slave, link)?;
                    opened = false;
                }
                std::thread::sleep(Duration::from_millis(5));
            } else if waited.is_err() {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        Ok(())
    }
}

fn write_all(file: &mut File, mut bytes: &[u8]) -> io::Result<()> {
    while !bytes.is_empty() {
        match file.write(bytes) {
            Ok(n) => bytes = &bytes[n..],
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_micros(200));
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Points `link` at `slave`, in one step.
pub fn relink(slave: &Path, link: &Path) -> io::Result<()> {
    let next = link.with_extension("new");
    let _ = std::fs::remove_file(&next);
    std::os::unix::fs::symlink(slave, &next)?;
    std::fs::rename(&next, link)
}
