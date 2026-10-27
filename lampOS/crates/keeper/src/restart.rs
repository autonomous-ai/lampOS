//! The lamp restarted or turned off from outside it (#2193, #3697): the
//! requests, their answers and the update lock they are refused under, one
//! copy for every root helper that takes them. `lamp-setup change` (the
//! change helper every lamp has) takes both from `lamp-cloud`; `lamp-keeper
//! restart-asked`, root's helper on [`SOCKET`] where a keeper runs, takes
//! the restart. Refused while an update installs; otherwise answered first
//! and then acted on: `lampos.target` restarted once the lamp is quiet
//! (never the machine), or `lamp-setup shutdown` started.
//! `docs/keeper.md`, "A restart asked from the app"; `docs/cloud.md`.
use serde_json::{Value, json};

/// The keeper's helper socket (`deploy/product/lamp-keeper-restart.socket`).
pub const SOCKET: &str = "/run/lampos-keeper-restart/restart.sock";

/// The restart request, as one line.
pub const REQUEST: &str = r#"{"restart":"lamp"}"#;

/// The shutdown request, as one line (#3697).
pub const SHUTDOWN_REQUEST: &str = r#"{"shutdown":"lamp"}"#;

/// Why either is refused while lamp-update holds its lock.
pub const INSTALLING: &str = "an update is being installed: try again once it is done";

/// What is asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Power {
    /// `lampos.target` restarted (`system.reboot`).
    Restart,
    /// `lamp-setup shutdown` (`system.shutdown`).
    Shutdown,
}

impl Power {
    /// Which one `line` asks, exactly; `None` for anything else.
    pub fn of(line: &str) -> Option<Self> {
        let asked = serde_json::from_str::<Value>(line.trim()).ok()?;
        [(REQUEST, Self::Restart), (SHUTDOWN_REQUEST, Self::Shutdown)]
            .into_iter()
            .find(|(request, _)| {
                serde_json::from_str::<Value>(request).ok().as_ref() == Some(&asked)
            })
            .map(|(_, power)| power)
    }

    /// The reply, and whether to act. `update` is `Err(why)` while an
    /// update installs or its lock could not be checked: refused either
    /// way, never under an update.
    pub fn reply(self, update: Result<(), String>) -> (Value, bool) {
        match update {
            Err(why) => (json!({ "refused": why }), false),
            Ok(()) => match self {
                Self::Restart => (json!({"done": {"restarting": true}}), true),
                Self::Shutdown => (json!({"done": {"shutting_down": true}}), true),
            },
        }
    }
}

/// The keeper's reply line to `line`, and whether the lamp is then
/// restarted: the restart only.
pub fn answer(line: &str, update: Result<(), String>) -> (String, bool) {
    match Power::of(line) {
        Some(Power::Restart) => {
            let (reply, go) = Power::Restart.reply(update);
            (reply.to_string(), go)
        }
        _ => (
            json!({ "refused": "not a restart request" }).to_string(),
            false,
        ),
    }
}

/// lamp-update's lock in `records`, taken without waiting, for the caller
/// to hold while it acts. Refused with [`INSTALLING`] while another holder
/// has it, and refused too when it can't be checked. Only with `prepare`:
/// lamp-cloud takes the library without lamp-update.
#[cfg(feature = "prepare")]
pub fn update_lock(
    records: &lamp_update::records::Records,
) -> Result<lamp_update::records::Lock, String> {
    records.lock().map_err(|why| {
        if why.kind() == std::io::ErrorKind::WouldBlock {
            INSTALLING.to_owned()
        } else {
            format!("the update's state could not be checked: {why}")
        }
    })
}

/// The keeper's answer to `line`, read against lamp-update's lock in
/// `records`: the reply line, and the lock to hold through the restart
/// when there is one.
#[cfg(feature = "prepare")]
pub fn asked(
    line: &str,
    records: &lamp_update::records::Records,
) -> (String, Option<lamp_update::records::Lock>) {
    let lock = update_lock(records);
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
        // The keeper takes the restart only.
        for line in [
            "",
            "{}",
            r#"{"restart":"machine"}"#,
            "reboot",
            SHUTDOWN_REQUEST,
        ] {
            assert!(!answer(line, Ok(())).1, "{line}");
        }
    }

    #[test]
    fn each_request_is_known_exactly_and_answered_first() {
        assert_eq!(Power::of(REQUEST), Some(Power::Restart));
        assert_eq!(
            Power::of(&format!(" {SHUTDOWN_REQUEST}\n")),
            Some(Power::Shutdown)
        );
        for line in [
            r#"{"shutdown":"machine"}"#,
            r#"{"shutdown":"lamp","x":1}"#,
            "{}",
        ] {
            assert_eq!(Power::of(line), None, "{line}");
        }
        assert_eq!(
            Power::Shutdown.reply(Ok(())),
            (json!({"done": {"shutting_down": true}}), true)
        );
        assert_eq!(
            Power::Shutdown.reply(Err(INSTALLING.into())),
            (json!({ "refused": INSTALLING }), false)
        );
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
