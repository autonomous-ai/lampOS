//! The privacy switch as the account's history reads it (the first
//! builder's privacy read on #8125, item 3): nothing is sent to the
//! account, neither a turn's save nor a forget's delete, while the switch
//! is closed. Both queues wait, and a turn said just before it closed goes
//! only once it is open again.
//!
//! Open only on a valid `voice/privacy` lease (`lamp_msgs::MicrophonePrivacy`,
//! the voice's own, renewed every 10 ms while the switch is open) that
//! permits, until its own `expires_at_us` and never past
//! [`lamp_msgs::MICROPHONE_PRIVACY_FRESH_US`] after it came.
//! A closed or stale lease, none, or no bus: closed.
use lamp_msgs::{MICROPHONE_PRIVACY_FRESH_US, MicrophonePrivacy};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Open until this monotonic time (µs); 0: closed.
static OPEN_UNTIL_US: AtomicU64 = AtomicU64::new(0);
/// The lease's bus subscriber name.
const SUBSCRIBER: &str = "history_privacy";

/// Whether the switch is open now.
pub fn open() -> bool {
    lamp_ipc::monotonic_us() < OPEN_UNTIL_US.load(Ordering::Relaxed)
}

/// `lease`, come at `now_us`: open when valid and permitting, until its
/// own `expires_at_us` and never past its freshness from now (a stale lease
/// read again at a reconnect never opens it, Twelve's P3 on #8167); else
/// closed at once.
pub fn saw(lease: &MicrophonePrivacy, now_us: u64) {
    OPEN_UNTIL_US.store(until(lease, now_us), Ordering::Relaxed);
}

/// Until when (µs) `lease`, come at `now_us`, opens it; 0: closed.
fn until(lease: &MicrophonePrivacy, now_us: u64) -> u64 {
    if lease.check().is_ok() && lease.permitted {
        lease
            .expires_at_us
            .min(now_us.saturating_add(MICROPHONE_PRIVACY_FRESH_US))
    } else {
        0
    }
}

/// Closed at once (a test, or the bus lost).
pub fn close() {
    OPEN_UNTIL_US.store(0, Ordering::Relaxed);
}

/// The lease read from the bus at `root` on a thread of its own, once.
pub fn start(root: &Path) {
    static STARTED: OnceLock<()> = OnceLock::new();
    if STARTED.set(()).is_err() {
        return;
    }
    let root: PathBuf = root.to_owned();
    let started = std::thread::Builder::new()
        .name("history-privacy".into())
        .spawn(move || {
            loop {
                if let Err(error) = read(&root) {
                    close();
                    eprintln!(
                        "lamp-voice history: the privacy switch could not be read ({}); nothing is sent until it is",
                        error.kind()
                    );
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        });
    if started.is_err() {
        eprintln!("lamp-voice history: no thread reads the privacy switch; nothing is sent");
    }
}

fn read(root: &Path) -> std::io::Result<()> {
    let bus = lamp_bus::Bus::new(root)?;
    let mut leases = bus.subscribe_typed(
        MicrophonePrivacy::TOPIC,
        SUBSCRIBER,
        MicrophonePrivacy::TYPE,
    )?;
    loop {
        // Nothing in 100 ms: the last lease runs out by itself.
        if let Some(message) = leases.recv(Duration::from_millis(100))?
            && let Ok(lease) = message.parse::<MicrophonePrivacy>()
        {
            saw(&lease, lamp_ipc::monotonic_us());
        }
        let _ = leases.notices();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lamp_msgs::ClockStamp;

    fn lease(at_us: u64, permitted: bool) -> MicrophonePrivacy {
        MicrophonePrivacy {
            stamp: ClockStamp {
                clock_id: "kernel-boot".into(),
                at_us,
                sequence: 1,
            },
            session_id: "0123456789abcdef".into(),
            controller_boot: "0123456789abcdef0123456789abcdef".into(),
            microphone_generation: 1,
            observed_us: at_us,
            expires_at_us: at_us + MICROPHONE_PRIVACY_FRESH_US,
            permitted,
            physical_muted: Some(!permitted),
            no_slide: false,
        }
    }

    /// Twelve's P3 on #8167: open until the lease's own expiry, never past
    /// its freshness from now; a stale lease read again at a reconnect, a
    /// closed one, or an invalid one: closed.
    #[test]
    fn a_lease_opens_only_until_its_own_expiry() {
        let fresh = lease(1_000_000, true);
        assert_eq!(until(&fresh, 1_000_000), fresh.expires_at_us);
        assert_eq!(until(&fresh, 1_010_000), fresh.expires_at_us);
        // Read again long after it expired: already past, so closed.
        assert!(until(&fresh, 5_000_000) <= 5_000_000);
        assert_eq!(until(&lease(1_000_000, false), 1_000_000), 0);
        let mut invalid = fresh.clone();
        invalid.microphone_generation = 0;
        assert_eq!(until(&invalid, 1_000_000), 0);
    }
}
