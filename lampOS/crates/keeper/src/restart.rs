//! A restart of the lamp asked from outside it (#2193): `lamp-cloud` asks
//! for the app's `system.reboot` on [`SOCKET`], and `lamp-keeper
//! restart-asked`, root's helper started for that one connection, answers
//! with [`answer`]. Refused while an update installs; otherwise answered
//! first and then, once the lamp is quiet, `lampos.target` is restarted
//! (never the machine). `docs/keeper.md`, "A restart asked from the app".
use serde_json::{Value, json};

/// The helper's socket (`deploy/product/lamp-keeper-restart.socket`).
pub const SOCKET: &str = "/run/lampos-keeper-restart/restart.sock";

/// The one request taken, as one line.
pub const REQUEST: &str = r#"{"restart":"lamp"}"#;

/// Why a restart is refused while lamp-update holds its lock.
pub const INSTALLING: &str = "an update is being installed: try again once it is done";

/// The reply line to `line`, and whether the lamp is then restarted.
/// `update` is `Err(why)` while an update installs or its lock could not be
/// checked: refused either way, never a restart under an update.
pub fn answer(line: &str, update: Result<(), String>) -> (String, bool) {
    let refused = |why: &str| (json!({ "refused": why }).to_string(), false);
    let asked = serde_json::from_str::<Value>(line.trim()).ok();
    if asked != serde_json::from_str::<Value>(REQUEST).ok() {
        return refused("not a restart request");
    }
    if let Err(why) = update {
        return refused(&why);
    }
    (json!({"done": {"restarting": true}}).to_string(), true)
}

/// The helper's answer to `line`, read against lamp-update's lock in
/// `records`, taken without waiting: the reply line, and the lock to hold
/// through the restart when there is one. Refused with [`INSTALLING`] while
/// another holder has the lock, and refused too when it can't be checked.
/// Only with `prepare`: lamp-cloud takes the library without lamp-update.
#[cfg(feature = "prepare")]
pub fn asked(
    line: &str,
    records: &lamp_update::records::Records,
) -> (String, Option<lamp_update::records::Lock>) {
    let lock = records.lock().map_err(|why| {
        if why.kind() == std::io::ErrorKind::WouldBlock {
            INSTALLING.to_owned()
        } else {
            format!("the update's state could not be checked: {why}")
        }
    });
    let update = lock.as_ref().map(|_| ()).map_err(Clone::clone);
    let (reply, go) = answer(line, update);
    (reply, lock.ok().filter(|_| go))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_restart_is_answered_first_and_refused_under_an_update() {
        let (reply, go) = answer(&format!("{REQUEST}\n"), Ok(()));
        assert!(go);
        assert_eq!(reply, r#"{"done":{"restarting":true}}"#);
        let (reply, go) = answer(REQUEST, Err(INSTALLING.into()));
        assert!(!go);
        assert_eq!(reply, json!({ "refused": INSTALLING }).to_string());
        for line in ["", "{}", r#"{"restart":"machine"}"#, "reboot"] {
            assert!(!answer(line, Ok(())).1, "{line}");
        }
    }

    /// The helper's own lock read: refused while another lamp-update holds
    /// the lock, and a restart, holding it, once it is let go.
    #[cfg(feature = "prepare")]
    #[test]
    fn a_restart_is_refused_while_another_holder_has_the_update_lock() {
        let folder = std::env::temp_dir().join(format!("lkr-{}", std::process::id()));
        let records = lamp_update::records::Records::new(folder.clone());
        let held = records.lock().unwrap();
        let (reply, lock) = asked(REQUEST, &records);
        assert_eq!(reply, json!({ "refused": INSTALLING }).to_string());
        assert!(lock.is_none());
        drop(held);
        let (reply, lock) = asked(REQUEST, &records);
        assert_eq!(reply, r#"{"done":{"restarting":true}}"#);
        assert!(lock.is_some());
        // Held by the restart: an update that starts meanwhile can't have it.
        assert!(records.lock().is_err());
        drop(lock);
        let _ = std::fs::remove_dir_all(folder);
    }
}
