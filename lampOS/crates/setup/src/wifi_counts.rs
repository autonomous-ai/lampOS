//! The Wi-Fi joins of setup, counted for the health report (#6256 F8,
//! Backend's B10 `wifi_setup`; `docs/cloud.md`, "Report counters").
//! Running totals since the last factory reset, in [`FILE`] (0644, in the
//! reset plan), which `lamp-cloud` reads at a health report and never sends
//! after the person's no: they wait here until the lamp's first report goes
//! through, so a lamp that joined after some failures still tells of them.
//! Numbers only: no network name, password or reason text. Off unless
//! [`ENVIRONMENT`] is `on` in setup's environment.
use crate::wifi::{Joined, Reason};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// `on` counts; anything else: nothing is written.
pub const ENVIRONMENT: &str = "LAMP_REPORT_COUNTERS";
/// Where the totals are, beside setup's record of the reports answer.
pub const FILE: &str = "/etc/lampOS/setup/wifi-setup.json";

/// The totals, by B10's names. A field a later build adds is dropped on
/// read; one missing is 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Totals {
    pub attempts: u64,
    pub ok: u64,
    pub fail_password: u64,
    pub fail_no_ip: u64,
    pub fail_other: u64,
}

impl Totals {
    /// One join's outcome added.
    pub fn add(&mut self, joined: &Joined) {
        self.attempts = self.attempts.saturating_add(1);
        let field = match joined {
            Joined::Connected => &mut self.ok,
            Joined::Failed {
                reason: Reason::WrongPassword,
            } => &mut self.fail_password,
            Joined::Failed {
                reason: Reason::NoDhcp,
            } => &mut self.fail_no_ip,
            Joined::Failed { .. } => &mut self.fail_other,
        };
        *field = field.saturating_add(1);
    }

    /// In B10's order: attempts, ok, fail_password, fail_no_ip, fail_other.
    pub fn as_array(&self) -> [u64; 5] {
        [
            self.attempts,
            self.ok,
            self.fail_password,
            self.fail_no_ip,
            self.fail_other,
        ]
    }
}

/// Whether the switch is on in this environment.
pub fn on() -> bool {
    std::env::var(ENVIRONMENT).is_ok_and(|value| value == "on")
}

/// The totals in `path`; zero when there is no file or it can't be read.
/// At most 4 KiB is read.
pub fn read(path: &Path) -> Totals {
    use std::io::Read;
    let mut text = String::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(4_096).read_to_string(&mut text))
        .ok()
        .and_then(|_| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// `joined` added to the totals in `path`, written whole (temporary file,
/// sync, rename), 0644 so `lamp-cloud` reads it.
pub fn count(path: &Path, joined: &Joined) -> std::io::Result<()> {
    let mut totals = read(path);
    totals.add(joined);
    let bytes = serde_json::to_vec(&totals).map_err(std::io::Error::other)?;
    let temporary = crate::serve::stage(path, &bytes, 0o644)?;
    crate::serve::put_in_place(&temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn each_join_is_counted_by_b10s_names_and_kept_across_reads() {
        let folder = std::env::temp_dir().join(format!("wifi-counts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let path = folder.join("wifi-setup.json");
        assert_eq!(read(&path), Totals::default());
        for joined in [
            Joined::Failed {
                reason: Reason::WrongPassword,
            },
            Joined::Failed {
                reason: Reason::NoDhcp,
            },
            Joined::Failed {
                reason: Reason::SsidNotFound,
            },
            Joined::Connected,
        ] {
            count(&path, &joined).unwrap();
        }
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            r#"{"attempts":4,"ok":1,"fail_password":1,"fail_no_ip":1,"fail_other":1}"#
        );
        assert_eq!(read(&path).as_array(), [4, 1, 1, 1, 1]);
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o644
        );
        // A later build's field is dropped, a missing one is 0, a bad file is 0.
        std::fs::write(&path, r#"{"attempts":2,"ok":2,"later":7}"#).unwrap();
        assert_eq!(read(&path).as_array(), [2, 2, 0, 0, 0]);
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(read(&path), Totals::default());
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_file_is_wiped_by_a_factory_reset() {
        let plan: serde_json::Value =
            serde_json::from_str(include_str!("../fixtures/reset-plan-product.json")).unwrap();
        assert!(
            plan["wipe"]
                .as_array()
                .unwrap()
                .iter()
                .any(|path| path == FILE)
        );
    }
}
