//! Where an app's parts are, in one place so a test can put them under a
//! temporary folder.
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone, Debug)]
pub struct Layout {
    /// `/opt/lampOS/apps`: `<name>/<version>/`, and `<name>/current`.
    pub code: PathBuf,
    /// `/var/lib`: where systemd makes the link `lampapp-<name>` for the unit's
    /// `StateDirectory=lampapp-<name>` (one path component, so it is a link
    /// and never a folder shared with other parts' state).
    pub data: PathBuf,
    /// `/var/lib/private`: where the data really is, behind that link.
    pub private: PathBuf,
    /// `/var/lib/lampos-apps-state`: the marks, root's.
    pub state: PathBuf,
    /// `/var/lib/lampos-apps-staging`: the folder lamp-cloud puts a package in (lampos's).
    pub staging: PathBuf,
    /// How long a second asker waits for the apps lock before it is told busy.
    pub lock_wait: Duration,
}

impl Layout {
    /// The link systemd makes for an app's data folder.
    pub fn data_link(&self, name: &str) -> PathBuf {
        self.data.join(format!("lampapp-{name}"))
    }

    /// Where an app's data really is.
    pub fn data_real(&self, name: &str) -> PathBuf {
        self.private.join(format!("lampapp-{name}"))
    }

    /// The fixed launcher the units run: root's, written by the helper.
    pub fn launcher(&self) -> PathBuf {
        self.code.join(".run.py")
    }

    /// The resolver the net units bind over `/etc/resolv.conf`: root's, written by the helper.
    pub fn resolver(&self) -> PathBuf {
        self.code.join(".resolv.conf")
    }

    pub fn product() -> Self {
        Self {
            code: "/opt/lampOS/apps".into(),
            data: "/var/lib".into(),
            private: "/var/lib/private".into(),
            state: "/var/lib/lampos-apps-state".into(),
            staging: "/var/lib/lampos-apps-staging".into(),
            lock_wait: Duration::from_secs(4),
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
            lock_wait: Duration::from_millis(300),
        }
    }
}
