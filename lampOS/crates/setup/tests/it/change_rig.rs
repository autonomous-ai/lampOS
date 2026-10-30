//! The folder and the voice's clock `change`'s tests run on: the `it`
//! binary's `change` module and `tests/update_lock.rs` both use it.
// Each binary uses part of it, so the rest is unused in that binary.
#![allow(dead_code)]
use lamp_setup::change::Ears;
use lamp_setup::serve::Config;
use lamp_update::quiet::Heard;
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

pub(crate) static NEXT: AtomicUsize = AtomicUsize::new(0);

pub(crate) struct Root(pub(crate) PathBuf);

impl Root {
    pub(crate) fn new() -> Self {
        let path = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "lamp-setup-change-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(path.join("zoneinfo/Asia")).unwrap();
        fs::create_dir_all(path.join("zoneinfo/Europe")).unwrap();
        fs::write(path.join("zoneinfo/Asia/Ho_Chi_Minh"), b"TZif").unwrap();
        fs::write(path.join("zoneinfo/Europe/Paris"), b"TZif").unwrap();
        // In the lamp's zone data, not in the parts' bundled one.
        fs::write(path.join("zoneinfo/Asia/Nowhere"), b"TZif").unwrap();
        fs::create_dir_all(path.join("etc")).unwrap();
        Self(path)
    }

    pub(crate) fn config(&self) -> Config {
        serde_json::from_value(json!({
            "listen": "127.0.0.1:5000",
            "marker": self.0.join("etc/setup-done"),
            "credential": self.0.join("etc/credential"),
            "policy_key": self.0.join("etc/jev.key"),
            "name": self.0.join("etc/person/name"),
            "country": "VN",
            "zoneinfo": self.0.join("zoneinfo"),
            "localtime": self.0.join("etc/localtime"),
            "timezone_file": self.0.join("etc/timezone"),
            "zone_env": self.0.join("etc/zone.env"),
            "backend": self.0.join("etc/backend.json"),
            "reports": self.0.join("etc/reports.json"),
            "display_name": self.0.join("etc/display-name"),
            "language": self.0.join("etc/setup/language"),
        }))
        .unwrap()
    }

    /// Every file under the folder but the zone data, with its bytes.
    pub(crate) fn files(&self) -> Vec<(PathBuf, Vec<u8>)> {
        let mut found = Vec::new();
        let mut folders = vec![self.0.join("etc")];
        while let Some(folder) = folders.pop() {
            for entry in fs::read_dir(folder).unwrap() {
                let path = entry.unwrap().path();
                let meta = fs::symlink_metadata(&path).unwrap();
                if meta.is_dir() {
                    folders.push(path);
                } else if meta.file_type().is_symlink() {
                    let target = fs::read_link(&path).unwrap();
                    found.push((path, target.into_os_string().into_encoded_bytes()));
                } else {
                    found.push((path.clone(), fs::read(&path).unwrap()));
                }
            }
        }
        found.sort();
        found
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A voice heard on a clock of the test's own: `voice/state` each second,
/// as the script says for each second.
pub(crate) struct Script {
    pub(crate) now: u64,
    pub(crate) at: Box<dyn Fn(u64) -> Vec<Heard>>,
}

impl Ears for Script {
    fn listen(&mut self, wait: Duration) -> Vec<Heard> {
        self.now += wait.as_millis() as u64;
        (self.at)(self.now)
    }
    fn now_ms(&mut self) -> u64 {
        self.now
    }
}

pub(crate) const S: u64 = 1_000;
