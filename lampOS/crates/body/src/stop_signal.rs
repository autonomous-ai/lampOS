//! SIGTERM and SIGINT as a polled "stop" request, without `unsafe`.
//!
//! The caller blocks both signals in its thread and asks `requested` once a
//! step. On Linux this is the signalfd the body has always used. Elsewhere
//! (macOS has no signalfd) one helper thread waits for the blocked signals
//! with `sigwait` and raises a flag. Dropping it ends that thread (it is woken
//! with SIGUSR1, sent to that thread alone, which it also waits for, and
//! joined) and puts the old mask back, so a later SIGTERM is not taken by a
//! thread nobody reads.
//!
//! Build it before any other thread is started: threads inherit the blocked
//! mask, and a thread started earlier would take the default action.
use nix::sys::signal::{SigSet, Signal};
use std::io;

#[cfg(target_os = "linux")]
use nix::sys::signalfd::{SfdFlags, SignalFd};
#[cfg(not(target_os = "linux"))]
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
};

pub struct StopSignal {
    old: SigSet,
    #[cfg(target_os = "linux")]
    fd: SignalFd,
    #[cfg(not(target_os = "linux"))]
    raised: Arc<AtomicBool>,
    #[cfg(not(target_os = "linux"))]
    waiter: Mutex<Option<JoinHandle<()>>>,
}

impl StopSignal {
    pub fn new() -> io::Result<Self> {
        let old = SigSet::thread_get_mask()?;
        let mut mask = SigSet::empty();
        mask.add(Signal::SIGTERM);
        mask.add(Signal::SIGINT);
        // Off Linux the helper thread is woken to end with SIGUSR1.
        #[cfg(not(target_os = "linux"))]
        mask.add(Signal::SIGUSR1);
        mask.thread_block()?;
        match Self::open(&mask) {
            Ok(open) => Ok(Self {
                old,
                #[cfg(target_os = "linux")]
                fd: open,
                #[cfg(not(target_os = "linux"))]
                raised: open.0,
                #[cfg(not(target_os = "linux"))]
                waiter: Mutex::new(Some(open.1)),
            }),
            Err(error) => {
                let _ = old.thread_set_mask();
                Err(error)
            }
        }
    }

    #[cfg(target_os = "linux")]
    fn open(mask: &SigSet) -> io::Result<SignalFd> {
        Ok(SignalFd::with_flags(
            mask,
            SfdFlags::SFD_CLOEXEC | SfdFlags::SFD_NONBLOCK,
        )?)
    }

    #[cfg(not(target_os = "linux"))]
    fn open(mask: &SigSet) -> io::Result<(Arc<AtomicBool>, JoinHandle<()>)> {
        let raised = Arc::new(AtomicBool::new(false));
        let waiter = Arc::clone(&raised);
        let mask = *mask;
        let thread = std::thread::Builder::new()
            .name("stop-signal".into())
            .spawn(move || {
                // The signals are blocked here too (inherited), so they are
                // taken by `sigwait`, never by their default action.
                loop {
                    match mask.wait() {
                        Ok(Signal::SIGUSR1) | Err(_) => break,
                        Ok(_) => waiter.store(true, Ordering::SeqCst),
                    }
                }
            })?;
        Ok((raised, thread))
    }

    /// Off Linux: ends the helper thread and waits for it, so the mask is
    /// restored only once nothing is left waiting for the signals. SIGUSR1 goes
    /// to that thread alone (`pthread_kill`), never to the process: another
    /// thread of the process may not block it, and its default action ends the
    /// process.
    #[cfg(not(target_os = "linux"))]
    fn end_waiter(&self) {
        use std::os::unix::thread::JoinHandleExt;
        let taken = self.waiter.lock().ok().and_then(|mut waiter| waiter.take());
        if let Some(thread) = taken {
            // A thread that has already left its loop is not signalled: a signal
            // to an exited thread is undefined.
            if thread.is_finished()
                || nix::sys::pthread::pthread_kill(thread.as_pthread_t(), Signal::SIGUSR1).is_ok()
            {
                let _ = thread.join();
            }
        }
    }

    /// Whether a stop was asked for since the last call.
    pub fn requested(&self) -> io::Result<bool> {
        #[cfg(target_os = "linux")]
        {
            Ok(self.fd.read_signal()?.is_some())
        }
        #[cfg(not(target_os = "linux"))]
        {
            Ok(self.raised.swap(false, Ordering::SeqCst))
        }
    }
}

impl StopSignal {
    /// Puts the old mask back now, and says if that failed (Drop does it too, silently).
    pub fn restore(&self) -> io::Result<()> {
        #[cfg(not(target_os = "linux"))]
        self.end_waiter();
        Ok(self.old.thread_set_mask()?)
    }
}

impl Drop for StopSignal {
    fn drop(&mut self) {
        #[cfg(not(target_os = "linux"))]
        self.end_waiter();
        let _ = self.old.thread_set_mask();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Off Linux the drop wakes the helper with SIGUSR1: it must reach that
    /// thread only. Other threads here (the test runner's) do not block it, and
    /// its default action ends the process (the first form of this did, on the
    /// macOS runner). Many rounds, so a stray delivery would show.
    #[test]
    fn dropping_many_stop_signals_never_signals_the_process() {
        for _ in 0..200 {
            let stop = StopSignal::new().unwrap();
            assert!(!stop.requested().unwrap());
            drop(stop);
        }
    }

    #[test]
    fn nothing_is_requested_until_a_signal_comes_and_the_old_mask_returns() {
        let before = SigSet::thread_get_mask().unwrap();
        let stop = StopSignal::new().unwrap();
        assert!(!stop.requested().unwrap());
        assert!(!stop.requested().unwrap());
        drop(stop);
        let after = SigSet::thread_get_mask().unwrap();
        assert_eq!(
            before.contains(Signal::SIGTERM),
            after.contains(Signal::SIGTERM)
        );
        assert_eq!(
            before.contains(Signal::SIGINT),
            after.contains(Signal::SIGINT)
        );
    }
}
