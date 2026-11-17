//! The clocks a bus reads: the wall clock for `at_unix_ms`, the kernel's
//! CLOCK_MONOTONIC for judging a stamp that says it is one, and the id of
//! the boot that monotonic clock belongs to.
//!
//! A test or a simulator gives a bus clocks of its own ([`crate::Bus::clocks`]),
//! so that a wall clock set forward or back by chrony can be played without
//! touching the host's.
use crate::wire::Now;
#[cfg(target_os = "linux")]
use std::fs;
use std::{
    fmt,
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

/// This boot's id, read once: Linux's `/proc/sys/kernel/random/boot_id`; on macOS the boot session
/// uuid, `kern.bootsessionuuid` (a new one with every boot, the id `lamp-controls`, `lamp-body` and
/// `lamp-sim` use there); `None` where there is none to read or the read failed, and then no stamp
/// is judged by the monotonic clock but by the wall clock, as before. Never an empty or made-up id.
pub fn kernel_boot_id() -> Option<&'static str> {
    static READ: OnceLock<Option<String>> = OnceLock::new();
    READ.get_or_init(read_boot_id).as_deref()
}

#[cfg(target_os = "linux")]
fn read_boot_id() -> Option<String> {
    fs::read_to_string(BOOT_ID_PATH)
        .ok()
        .map(|text| text.trim().to_owned())
        .filter(|id| crate::wire::clock_id_is_valid(id))
}

/// Where macOS keeps `sysctl`: the absolute path, as every other asker of the id uses.
#[cfg(target_os = "macos")]
const SYSCTL: &str = "/usr/sbin/sysctl";

#[cfg(target_os = "macos")]
fn read_boot_id() -> Option<String> {
    let out = std::process::Command::new(SYSCTL)
        .args(["-n", "kern.bootsessionuuid"])
        .output()
        .ok()?;
    boot_session_id(out.status.success(), &out.stdout)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn read_boot_id() -> Option<String> {
    None
}

/// What `sysctl -n kern.bootsessionuuid` printed, as an id: its text trimmed, when the command
/// succeeded and the text is a valid clock id (1 to 64 letters, digits or dashes); otherwise none.
#[cfg(any(target_os = "macos", test))]
fn boot_session_id(succeeded: bool, stdout: &[u8]) -> Option<String> {
    if !succeeded {
        return None;
    }
    let text = std::str::from_utf8(stdout).ok()?.trim();
    crate::wire::clock_id_is_valid(text).then(|| text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const UUID: &str = "6F2C1A90-3B7D-4E58-9A14-0C5D8E7B2F61";

    /// What `sysctl -n kern.bootsessionuuid` prints is an id only when the command succeeded and
    /// the text is a valid clock id; a failed read is none, never an empty or made-up id, so the
    /// stamps are then judged by the wall clock as before.
    #[test]
    fn a_boot_session_id_is_the_trimmed_text_of_a_successful_read_and_none_otherwise() {
        assert_eq!(
            boot_session_id(true, format!("{UUID}\n").as_bytes()).as_deref(),
            Some(UUID)
        );
        assert_eq!(boot_session_id(false, UUID.as_bytes()), None);
        assert_eq!(boot_session_id(true, b""), None);
        assert_eq!(boot_session_id(true, b"  \n"), None);
        assert_eq!(boot_session_id(true, &[0xff, 0xfe]), None);
        assert_eq!(boot_session_id(true, b"not a uuid"), None);
        assert_eq!(boot_session_id(true, "A".repeat(65).as_bytes()), None);
    }

    /// The boot id, when there is one, is a valid clock id, and it is read once.
    #[test]
    fn the_kernel_boot_id_is_valid_when_there_is_one_and_the_same_every_time() {
        let first = kernel_boot_id();
        assert_eq!(first, kernel_boot_id());
        if let Some(id) = first {
            assert!(crate::wire::clock_id_is_valid(id), "{id}");
        }
    }

    /// Linux is as it was: the boot id is `/proc`'s, trimmed.
    #[cfg(target_os = "linux")]
    #[test]
    fn on_linux_the_boot_id_is_proc_s_text_trimmed() {
        let text = std::fs::read_to_string(BOOT_ID_PATH).unwrap();
        assert_eq!(kernel_boot_id(), Some(text.trim()));
    }

    /// On a Mac the boot id is what `sysctl -n kern.bootsessionuuid` says (the id lamp-controls
    /// and lamp-body use there), byte for byte.
    #[cfg(target_os = "macos")]
    #[test]
    fn on_a_mac_the_boot_id_is_the_sysctl_boot_session_uuid() {
        let out = std::process::Command::new(SYSCTL)
            .args(["-n", "kern.bootsessionuuid"])
            .output()
            .unwrap();
        assert!(out.status.success());
        let said = String::from_utf8(out.stdout).unwrap();
        assert_eq!(kernel_boot_id(), Some(said.trim()));
    }
}
