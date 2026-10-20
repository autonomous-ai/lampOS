//! A USB reset of the camera (#2126): after a mute, lamp-52e6's camera
//! refused every open until a reboot, a restart of lamp-eyes included. The
//! kernel's USBDEVFS_RESET on the device's own node resets its port and binds
//! its interfaces again, as an unplug would, with no reboot. The device is
//! found by the configured topology alone, and reset only when its vendor and
//! product are the expected ones: never another device. The node needs write
//! access for the eyes' user (the lamp's udev rule gives `video` the camera's
//! USB node).

use super::errno;
use crate::{PortError, linux::UsbIdentity};
use rustix::{
    fs::{Mode, OFlags},
    ioctl::{NoArg, Opcode, ioctl, opcode},
};
use std::path::{Path, PathBuf};

const USBDEVFS_RESET: Opcode = opcode::none(b'U', 20);

/// Resets the USB device at `expected`'s topology, read under `sys`
/// (`/sys/bus/usb/devices`), through its node under `dev` (`/dev/bus/usb`).
pub(super) fn reset_at(
    sys: &Path,
    dev: &Path,
    expected: &UsbIdentity,
) -> Result<PathBuf, PortError> {
    expected.validate()?;
    let device = sys.join(&expected.topology);
    let read = |name: &str| {
        std::fs::read_to_string(device.join(name))
            .map(|text| text.trim().to_owned())
            .map_err(|error| errno(error.raw_os_error().unwrap_or(5)))
    };
    let hex =
        |name: &str| u16::from_str_radix(&read(name)?, 16).map_err(|_| PortError::InvalidData);
    if hex("idVendor")? != expected.vendor || hex("idProduct")? != expected.product {
        return Err(PortError::InvalidData);
    }
    let number = |name: &str| {
        read(name)?
            .parse::<u16>()
            .map_err(|_| PortError::InvalidData)
    };
    let node = dev
        .join(format!("{:03}", number("busnum")?))
        .join(format!("{:03}", number("devnum")?));
    let fd = rustix::fs::open(&node, OFlags::WRONLY | OFlags::CLOEXEC, Mode::empty())
        .map_err(|error| errno(error.raw_os_error()))?;
    // SAFETY: USBDEVFS_RESET takes no argument; the descriptor is a file
    // opened here and alive for the call.
    #[allow(unsafe_code)]
    unsafe { ioctl(&fd, NoArg::<USBDEVFS_RESET>::new()) }
        .map_err(|error| errno(error.raw_os_error()))?;
    Ok(node)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usb() -> UsbIdentity {
        UsbIdentity {
            vendor: 0x01da,
            product: 0x5875,
            topology: "3-1.1.4".into(),
            serial: None,
            interface_number: 0,
            capture_index: 0,
        }
    }

    /// The device is found by topology and reset only when it is the
    /// expected camera: another vendor or product, or no device there, is
    /// refused before any node is opened. The node is the one its bus and
    /// device numbers name.
    #[test]
    fn only_the_expected_camera_at_its_topology_is_reset_through_its_own_node() {
        let root = std::env::temp_dir().join(format!("camera-reset-{}", std::process::id()));
        let (sys, dev) = (root.join("sys"), root.join("dev"));
        let device = sys.join("3-1.1.4");
        std::fs::create_dir_all(&device).unwrap();
        std::fs::create_dir_all(dev.join("003")).unwrap();
        let write = |name: &str, value: &str| {
            std::fs::write(device.join(name), format!("{value}\n")).unwrap()
        };
        for (name, value) in [
            ("idVendor", "01da"),
            ("idProduct", "5875"),
            ("busnum", "3"),
            ("devnum", "7"),
        ] {
            write(name, value);
        }
        // No node yet: refused at the open, by its errno.
        assert_eq!(reset_at(&sys, &dev, &usb()), Err(PortError::Io(2)));
        // A plain file as the node: opened, and the ioctl is refused (ENOTTY),
        // which shows the reset went to bus 003 device 007.
        std::fs::write(dev.join("003/007"), b"").unwrap();
        assert_eq!(reset_at(&sys, &dev, &usb()), Err(PortError::Io(25)));
        write("idProduct", "5876");
        assert_eq!(reset_at(&sys, &dev, &usb()), Err(PortError::InvalidData));
        write("idProduct", "5875");
        write("idVendor", "1bcf");
        assert_eq!(reset_at(&sys, &dev, &usb()), Err(PortError::InvalidData));
        let elsewhere = UsbIdentity {
            topology: "3-1.1.3".into(),
            ..usb()
        };
        assert_eq!(reset_at(&sys, &dev, &elsewhere), Err(PortError::Io(2)));
        let unsafe_topology = UsbIdentity {
            topology: "../3-1.1.4".into(),
            ..usb()
        };
        assert_eq!(
            reset_at(&sys, &dev, &unsafe_topology),
            Err(PortError::InvalidData)
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
