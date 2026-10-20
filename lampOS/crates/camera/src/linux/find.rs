//! The camera by its USB ids on any port (#2190): lamp-dbda's camera is on
//! `3-1.3.4` where lamp-4ace's and lamp-52e6's are on `3-1.1.4`. Each video
//! node's capture side (`index` 0) is followed up its own sysfs ancestry to
//! the USB device holding it, as `identity::observe_usb` does at open; only
//! nodes whose device has the ids are taken. The open then checks the node's
//! identity as before, port included.

use crate::PortError;
use std::path::{Path, PathBuf};

/// Why no one camera was found.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NotFound {
    /// No capture node with those ids.
    None,
    /// This many, each on its own port: one must be named.
    Several(Vec<String>),
}

/// The one capture node whose USB device has `vendor` and `product`, under
/// `sys` (`/sys`) with its node under `dev` (`/dev`): the node and its
/// device's topology.
pub(super) fn find_under(
    sys: &Path,
    dev: &Path,
    vendor: u16,
    product: u16,
) -> Result<Result<(PathBuf, String), NotFound>, PortError> {
    let class = sys.join("class/video4linux");
    let devices = std::fs::canonicalize(sys.join("devices")).map_err(io)?;
    let mut found = Vec::new();
    for entry in std::fs::read_dir(&class).map_err(io)? {
        let entry = entry.map_err(io)?;
        let name = entry.file_name();
        let Some(name) = name.to_str().filter(|name| name.starts_with("video")) else {
            continue;
        };
        let Ok(mut at) = std::fs::canonicalize(entry.path()) else {
            continue;
        };
        if !at.starts_with(&devices) || read(&at.join("index")).as_deref() != Some("0") {
            continue;
        }
        // This node's ancestry only, as at open.
        for _ in 0..16 {
            if let Some(found_vendor) = read(&at.join("idVendor")) {
                let ids = (
                    hex(&found_vendor),
                    read(&at.join("idProduct")).as_deref().and_then(hex),
                );
                if ids == (Some(vendor), Some(product))
                    && let Some(topology) = at.file_name().and_then(|n| n.to_str())
                {
                    found.push((dev.join(name), topology.to_owned()));
                }
                break;
            }
            if !at.pop() || !at.starts_with(&devices) {
                break;
            }
        }
    }
    found.sort();
    Ok(match found.len() {
        0 => Err(NotFound::None),
        1 => Ok(found.remove(0)),
        _ => Err(NotFound::Several(
            found.into_iter().map(|(_, topology)| topology).collect(),
        )),
    })
}

fn read(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    (text.len() <= 64).then(|| text.trim().to_owned())
}

fn hex(text: &str) -> Option<u16> {
    (text.len() == 4)
        .then(|| u16::from_str_radix(text, 16).ok())
        .flatten()
}

fn io(error: std::io::Error) -> PortError {
    super::errno(error.raw_os_error().unwrap_or(5))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    /// A sysfs stand-in: `video<N>` under `class/video4linux`, linked to its
    /// node under a USB device at `topology` with `ids`, capture side or not.
    fn node(sys: &Path, n: u32, topology: &str, ids: (&str, &str), index: &str) {
        let usb = sys.join("devices/platform/ehci/usb3").join(topology);
        let at = usb.join(format!("{topology}:1.0/video4linux/video{n}"));
        std::fs::create_dir_all(&at).unwrap();
        std::fs::write(usb.join("idVendor"), format!("{}\n", ids.0)).unwrap();
        std::fs::write(usb.join("idProduct"), format!("{}\n", ids.1)).unwrap();
        std::fs::write(at.join("index"), format!("{index}\n")).unwrap();
        let class = sys.join("class/video4linux");
        std::fs::create_dir_all(&class).unwrap();
        symlink(&at, class.join(format!("video{n}"))).unwrap();
    }

    /// The one camera with the ids is taken on whatever port it is on, by
    /// its capture node; its metadata node, another camera and nodes outside
    /// the devices are not it. None, or two, is said, with their ports.
    #[test]
    fn the_one_camera_with_the_ids_is_found_on_any_port() {
        let root = std::env::temp_dir().join(format!("camera-find-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (sys, dev) = (root.join("sys"), Path::new("/dev"));
        std::fs::create_dir_all(sys.join("class/video4linux")).unwrap();
        std::fs::create_dir_all(sys.join("devices")).unwrap();
        let zvcai = (0x01da, 0x5875);
        assert_eq!(
            find_under(&sys, dev, zvcai.0, zvcai.1),
            Ok(Err(NotFound::None))
        );
        // lamp-dbda's port; its metadata node beside it; another camera.
        node(&sys, 0, "3-1.3.4", ("01da", "5875"), "0");
        node(&sys, 1, "3-1.3.4", ("01da", "5875"), "1");
        node(&sys, 2, "3-1.2", ("1bcf", "28cc"), "0");
        assert_eq!(
            find_under(&sys, dev, zvcai.0, zvcai.1),
            Ok(Ok((PathBuf::from("/dev/video0"), "3-1.3.4".into())))
        );
        // A second one with the same ids: neither is taken, both ports said.
        node(&sys, 4, "3-1.1.4", ("01da", "5875"), "0");
        assert_eq!(
            find_under(&sys, dev, zvcai.0, zvcai.1),
            Ok(Err(NotFound::Several(vec![
                "3-1.3.4".into(),
                "3-1.1.4".into()
            ])))
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
