//! Tests that flush files to the disk, and the one test that times bus calls
//! whose files are on the same disk, take turns (#3315).
//!
//! On macOS `File::sync_all` and `sync_data` are `F_FULLFSYNC`, which empties
//! the drive's cache: about 19 ms each on the office Mac. While one runs, a
//! bus `latest` file written and renamed under `/tmp` waits for it, so
//! `light_call`'s twenty calls took 19, 38, 57 ... 164 ms beside `diagnostics`
//! and `news_told` in a full run, against 3 ms alone or beside sixteen busy
//! processes. A lamp's bus is on tmpfs (`/run/lampos-bus`), and Linux has no
//! `F_FULLFSYNC`. Flushing tests share the lock with each other; the timed
//! test takes it alone, so it times the light calls and not another test's
//! flush.
use std::cell::RefCell;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

static FLUSHES: RwLock<()> = RwLock::new(());

thread_local! {
    /// This thread's share and how many `Flushing`s hold it. A second shared
    /// lock on one thread could wait behind a queued `timing` forever.
    static HELD: RefCell<(usize, Option<RwLockReadGuard<'static, ()>>)> =
        const { RefCell::new((0, None)) };
}

/// Held for as long as a test may flush files to the disk, on the thread
/// that took it (not `Send`).
pub(crate) struct Flushing(std::marker::PhantomData<*const ()>);

pub(crate) fn flushing() -> Flushing {
    HELD.with(|held| {
        let mut held = held.borrow_mut();
        if held.0 == 0 {
            held.1 = Some(
                FLUSHES
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()),
            );
        }
        held.0 += 1;
    });
    Flushing(std::marker::PhantomData)
}

impl Drop for Flushing {
    fn drop(&mut self) {
        HELD.with(|held| {
            let mut held = held.borrow_mut();
            held.0 -= 1;
            if held.0 == 0 {
                held.1 = None;
            }
        });
    }
}

/// Held while a test times calls that write files, with no flush beside it.
pub(crate) fn timing() -> RwLockWriteGuard<'static, ()> {
    FLUSHES
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
