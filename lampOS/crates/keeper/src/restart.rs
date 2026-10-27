//! A restart or a shutdown of the lamp asked from outside it: `lamp-cloud`
//! asks for the app's `system.reboot` (#2193) or `system.shutdown` (#3697)
//! on [`SOCKET`], and `lamp-keeper restart-asked`, root's helper started for
//! that one connection, answers with [`answer`]. Refused while an update
//! installs; otherwise answered first, and then either `lampos.target` is
//! restarted once the lamp is quiet (never the machine), or [`SHUTDOWN`] is
//! run. `docs/keeper.md`, "A restart asked from the app".
use serde_json::{Value, json};

/// The helper's socket (`deploy/product/lamp-keeper-restart.socket`).
pub const SOCKET: &str = "/run/lampos-keeper-restart/restart.sock";

/// The restart request, as one line.
pub const REQUEST: &str = r#"{"restart":"lamp"}"#;

/// The other request taken: the lamp turned off (#3697, `docs/shutdown.md`).
pub const SHUTDOWN_REQUEST: &str = r#"{"shutdown":"lamp"}"#;

/// What a shutdown runs, as root: `lamp-setup` hands its runner to a unit
/// of its own (`lamp-setup-shutdown`), which takes the update's lock itself.
pub const SHUTDOWN: [&str; 2] = ["/opt/lampOS/setup/current/lamp-setup", "shutdown"];

/// What the helper was asked, once it has said yes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    Restart,
    Shutdown,
}

/// Why a restart or a shutdown is refused while lamp-update holds its lock.
pub const INSTALLING: &str = "an update is being installed: try again once it is done";

/// The reply line to `line`, and what is then done, if anything.
/// `update` is `Err(why)` while an update installs or its lock could not be
/// checked: refused either way, never a restart or a shutdown under an
/// update.
pub fn answer(line: &str, update: Result<(), String>) -> (String, Option<Asked>) {
    let refused = |why: &str| (json!({ "refused": why }).to_string(), None);
    let asked = serde_json::from_str::<Value>(line.trim()).ok();
    let is = |request: &str| asked.is_some() && asked == serde_json::from_str(request).ok();
    let (asked, done) = if is(REQUEST) {
        (Asked::Restart, json!({"done": {"restarting": true}}))
    } else if is(SHUTDOWN_REQUEST) {
        (Asked::Shutdown, json!({"done": {"shutting_down": true}}))
    } else {
        return refused("not a restart or shutdown request");
    };
    if let Err(why) = update {
        return refused(&why);
    }
    (done.to_string(), Some(asked))
}

/// The helper's answer to `line`, read against lamp-update's lock in
/// `records`, taken without waiting: the reply line, and what was asked
/// with the lock to hold while it is answered. Refused with [`INSTALLING`]
/// while another holder has the lock, and refused too when it can't be
/// checked. Only with `prepare`: lamp-cloud takes the library without
/// lamp-update.
#[cfg(feature = "prepare")]
pub fn asked(
    line: &str,
    records: &lamp_update::records::Records,
) -> (String, Option<(Asked, lamp_update::records::Lock)>) {
    let lock = records.lock().map_err(|why| {
        if why.kind() == std::io::ErrorKind::WouldBlock {
            INSTALLING.to_owned()
        } else {
            format!("the update's state could not be checked: {why}")
        }
    });
    let update = lock.as_ref().map(|_| ()).map_err(Clone::clone);
    let (reply, asked) = answer(line, update);
    (reply, asked.zip(lock.ok()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_restart_is_answered_first_and_refused_under_an_update() {
        let (reply, go) = answer(&format!("{REQUEST}\n"), Ok(()));
        assert_eq!(go, Some(Asked::Restart));
        assert_eq!(reply, r#"{"done":{"restarting":true}}"#);
        let (reply, go) = answer(REQUEST, Err(INSTALLING.into()));
        assert_eq!(go, None);
        assert_eq!(reply, json!({ "refused": INSTALLING }).to_string());
        for line in [
            "",
            "{}",
            "null",
            r#"{"restart":"machine"}"#,
            r#"{"shutdown":"now"}"#,
            "reboot",
        ] {
            assert_eq!(answer(line, Ok(())).1, None, "{line}");
        }
    }

    /// #3697: a shutdown is answered first, and refused under an update.
    #[test]
    fn a_shutdown_is_answered_first_and_refused_under_an_update() {
        let (reply, go) = answer(&format!(" {SHUTDOWN_REQUEST}\n"), Ok(()));
        assert_eq!(go, Some(Asked::Shutdown));
        assert_eq!(reply, r#"{"done":{"shutting_down":true}}"#);
        let (reply, go) = answer(SHUTDOWN_REQUEST, Err(INSTALLING.into()));
        assert_eq!(go, None);
        assert_eq!(reply, json!({ "refused": INSTALLING }).to_string());
    }

    /// The helper's own lock read: refused while another lamp-update holds
    /// the lock, and a restart, holding it, once it is let go.
    #[cfg(feature = "prepare")]
    #[test]
    fn a_restart_is_refused_while_another_holder_has_the_update_lock() {
        let folder = std::env::temp_dir().join(format!("lkr-{}", std::process::id()));
        let records = lamp_update::records::Records::new(folder.clone());
        let held = records.lock().unwrap();
        for request in [REQUEST, SHUTDOWN_REQUEST] {
            let (reply, lock) = asked(request, &records);
            assert_eq!(reply, json!({ "refused": INSTALLING }).to_string());
            assert!(lock.is_none());
        }
        drop(held);
        let (reply, lock) = asked(REQUEST, &records);
        assert_eq!(reply, r#"{"done":{"restarting":true}}"#);
        assert_eq!(lock.as_ref().map(|(asked, _)| *asked), Some(Asked::Restart));
        // Held by the restart: an update that starts meanwhile can't have it.
        assert!(records.lock().is_err());
        drop(lock);
        let _ = std::fs::remove_dir_all(folder);
    }
}
