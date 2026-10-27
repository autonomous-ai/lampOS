//! The camera's helper (#2892, Sixth's find on lamp-52e6, 22 Oct 11:11): a
//! voice restart that dropped the camera's permit left the USB camera
//! refusing every open (`DeadlineExceeded` at StreamOn or Controls) until
//! its USB device was re-authorized by hand. The eyes' own reset
//! ([`crate::reopen`], `lamp_camera::linux::reset_usb`) needs the device's
//! node under `/dev/bus/usb`, root's (0664), and the eyes run as `lampos`,
//! so it is refused there.
//!
//! So a narrow root helper does the one thing: `lamp-eyes
//! reauthorize-camera --usb VENDOR:PRODUCT@TOPOLOGY`, started by its socket
//! (`lamp-eyes-camera.socket`) for each connection, with the id its own
//! unit names. It checks that the device at that port is that camera,
//! writes its `authorized` 0 then 1, as an unplug and a plug would, at most
//! once in [`EVERY_S`], and answers one line. Whoever connects says
//! nothing it reads: it can't be pointed at another device.
//!
//! The eyes ask it ([`ask`], `--camera-helper`) from a thread of their own
//! after [`ASK_AFTER`] opens in a row that did not open, then at most once
//! in [`EVERY_S`] while it stays so, and say each answer once.

use std::{
    io::{self, Read},
    path::Path,
    time::Duration,
};

/// The helper re-authorizes the camera at most once in this many seconds,
/// and the eyes ask it no more often.
pub const EVERY_S: u64 = 60;
/// The eyes ask after this many opens in a row that did not open: about
/// 10 s at [`crate::reopen::RETRY`], so the camera is back within #2892's
/// 30 s.
pub const ASK_AFTER: u32 = 3;
/// The eyes wait at most this long for the helper's answer, on their
/// thread for it, never the loop's.
pub const ANSWER_WITHIN: Duration = Duration::from_secs(5);
/// Between `authorized` 0 and 1: the kernel lets the device's interfaces go.
const UNPLUGGED_FOR: Duration = Duration::from_millis(500);

/// What the helper did.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Re-authorized, as an unplug and a plug.
    Reauthorized,
    /// Done less than [`EVERY_S`] ago: this many seconds.
    TooSoon(u64),
    /// No device at that port.
    NoDevice,
    /// The device at that port isn't the camera named.
    NotTheCamera,
}

impl Outcome {
    /// The helper's one-line answer.
    pub fn line(&self) -> String {
        match self {
            Outcome::Reauthorized => r#"{"reauthorized":true}"#.to_owned(),
            Outcome::TooSoon(ago) => {
                format!(r#"{{"reauthorized":false,"why":"too_soon","ago_s":{ago}}}"#)
            }
            Outcome::NoDevice => r#"{"reauthorized":false,"why":"no_device"}"#.to_owned(),
            Outcome::NotTheCamera => r#"{"reauthorized":false,"why":"not_the_camera"}"#.to_owned(),
        }
    }
}

/// Whether `topology` is a USB port's name and nothing else: digits, `-`
/// and `.`, as `--usb` takes it, so no path is made of it.
pub fn plain_port(topology: &str) -> bool {
    !topology.is_empty()
        && topology.len() <= 64
        && topology
            .bytes()
            .all(|byte| byte.is_ascii_digit() || b"-.".contains(&byte))
        && !topology.contains("..")
}

/// Re-authorizes the USB device at `topology` under `devices`
/// (`/sys/bus/usb/devices`) if it is `vendor:product`, at most once in
/// [`EVERY_S`] by the time kept in `stamp` (`now_s`, seconds since the
/// epoch). The stamp is written before the device is touched, so a helper
/// that dies half way still waits its minute.
pub fn reauthorize_at(
    devices: &Path,
    topology: &str,
    vendor: u16,
    product: u16,
    stamp: &Path,
    now_s: u64,
) -> io::Result<Outcome> {
    if !plain_port(topology) {
        return Err(io::Error::other("not a USB port's name"));
    }
    if let Some(last) = std::fs::read_to_string(stamp)
        .ok()
        .and_then(|text| text.trim().parse::<u64>().ok())
        && now_s.saturating_sub(last) < EVERY_S
        && last <= now_s
    {
        return Ok(Outcome::TooSoon(now_s - last));
    }
    let device = devices.join(topology);
    let id = |name: &str| {
        std::fs::read_to_string(device.join(name))
            .ok()
            .and_then(|text| u16::from_str_radix(text.trim(), 16).ok())
    };
    let (Some(found_vendor), Some(found_product)) = (id("idVendor"), id("idProduct")) else {
        return Ok(Outcome::NoDevice);
    };
    if (found_vendor, found_product) != (vendor, product) {
        return Ok(Outcome::NotTheCamera);
    }
    std::fs::write(stamp, now_s.to_string())?;
    let authorized = device.join("authorized");
    std::fs::write(&authorized, "0")?;
    std::thread::sleep(UNPLUGGED_FOR);
    // Plugged back in once more if the first write fails, so a camera is
    // never left unplugged until the next ask (the first builder's P3 on
    // #2921); the first error is the one said.
    if let Err(first) = std::fs::write(&authorized, "1") {
        std::thread::sleep(UNPLUGGED_FOR);
        std::fs::write(&authorized, "1").map_err(|_| first)?;
    }
    Ok(Outcome::Reauthorized)
}

/// The eyes' ask: connects to the helper's socket, says nothing, and reads
/// its one line, within [`ANSWER_WITHIN`].
pub fn ask(socket: &Path) -> io::Result<String> {
    let stream = std::os::unix::net::UnixStream::connect(socket)?;
    stream.set_read_timeout(Some(ANSWER_WITHIN))?;
    stream.shutdown(std::net::Shutdown::Write)?;
    let mut answer = String::new();
    stream.take(512).read_to_string(&mut answer)?;
    Ok(answer.trim().to_owned())
}

/// When the eyes ask: after the [`ASK_AFTER`]th open in a row that did not
/// open, then at most once in [`EVERY_S`] (by `last_s`, seconds since any
/// fixed start, of the last ask) while they go on failing.
pub fn ask_now(failed: u32, last_s: Option<u64>, now_s: u64) -> bool {
    failed >= ASK_AFTER && last_s.is_none_or(|last| now_s.saturating_sub(last) >= EVERY_S)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("eyes-helper-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A port in a fake `/sys/bus/usb/devices` with the camera's ids.
    fn camera_at(devices: &Path, port: &str, vendor: &str, product: &str) {
        let device = devices.join(port);
        std::fs::create_dir_all(&device).unwrap();
        std::fs::write(device.join("idVendor"), format!("{vendor}\n")).unwrap();
        std::fs::write(device.join("idProduct"), format!("{product}\n")).unwrap();
        std::fs::write(device.join("authorized"), "1\n").unwrap();
    }

    /// #2892: the helper re-authorizes only the camera named, at its port,
    /// at most once a minute, and nothing else.
    #[test]
    fn the_helper_reauthorizes_only_the_named_camera_once_a_minute() {
        let dir = folder("once");
        let devices = dir.join("devices");
        camera_at(&devices, "3-1.1.4", "01da", "5875");
        camera_at(&devices, "3-1.2", "046d", "0825");
        let stamp = dir.join("last");
        let at = |port: &str, now| reauthorize_at(&devices, port, 0x01da, 0x5875, &stamp, now);
        // Another device at the port named: left alone.
        assert_eq!(at("3-1.2", 1_000).unwrap(), Outcome::NotTheCamera);
        assert!(!stamp.exists());
        // Nothing at the port.
        assert_eq!(at("3-1.3", 1_000).unwrap(), Outcome::NoDevice);
        // The camera: 0 then 1, the time kept.
        assert_eq!(at("3-1.1.4", 1_000).unwrap(), Outcome::Reauthorized);
        let authorized = std::fs::read_to_string(devices.join("3-1.1.4/authorized")).unwrap();
        assert_eq!(authorized, "1");
        assert_eq!(std::fs::read_to_string(&stamp).unwrap(), "1000");
        // Not again within the minute, said with how long ago.
        std::fs::write(devices.join("3-1.1.4/authorized"), "untouched").unwrap();
        assert_eq!(at("3-1.1.4", 1_059).unwrap(), Outcome::TooSoon(59));
        assert_eq!(
            std::fs::read_to_string(devices.join("3-1.1.4/authorized")).unwrap(),
            "untouched"
        );
        assert_eq!(at("3-1.1.4", 1_060).unwrap(), Outcome::Reauthorized);
        // A clock set back is no reason to wait.
        assert_eq!(at("3-1.1.4", 10).unwrap(), Outcome::Reauthorized);
        // No path made of the port.
        for port in ["", "../3-1.1.4", "3-1.1.4/..", "3-1/../../x", "a-1"] {
            assert!(at(port, 5_000).is_err(), "{port:?}");
        }
        assert_eq!(
            Outcome::TooSoon(12).line(),
            r#"{"reauthorized":false,"why":"too_soon","ago_s":12}"#
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// #3504: a helper started for one port never touches the camera at
    /// another, with the same ids or not: the port is its unit's, and
    /// nothing at that port is no device, not a camera found elsewhere.
    #[test]
    fn the_helper_acts_only_at_the_port_it_was_started_with() {
        let dir = folder("port");
        let devices = dir.join("devices");
        camera_at(&devices, "3-1.3.4", "01da", "5875");
        camera_at(&devices, "3-1.2", "046d", "0825");
        let stamp = dir.join("last");
        let at = |now| reauthorize_at(&devices, "3-1.1.4", 0x01da, 0x5875, &stamp, now);
        assert_eq!(at(1_000).unwrap(), Outcome::NoDevice);
        assert!(!stamp.exists());
        for other in ["3-1.3.4", "3-1.2"] {
            assert_eq!(
                std::fs::read_to_string(devices.join(other).join("authorized")).unwrap(),
                "1\n",
                "{other}"
            );
        }
        // Once the camera is at its own port, that one only.
        camera_at(&devices, "3-1.1.4", "01da", "5875");
        assert_eq!(at(1_000).unwrap(), Outcome::Reauthorized);
        assert_eq!(
            std::fs::read_to_string(devices.join("3-1.3.4/authorized")).unwrap(),
            "1\n"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The eyes ask after the third open that did not open, about 10 s, then
    /// at most once a minute while it stays so.
    #[test]
    fn the_eyes_ask_after_three_failed_opens_then_once_a_minute() {
        assert!(!ask_now(1, None, 0));
        assert!(!ask_now(2, None, 0));
        assert!(ask_now(3, None, 0));
        assert!(!ask_now(4, Some(100), 159));
        assert!(ask_now(9, Some(100), 160));
        // About 10 s at the retry's 5 s: within #2892's 30 s.
        assert!(
            u64::from(ASK_AFTER - 1) * crate::reopen::RETRY.as_secs() <= 10,
            "asked within 10 s of the first failure"
        );
    }

    /// The eyes' side: connects, says nothing, reads one line; a helper
    /// that isn't there is an error, said, and the eyes go on.
    #[test]
    fn the_eyes_read_the_helpers_one_line() {
        let dir = folder("ask");
        let socket = dir.join("camera.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let helper = std::thread::spawn(move || {
            use std::io::Write;
            let (mut stream, _) = listener.accept().unwrap();
            let mut said = Vec::new();
            stream.read_to_end(&mut said).unwrap();
            stream.write_all(b"{\"reauthorized\":true}\n").unwrap();
            said
        });
        assert_eq!(ask(&socket).unwrap(), r#"{"reauthorized":true}"#);
        assert!(helper.join().unwrap().is_empty(), "the eyes say nothing");
        assert!(ask(&dir.join("none.sock")).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
