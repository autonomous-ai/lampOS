//! The clocks a bus reads: the wall clock for `at_unix_ms`, the kernel's
//! CLOCK_MONOTONIC for judging a stamp that says it is one, and the id of
//! the boot that monotonic clock belongs to.
//!
//! A test or a simulator gives a bus clocks of its own ([`crate::Bus::clocks`]),
//! so that a wall clock set forward or back by chrony can be played without
//! touching the host's.
use crate::wire::Now;
use std::{
    fmt, fs,
    sync::{Arc, OnceLock},
};

/// Where Linux says which boot this is. A new id with every boot, so a
/// monotonic stamp with another id is from a clock that has begun again.
pub const BOOT_ID_PATH: &str = "/proc/sys/kernel/random/boot_id";

type Read = Arc<dyn Fn() -> u64 + Send + Sync>;

#[derive(Clone)]
pub(crate) struct Clocks {
    wall_ms: Read,
    monotonic_us: Read,
    boot_id: Option<Arc<str>>,
}

impl fmt::Debug for Clocks {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Clocks")
            .field("boot_id", &self.boot_id)
            .finish_non_exhaustive()
    }
}

impl Default for Clocks {
    fn default() -> Self {
        Self {
            wall_ms: Arc::new(crate::wire::unix_ms),
            monotonic_us: Arc::new(monotonic_us),
            boot_id: kernel_boot_id().map(Arc::from),
        }
    }
}

impl Clocks {
    pub(crate) fn set(
        &mut self,
        wall_ms: impl Fn() -> u64 + Send + Sync + 'static,
        monotonic_us: impl Fn() -> u64 + Send + Sync + 'static,
    ) {
        self.wall_ms = Arc::new(wall_ms);
        self.monotonic_us = Arc::new(monotonic_us);
    }

    pub(crate) fn set_boot_id(&mut self, id: Option<&str>) {
        self.boot_id = id.map(Arc::from);
    }

    pub(crate) fn wall_ms(&self) -> u64 {
        (self.wall_ms)()
    }

    pub(crate) fn boot_id(&self) -> Option<&str> {
        self.boot_id.as_deref()
    }

    pub(crate) fn now(&self) -> Now<'_> {
        Now {
            unix_ms: (self.wall_ms)(),
            monotonic_us: (self.monotonic_us)(),
            boot_id: self.boot_id(),
        }
    }
}

/// CLOCK_MONOTONIC, in microseconds: the clock every process of one boot
/// shares, which no time server sets.
pub fn monotonic_us() -> u64 {
    let now = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
    let seconds = u64::try_from(now.tv_sec).unwrap_or(0);
    let micros = u64::try_from(now.tv_nsec).unwrap_or(0) / 1_000;
    seconds.saturating_mul(1_000_000).saturating_add(micros)
}

/// This boot's id, read once: `None` where there is none to read (macOS),
/// and then no stamp is judged by the monotonic clock.
pub fn kernel_boot_id() -> Option<&'static str> {
    static READ: OnceLock<Option<String>> = OnceLock::new();
    READ.get_or_init(|| {
        fs::read_to_string(BOOT_ID_PATH)
            .ok()
            .map(|text| text.trim().to_owned())
            .filter(|id| crate::wire::clock_id_is_valid(id))
    })
    .as_deref()
}
