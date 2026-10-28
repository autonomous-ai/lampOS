//! Tests that start a child process, and tests that check a socket of theirs
//! closed, take turns (#3315).
//!
//! On macOS a socket is made close-on-exec in a second step after it is
//! created. A child started between the two keeps a copy until it exits, and
//! a child forked while a peer closes keeps one until its exec. Either way the
//! peer is not closed: `reference_link`'s send to a closed capture was
//! accepted in 16 of 20 runs beside `process`, which starts `lamp-voice`
//! workers, and in 0 of 20 alone. The worker fixture's Stop already waits
//! this out (`send_stop_until_peer_gone`, #1569). Linux makes a socket close-on-exec as it is
//! created, and nextest runs each test in its own process.
//!
//! Every test in this binary that starts a child takes `spawning()`:
//! `process`'s five, `fixture_provider`'s real fixture worker and
//! `ring_worker`'s late-startup child. `privacy_worker` also starts workers,
//! but it is built only on Linux, where this race does not happen.
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

static SPAWNS: RwLock<()> = RwLock::new(());

/// Held by a test that starts children, for as long as it may start one.
pub(crate) fn spawning() -> RwLockWriteGuard<'static, ()> {
    SPAWNS
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Held by a test whose sockets must close when it closes them, once, for
/// the whole test.
pub(crate) fn closing_sockets() -> RwLockReadGuard<'static, ()> {
    SPAWNS
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
