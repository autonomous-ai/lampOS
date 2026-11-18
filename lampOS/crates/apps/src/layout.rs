//! Where an app's parts are, in one place so a test can put them under a
//! temporary folder.
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Layout {
    /// `/opt/lampOS/apps`: `<name>/<version>/`, and `<name>/current`.
    pub code: PathBuf,
    /// `/var/lib/lampOS/apps`: `<name>`, a link systemd makes for `StateDirectory=`.
    pub data: PathBuf,
    /// `/var/lib/private/lampOS/apps`: where the data really is, behind that link.
    pub private: PathBuf,
    /// `/var/lib/lampos-apps-state`: the marks, root's.
    pub state: PathBuf,
    /// `/var/lib/lampos-apps-staging`: the folder lamp-cloud puts a package in (lampos's).
    pub staging: PathBuf,
}

impl Layout {
    pub fn product() -> Self {
        Self {
            code: "/opt/lampOS/apps".into(),
            data: "/var/lib/lampOS/apps".into(),
            private: "/var/lib/private/lampOS/apps".into(),
            state: "/var/lib/lampos-apps-state".into(),
            staging: "/var/lib/lampos-apps-staging".into(),
        }
    }

    /// The same folders under `root`, for a test.
    pub fn under(root: &Path) -> Self {
        Self {
            code: root.join("opt/apps"),
            data: root.join("var/lib/apps"),
            private: root.join("var/lib/private/apps"),
            state: root.join("state"),
            staging: root.join("staging"),
        }
    }
}
