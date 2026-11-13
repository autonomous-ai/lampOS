//! The virtual desktop lamp's hardware edge (docs/simulator-lamp.md).
/// Tests that write a script and run it take this first: a script just
/// written can be refused as busy ("text file busy") when another test's
/// fork holds its write handle for a moment, which failed one of them about
/// one run in three.
#[cfg(test)]
pub(crate) static FAKE_PROGRAM: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The folder a test makes its own under: the system's temp folder, or `/tmp`
/// when that one is too long for the bus's sockets. A Unix socket's address
/// holds 103 bytes, and macOS's temp folder (`/var/folders/…/T/`, about 49
/// bytes) left the ring's and the voice's test sockets at 105 to 119.
#[cfg(test)]
pub(crate) fn test_root() -> std::path::PathBuf {
    let temp = std::env::temp_dir();
    if temp.as_os_str().len() > 24 {
        std::path::PathBuf::from("/tmp")
    } else {
        temp
    }
}

/// This machine's boot id, the one id the ring, lamp-control and lamp-body
/// share as the bus's clock id: Linux's `/proc/sys/kernel/random/boot_id`, or
/// on macOS the boot session uuid (`kern.bootsessionuuid`, as lamp-body reads
/// it there, #6953), else one made for this run. The body refuses a lease whose
/// clock id is not its own, so a made-up id here would refuse every expression
/// request once the body runs on macOS.
pub fn boot_id() -> String {
    std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .ok()
        .or_else(macos_boot_session)
        .map(|id| id.trim().to_owned())
        .filter(|id| !id.is_empty() && id.len() <= 128)
        .unwrap_or_else(|| {
            format!(
                "lamp-sim-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_nanos())
            )
        })
}

#[cfg(target_os = "macos")]
fn macos_boot_session() -> Option<String> {
    let out = keys::command("/usr/sbin/sysctl")
        .args(["-n", "kern.bootsessionuuid"])
        .output()
        .ok()?;
    out.status.success().then_some(())?;
    String::from_utf8(out.stdout).ok()
}

#[cfg(not(target_os = "macos"))]
fn macos_boot_session() -> Option<String> {
    None
}

pub mod camera;
pub mod controls;
pub mod keys;
pub mod manage;
pub mod mic;
pub mod people;
pub mod pose;
pub mod pty;
pub mod real_voice;
pub mod ring;
pub mod servo;
pub mod stimuli;
pub mod voice;
pub mod warp;

#[cfg(test)]
mod boot_id_tests {
    // The boot id is the system's own where it has one, the same at every ask:
    // Linux's boot_id, macOS's boot session uuid (what lamp-body reads, #6953).
    #[test]
    fn the_boot_id_is_the_systems_own_and_stays_the_same() {
        let id = super::boot_id();
        assert_eq!(id, super::boot_id());
        #[cfg(target_os = "linux")]
        assert_eq!(
            id,
            std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
                .unwrap()
                .trim()
        );
        #[cfg(target_os = "macos")]
        {
            let out = std::process::Command::new("/usr/sbin/sysctl")
                .args(["-n", "kern.bootsessionuuid"])
                .output()
                .unwrap();
            assert_eq!(id, String::from_utf8(out.stdout).unwrap().trim());
            assert!(!id.starts_with("lamp-sim-"), "{id}");
        }
    }
}
