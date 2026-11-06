//! The voice's own code kept in memory (#5611), with `LAMP_VOICE_LOCK_CODE=on`.
//!
//! On lamp-06c1 at 21:30:02 on 6 Nov, capture, the ring and the coordinator,
//! three processes of the one `lamp-voice` executable, stalled 120-132 ms
//! together during an SD log flush, each on exactly one major fault, and the
//! privacy lease lapsed. Nothing was blocked on a write: a page the three run
//! was read back from the card. This keeps those pages in memory.
//!
//! At the start of the voice and of each of its workers, a thread of its own
//! reads `/proc/self/maps` and `mlock`s every executable file mapping
//! (`r-xp` with a path): the executable's text and its shared libraries'.
//! Not the heap, the stacks or any data (no `mlockall`). The pages are the
//! page cache's, shared by every process that maps the file, so each
//! process's count is the same bytes again, not more memory.
//!
//! The thread first takes the ordinary policy at nice 0, never the RR 20 it
//! inherits, so reading the code in from the card never takes a core from
//! capture, the speaker or the ring ([`LOCK_SCHEDULING`]).
//!
//! The lock never holds up a start: no start path waits for its thread. A
//! failed lock is one log line, with the `RLIMIT_MEMLOCK` it met, and the
//! voice runs on unlocked. Off unless set to exactly `on`: then nothing is
//! read or locked.
use std::io;

pub const ENVIRONMENT: &str = "LAMP_VOICE_LOCK_CODE";

/// On only for exactly `on`.
pub fn on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// The switch, as the voice has it, for `worker_command`: a worker starts
/// with a cleared environment.
pub fn passed_on() -> Vec<(&'static str, std::ffi::OsString)> {
    std::env::var_os(ENVIRONMENT)
        .map(|value| (ENVIRONMENT, value))
        .into_iter()
        .collect()
}

/// One executable file mapping of `/proc/self/maps`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mapping {
    pub start: usize,
    pub end: usize,
    pub path: String,
}

impl Mapping {
    pub fn bytes(&self) -> usize {
        self.end.saturating_sub(self.start)
    }
}

/// The `r-xp` mappings with a file path, in the order listed. Anonymous
/// mappings, `[vdso]` and its kind, and every other permission are left out;
/// a line that doesn't read is skipped.
pub fn code_mappings(maps: &str) -> Vec<Mapping> {
    maps.lines().filter_map(code_mapping).collect()
}

/// `start-end perms offset dev inode path`, the path from the sixth field
/// to the line's end (it may hold spaces, and ` (deleted)`).
fn code_mapping(line: &str) -> Option<Mapping> {
    let mut fields = line.splitn(6, ' ');
    let range = fields.next()?;
    if fields.next()? != "r-xp" {
        return None;
    }
    let (_offset, _device, _inode) = (fields.next()?, fields.next()?, fields.next()?);
    let path = fields.next()?.trim_start();
    if !path.starts_with('/') {
        return None;
    }
    let (start, end) = range.split_once('-')?;
    let start = usize::from_str_radix(start, 16).ok()?;
    let end = usize::from_str_radix(end, 16).ok()?;
    (end > start).then(|| Mapping {
        start,
        end,
        path: path.to_owned(),
    })
}

/// What locking did.
#[derive(Debug, PartialEq, Eq)]
pub struct Outcome {
    pub mappings: usize,
    pub locked: usize,
    pub locked_bytes: usize,
    /// The first mapping that failed and why; the rest are still tried.
    pub failed: Option<(String, io::ErrorKind, String)>,
    pub failures: usize,
}

/// Lock each of `mappings` with `lock(start, bytes)`. A failure is counted
/// and kept, never a panic or an exit.
pub fn lock_all(
    mappings: &[Mapping],
    mut lock: impl FnMut(usize, usize) -> io::Result<()>,
) -> Outcome {
    let mut outcome = Outcome {
        mappings: mappings.len(),
        locked: 0,
        locked_bytes: 0,
        failed: None,
        failures: 0,
    };
    for mapping in mappings {
        match lock(mapping.start, mapping.bytes()) {
            Ok(()) => {
                outcome.locked += 1;
                outcome.locked_bytes += mapping.bytes();
            }
            Err(error) => {
                outcome.failures += 1;
                outcome
                    .failed
                    .get_or_insert_with(|| (mapping.path.clone(), error.kind(), error.to_string()));
            }
        }
    }
    outcome
}

/// With the switch on, read the maps and lock what they list; off, neither
/// `maps` nor `lock` is called and there is nothing to say.
pub fn run(
    value: Option<&str>,
    maps: impl FnOnce() -> io::Result<String>,
    lock: impl FnMut(usize, usize) -> io::Result<()>,
) -> Option<Result<Outcome, io::Error>> {
    if !on(value) {
        return None;
    }
    Some(maps().map(|maps| lock_all(&code_mappings(&maps), lock)))
}

/// The one line for the journal, with the memory lock limit `(soft, hard)`
/// and the memory available after, where they could be read.
pub fn line(
    role: &str,
    outcome: &Result<Outcome, io::Error>,
    limit: Option<(u64, u64)>,
    available_kb: Option<u64>,
) -> String {
    let limit = limit.map_or_else(
        || "RLIMIT_MEMLOCK unread".to_owned(),
        |(soft, hard)| format!("RLIMIT_MEMLOCK {soft}/{hard}"),
    );
    let available = available_kb.map_or_else(
        || "MemAvailable unread".to_owned(),
        |kb| format!("MemAvailable {kb} kB"),
    );
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(error) => {
            return format!(
                "lamp-voice {role}: its code is not locked in memory ({ENVIRONMENT}=on): /proc/self/maps unread: {error}; {limit}; it runs on as before"
            );
        }
    };
    let mib = outcome.locked_bytes as f64 / (1024.0 * 1024.0);
    match &outcome.failed {
        None => format!(
            "lamp-voice {role}: its code is locked in memory ({ENVIRONMENT}=on): {} mappings, {} bytes ({mib:.1} MiB); {available}",
            outcome.mappings, outcome.locked_bytes
        ),
        Some((path, kind, error)) => format!(
            "lamp-voice {role}: its code is not all locked in memory ({ENVIRONMENT}=on): {} of {} mappings, {} bytes ({mib:.1} MiB); the first failure, {path}: {error} ({kind:?}); {limit}; {available}; it runs on as before",
            outcome.locked, outcome.mappings, outcome.locked_bytes
        ),
    }
}

/// Lock this process's code on a thread of its own, if the switch is on,
/// and say the one line. Nothing waits for it, and nothing it meets ends
/// the process: a thread that can't start is a line too.
pub fn start(role: String) {
    if !on(std::env::var(ENVIRONMENT).ok().as_deref()) {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("lamp-lock-code".into())
        .spawn(move || {
            for said in on_its_thread(
                &role,
                || crate::worker_scheduling::take(LOCK_SCHEDULING),
                || std::fs::read_to_string("/proc/self/maps"),
                real_lock,
                limit(),
                available_kb,
            ) {
                eprintln!("{said}");
            }
        });
    if let Err(error) = spawned {
        eprintln!(
            "lamp-voice: its code is not locked in memory ({ENVIRONMENT}=on): its thread did not start: {error}; it runs on as before"
        );
    }
}

/// The scheduling the lock's thread takes before it reads or locks: the
/// ordinary policy at nice 0 (`worker_scheduling::take`), never the
/// real-time RR 20 it inherits from the voice or a worker, so paging the
/// code in never takes a core from capture, the speaker or the ring.
pub const LOCK_SCHEDULING: lamp_voice_processes::Scheduling =
    lamp_voice_processes::Scheduling::Ordinary;

/// The lock thread's whole work, in order: `lower` first, then the maps
/// read and the locks. A failed `lower` is a line of its own and the lock
/// goes on; the lines to say are returned, at most two.
pub fn on_its_thread(
    role: &str,
    lower: impl FnOnce() -> io::Result<()>,
    maps: impl FnOnce() -> io::Result<String>,
    lock: impl FnMut(usize, usize) -> io::Result<()>,
    limit: Option<(u64, u64)>,
    available_kb: impl FnOnce() -> Option<u64>,
) -> Vec<String> {
    let mut said = Vec::new();
    if let Err(error) = lower() {
        said.push(format!(
            "lamp-voice {role}: the code lock's thread did not take the ordinary policy ({ENVIRONMENT}=on): {error}; it locks at the policy it has"
        ));
    }
    if let Some(outcome) = run(Some("on"), maps, lock) {
        said.push(line(role, &outcome, limit, available_kb()));
    }
    said
}

/// `mlock` of one of this process's own mappings. The only unsafe call
/// here: `mlock` is `unsafe` in `nix` (and the standard library has none)
/// as it takes a raw address, but it neither reads nor writes through it;
/// it pins the pages of the range in memory. The range is one the kernel
/// listed for this process in `/proc/self/maps`; one unmapped since is an
/// `ENOMEM`, never a fault.
#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn real_lock(start: usize, bytes: usize) -> io::Result<()> {
    let Some(address) = std::ptr::NonNull::new(start as *mut std::ffi::c_void) else {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    };
    // SAFETY: `mlock` only pins the pages of `[start, start + bytes)` and
    // never dereferences them; a range not mapped is refused with ENOMEM.
    unsafe { nix::sys::mman::mlock(address, bytes) }.map_err(io::Error::from)
}

/// Elsewhere the voice runs only on a builder's machine: nothing locked.
#[cfg(not(target_os = "linux"))]
fn real_lock(_start: usize, _bytes: usize) -> io::Result<()> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

/// `(soft, hard)`; `rlim_t` is `u64` on the lamps' 64-bit Linux.
#[cfg(target_os = "linux")]
fn limit() -> Option<(u64, u64)> {
    nix::sys::resource::getrlimit(nix::sys::resource::Resource::RLIMIT_MEMLOCK).ok()
}

#[cfg(not(target_os = "linux"))]
fn limit() -> Option<(u64, u64)> {
    None
}

/// `MemAvailable` of `/proc/meminfo`, in kB.
fn available_kb() -> Option<u64> {
    std::fs::read_to_string("/proc/meminfo")
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("MemAvailable:"))?
        .trim()
        .trim_end_matches("kB")
        .trim()
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAPS: &str = "\
aaaab0000000-aaaab1e00000 r-xp 00000000 b3:02 131090                     /opt/lampOS/voice/releases/10579/lamp-voice
aaaab1e10000-aaaab1f80000 r--p 01e00000 b3:02 131090                     /opt/lampOS/voice/releases/10579/lamp-voice
aaaab1f80000-aaaab1f90000 rw-p 01f70000 b3:02 131090                     /opt/lampOS/voice/releases/10579/lamp-voice
aaaab2000000-aaaab2100000 rw-p 00000000 00:00 0                          [heap]
ffff80000000-ffff80021000 rw-p 00000000 00:00 0
ffff8a000000-ffff8a188000 r-xp 00000000 b3:02 2053                       /usr/lib/aarch64-linux-gnu/libc.so.6
ffff8a188000-ffff8a19d000 ---p 00188000 b3:02 2053                       /usr/lib/aarch64-linux-gnu/libc.so.6
ffff8a200000-ffff8a2d0000 r-xp 00000000 b3:02 4410                       /usr/lib/aarch64-linux-gnu/libasound.so.2.0.0
ffff8a300000-ffff8a301000 r-xp 00000000 00:00 0
ffff8a400000-ffff8a42b000 r-xp 00000000 b3:02 2050                       /usr/lib/aarch64-linux-gnu/ld-linux-aarch64.so.1
ffff8a500000-ffff8a501000 r-xs 00000000 00:05 77                         /dev/shm/a-shared-exec
ffff8a600000-ffff8a700000 rwxp 00000000 b3:02 99                         /opt/jit-ish
ffffc0000000-ffffc0021000 rw-p 00000000 00:00 0                          [stack]
ffffc1000000-ffffc1001000 r-xp 00000000 00:00 0                          [vdso]
not a mapping line
ffff8a800000-ffff8a900000 r-xp 00000000 b3:02 131000                     /opt/lampOS/voice/releases/10578/lamp-voice (deleted)
";

    fn mapping(start: usize, end: usize, path: &str) -> Mapping {
        Mapping {
            start,
            end,
            path: path.to_owned(),
        }
    }

    #[test]
    fn only_executable_private_file_mappings_are_chosen_from_the_maps() {
        assert_eq!(
            code_mappings(MAPS),
            vec![
                mapping(
                    0xaaaab0000000,
                    0xaaaab1e00000,
                    "/opt/lampOS/voice/releases/10579/lamp-voice"
                ),
                mapping(
                    0xffff8a000000,
                    0xffff8a188000,
                    "/usr/lib/aarch64-linux-gnu/libc.so.6"
                ),
                mapping(
                    0xffff8a200000,
                    0xffff8a2d0000,
                    "/usr/lib/aarch64-linux-gnu/libasound.so.2.0.0"
                ),
                mapping(
                    0xffff8a400000,
                    0xffff8a42b000,
                    "/usr/lib/aarch64-linux-gnu/ld-linux-aarch64.so.1"
                ),
                // A replaced executable still mapped is still the code it runs.
                mapping(
                    0xffff8a800000,
                    0xffff8a900000,
                    "/opt/lampOS/voice/releases/10578/lamp-voice (deleted)"
                ),
            ]
        );
        assert_eq!(code_mappings(""), Vec::new());
    }

    #[test]
    fn a_failing_lock_is_counted_and_the_rest_are_still_locked() {
        let mappings = code_mappings(MAPS);
        let mut asked = Vec::new();
        let outcome = lock_all(&mappings, |start, bytes| {
            asked.push((start, bytes));
            if start == 0xffff8a200000 {
                Err(io::Error::from_raw_os_error(12))
            } else {
                Ok(())
            }
        });
        assert_eq!(asked.len(), 5, "every mapping is tried: {asked:x?}");
        assert_eq!(asked[0], (0xaaaab0000000, 0x1e00000));
        assert_eq!(outcome.mappings, 5);
        assert_eq!(outcome.locked, 4);
        assert_eq!(outcome.failures, 1);
        assert_eq!(
            outcome.locked_bytes,
            0x1e00000 + 0x188000 + 0x2b000 + 0x100000
        );
        let (path, _, _) = outcome.failed.as_ref().expect("the failure kept");
        assert_eq!(path, "/usr/lib/aarch64-linux-gnu/libasound.so.2.0.0");
        let said = line(
            "capture",
            &Ok(outcome),
            Some((8_388_608, 8_388_608)),
            Some(512_000),
        );
        assert_eq!(said.lines().count(), 1, "{said}");
        assert!(said.contains("lamp-voice capture"), "{said}");
        assert!(said.contains("4 of 5"), "{said}");
        assert!(said.contains("libasound.so.2.0.0"), "{said}");
        assert!(said.contains("RLIMIT_MEMLOCK 8388608/8388608"), "{said}");
        assert!(said.contains("runs on"), "{said}");
    }

    #[test]
    fn every_lock_failing_or_no_maps_is_one_line_and_never_a_panic() {
        let all_fail = run(
            Some("on"),
            || Ok(MAPS.to_owned()),
            |_, _| Err(io::Error::from_raw_os_error(1)),
        )
        .expect("on");
        let outcome = all_fail.as_ref().expect("the maps were read");
        assert_eq!((outcome.locked, outcome.failures), (0, 5));
        let said = line("serve", &all_fail, Some((65_536, 65_536)), None);
        assert_eq!(said.lines().count(), 1, "{said}");
        assert!(said.contains("0 of 5"), "{said}");
        assert!(said.contains("runs on"), "{said}");

        let unread = run(
            Some("on"),
            || Err(io::Error::from(io::ErrorKind::NotFound)),
            |_, _| panic!("nothing to lock without the maps"),
        )
        .expect("on");
        assert!(unread.is_err());
        let said = line("ring", &unread, None, None);
        assert_eq!(said.lines().count(), 1, "{said}");
        assert!(said.contains("/proc/self/maps"), "{said}");
        assert!(said.contains("runs on"), "{said}");
    }

    #[test]
    fn all_locked_is_one_line_with_the_bytes() {
        let done = run(Some("on"), || Ok(MAPS.to_owned()), |_, _| Ok(())).expect("on");
        let said = line("serve", &done, None, Some(400_000));
        assert_eq!(said.lines().count(), 1, "{said}");
        assert!(said.contains("5 mappings"), "{said}");
        let bytes = 0x1e00000 + 0x188000 + 0xd0000 + 0x2b000 + 0x100000;
        assert!(said.contains(&format!("{bytes} bytes")), "{said}");
        assert!(said.contains("400000 kB"), "{said}");
    }

    #[test]
    fn switched_off_nothing_is_read_or_locked() {
        for value in [
            None,
            Some(""),
            Some("off"),
            Some("ON"),
            Some("on "),
            Some("1"),
        ] {
            assert!(!on(value), "{value:?}");
            let said = run(
                value,
                || panic!("the maps read with {value:?}"),
                |_, _| panic!("a lock with {value:?}"),
            );
            assert!(said.is_none(), "{value:?}");
        }
        assert!(on(Some("on")));
    }

    #[test]
    fn the_lock_thread_takes_the_ordinary_policy_before_any_lock() {
        assert_eq!(LOCK_SCHEDULING, lamp_voice_processes::Scheduling::Ordinary);
        let order = std::cell::RefCell::new(Vec::new());
        let said = on_its_thread(
            "capture",
            || {
                order.borrow_mut().push("lower");
                Ok(())
            },
            || {
                order.borrow_mut().push("maps");
                Ok(MAPS.to_owned())
            },
            |_, _| {
                order.borrow_mut().push("lock");
                Ok(())
            },
            None,
            || None,
        );
        let order = order.into_inner();
        assert_eq!(order[..2], ["lower", "maps"], "{order:?}");
        assert_eq!(order.len(), 2 + 5, "{order:?}");
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].contains("its code is locked"), "{said:?}");
    }

    #[test]
    fn a_failed_lowering_is_a_line_and_the_lock_goes_on() {
        let mut locks = 0;
        let said = on_its_thread(
            "ring",
            || Err(io::Error::from_raw_os_error(1)),
            || Ok(MAPS.to_owned()),
            |_, _| {
                locks += 1;
                Ok(())
            },
            None,
            || None,
        );
        assert_eq!(locks, 5);
        assert_eq!(said.len(), 2, "{said:?}");
        assert!(said[0].contains("lamp-voice ring"), "{said:?}");
        assert!(said[0].contains("ordinary policy"), "{said:?}");
        assert!(said[0].contains("locks at the policy it has"), "{said:?}");
        assert!(said[1].contains("its code is locked"), "{said:?}");
    }

    /// The real call: this test process's own code, locked. Root, or a
    /// memory lock limit too small, may refuse it; that is a line too.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_real_lock_locks_this_process_code_or_says_why_not() {
        let said = run(
            Some("on"),
            || std::fs::read_to_string("/proc/self/maps"),
            real_lock,
        )
        .expect("on");
        let outcome = said.as_ref().expect("the maps read");
        assert!(outcome.mappings > 0);
        assert_eq!(outcome.locked + outcome.failures, outcome.mappings);
        assert_eq!(
            line("test", &said, limit(), available_kb()).lines().count(),
            1
        );
    }
}
