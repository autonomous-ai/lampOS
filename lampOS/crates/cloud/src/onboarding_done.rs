//! The app told once that the lamp's setup is done (#5812, the design on
//! #1711): one unasked `data` reply on `fd_channel`, kind [`KIND`], data
//! `{"at": <unix ms>}`, as `harness.sign_in.state` is pushed. `at` is the
//! walk's own date (`lamp_msgs::OnboardingPending::done_at_unix_ms`, kept
//! through a redo, gone with a factory reset) and the app's dedupe key.
//!
//! Read off the voice's `onboarding/pending` heartbeat every [`EVERY`], sent
//! only while connected, kept until the broker's ack (sent again on the next
//! connection otherwise), and once acked written to [`FILE`] in lamp-cloud's
//! state folder, so a restart never sends it again. No date (a walk not
//! ended, or set up before it was dated) sends nothing. Off unless
//! [`ENVIRONMENT`] is `on`: Backend and Mobile have yet to confirm the
//! backend relays a new kind to the app.
use serde_json::{Value, json};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// The kind the app opens Skills on, once per `(device id, at)`.
pub const KIND: &str = "onboarding.done";

/// The switch, in lamp-cloud's unit: `on` exactly. Not in the product's
/// unit until the relay is confirmed.
pub const ENVIRONMENT: &str = "LAMP_APP_ONBOARDING_DONE";

/// The file, in `STATE_DIRECTORY`, that holds the `at` the broker acked.
pub const FILE: &str = "onboarding-done";

/// How often the heartbeat is looked at.
pub const EVERY: Duration = Duration::from_secs(1);

/// Whether the switch is on.
pub fn on() -> bool {
    std::env::var(ENVIRONMENT).is_ok_and(|value| value == "on")
}

/// The reply's `data`.
pub fn data(at: u64) -> Value {
    json!({ "at": at })
}

/// What was told of the walk's end: the date acked (from [`FILE`] at the
/// start), and the one handed to the client and not yet acked.
pub struct Done {
    file: PathBuf,
    sent: Option<u64>,
    flying: Option<u64>,
    looked: Option<Instant>,
}

impl Done {
    pub fn new(file: PathBuf) -> Self {
        let sent = std::fs::read_to_string(&file)
            .ok()
            .and_then(|text| text.trim().parse().ok());
        Self {
            file,
            sent,
            flying: None,
            looked: None,
        }
    }

    /// The date to send now, if one is known that the broker has neither
    /// acked nor holds: `read` gives the heartbeat's, at most every
    /// [`EVERY`].
    pub fn due(&mut self, now: Instant, read: impl FnOnce() -> Option<u64>) -> Option<u64> {
        if self.looked.is_some_and(|looked| now < looked + EVERY) {
            return None;
        }
        self.looked = Some(now);
        read().filter(|at| self.sent != Some(*at) && self.flying != Some(*at))
    }

    /// `at` was handed to the client.
    pub fn handed(&mut self, at: u64) {
        self.flying = Some(at);
    }

    /// The broker acked `at`: kept in [`FILE`]. The journal's line.
    pub fn acked(&mut self, at: u64) -> String {
        if self.flying == Some(at) {
            self.flying = None;
        }
        self.sent = Some(at);
        match std::fs::write(&self.file, at.to_string()) {
            Ok(()) => format!("{KIND}: acked by the broker, at {at} kept"),
            Err(error) => format!(
                "{KIND}: acked by the broker, at {at} not kept ({}): said again after a restart",
                error.kind()
            ),
        }
    }

    /// The connection went before the ack: sent again on the next one.
    pub fn lost(&mut self) {
        self.flying = None;
        self.looked = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_date_is_due_until_acked_and_again_after_a_loss() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(FILE);
        let mut done = Done::new(file.clone());
        let t = Instant::now();
        assert_eq!(done.due(t, || None), None);
        assert_eq!(done.due(t + EVERY, || Some(7)), Some(7));
        // Looked at no more often than every second.
        assert_eq!(done.due(t + EVERY, || Some(7)), None);
        done.handed(7);
        assert_eq!(done.due(t + 2 * EVERY, || Some(7)), None);
        done.lost();
        assert_eq!(done.due(t + 2 * EVERY, || Some(7)), Some(7));
        done.handed(7);
        assert!(done.acked(7).contains("kept"));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "7");
        assert_eq!(done.due(t + 3 * EVERY, || Some(7)), None);
        // A restart reads it back; a new setup's date is sent.
        let mut again = Done::new(file);
        assert_eq!(again.due(t, || Some(7)), None);
        assert_eq!(again.due(t + EVERY, || Some(8)), Some(8));
    }
}
