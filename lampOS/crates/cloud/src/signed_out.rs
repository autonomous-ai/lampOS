//! The lamp signed out (`LAMP_SIGNED_OUT`, `docs/cloud.md`, "Signed out"): a
//! lamp removed and added again keeps running on its old login, and the ping
//! sends that same login, so it can't get a new one by itself. Several
//! rejected pings in a row are this state; only a rejection counts. A timeout,
//! a failed connection or a failing backend says nothing about the login, so
//! it neither counts nor starts the count again. Nothing is wiped and setup is
//! not reset: the state ends when a ping is accepted, or at a restart with a
//! new login (setup's writes reach this process only through a restart, as
//! its credentials are handed in at start).
use lamp_msgs::AccountState;
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::watch;

/// The switch, exactly `on`. Off unless set.
pub const ENVIRONMENT: &str = "LAMP_SIGNED_OUT";

/// Rejected pings in a row that mean signed out: at one every 15 s, the twelfth
/// comes 2 min 45 s after the first. A login check that fails for a minute or
/// two, as when the backend restarts, must not make the lamp tell a person to
/// set it up again, and past that the lamp has been silent long enough that
/// saying why is what matters. Turns refused meanwhile have the voice's own
/// handling.
pub const REJECTED_IN_A_ROW: u32 = 12;

/// How often the state is said on the bus; its lifespan is three of these.
const SAY_EVERY: Duration = Duration::from_secs(5);

/// The answer the backend refuses a login with. Nothing else is a rejection:
/// no other refusal says it is this login that is no longer accepted.
const REJECTED_STATUS: u16 = 401;

pub fn on_from_environment() -> bool {
    std::env::var(ENVIRONMENT).ok().as_deref() == Some("on")
}

/// What one ping's answer says of the login.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    /// The backend took the login.
    Accepted,
    /// The backend refused the login.
    Rejected,
    /// Nothing about the login: neither counts nor starts the count again.
    Neither,
}

impl Answer {
    /// The answer of a ping that got this status, not a success.
    pub fn of_status(status: u16) -> Self {
        if status == REJECTED_STATUS {
            Self::Rejected
        } else {
            Self::Neither
        }
    }

    /// The answer of a ping that got a success with this body: one that is
    /// no JSON is not the backend's.
    pub fn of_body(body: &[u8]) -> Self {
        if serde_json::from_slice::<serde_json::Value>(body).is_ok() {
            Self::Accepted
        } else {
            Self::Neither
        }
    }
}

/// What is kept across a restart: which login was signed out, and since when.
/// The login only as a short fingerprint, enough to tell a new one.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Kept {
    login: String,
    since_s: u64,
}

fn fingerprint(key: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn read_kept(path: &Path) -> Option<Kept> {
    let text = std::fs::read(path).ok()?;
    serde_json::from_slice(&text).ok()
}

fn write_kept(path: &Path, kept: &Kept) -> std::io::Result<()> {
    let temporary = path.with_extension("tmp");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&temporary)?;
    file.write_all(&serde_json::to_vec(kept)?)?;
    file.sync_all()?;
    std::fs::rename(&temporary, path)
}

/// Counts the ping's answers. The spell's start, in wall seconds, is what
/// others read: `None` while signed in.
pub struct Login {
    fingerprint: String,
    kept: Option<PathBuf>,
    in_a_row: u32,
    since: watch::Sender<Option<u64>>,
}

impl Login {
    /// At start: a spell kept for this same login goes on, so a restart
    /// says nothing again and waits for nothing; one kept for another login
    /// ended when that new login arrived.
    pub fn open(key: &str, kept: Option<PathBuf>, say: &mut impl FnMut(&str)) -> Self {
        let fingerprint = fingerprint(key);
        let mut since = None;
        if let Some(path) = &kept
            && let Some(found) = read_kept(path)
        {
            if found.login == fingerprint {
                since = Some(found.since_s);
                say("this lamp is still signed out: the same login as before the restart");
            } else {
                let _ = std::fs::remove_file(path);
                say("a new login: this lamp is no longer signed out");
            }
        }
        Self {
            fingerprint,
            kept,
            in_a_row: 0,
            since: watch::channel(since).0,
        }
    }

    pub fn signed_out(&self) -> bool {
        self.since.borrow().is_some()
    }

    /// The spell's start, for [`tell`].
    pub fn watch(&self) -> watch::Receiver<Option<u64>> {
        self.since.subscribe()
    }

    /// Takes one ping's answer, at wall second `now_s`.
    pub fn observe(&mut self, answer: Answer, now_s: u64, say: &mut impl FnMut(&str)) {
        match answer {
            Answer::Neither => {}
            Answer::Accepted => {
                self.in_a_row = 0;
                if self.signed_out() {
                    self.since.send_replace(None);
                    if let Some(path) = &self.kept {
                        let _ = std::fs::remove_file(path);
                    }
                    say("a ping was accepted: this lamp is no longer signed out");
                }
            }
            Answer::Rejected => {
                self.in_a_row = self.in_a_row.saturating_add(1);
                if self.in_a_row >= REJECTED_IN_A_ROW && !self.signed_out() {
                    self.since.send_replace(Some(now_s));
                    say(&format!(
                        "this lamp is signed out: {} pings in a row were rejected; nothing is wiped, it waits for a new login",
                        self.in_a_row
                    ));
                    if let Some(path) = &self.kept {
                        let kept = Kept {
                            login: self.fingerprint.clone(),
                            since_s: now_s,
                        };
                        if write_kept(path, &kept).is_err() {
                            say("the signed-out state could not be kept: it lasts until a restart");
                        }
                    }
                }
            }
        }
    }
}

/// What the bus is told of a spell that began at `since`.
pub fn state(since: Option<u64>, now_s: u64) -> AccountState {
    AccountState {
        signed_out: since.is_some(),
        for_s: since.map_or(0, |since| now_s.saturating_sub(since)),
    }
}

/// The wall clock, in seconds.
pub fn now_s() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// Says the state on `account/state` when it changes and every few seconds.
/// Without a bus, or a topic it can publish, it only waits.
pub async fn tell(mut since: watch::Receiver<Option<u64>>, bus: Option<lamp_bus::Bus>) {
    let publisher = bus.and_then(|bus| {
        bus.publisher_declared(
            AccountState::TOPIC,
            &AccountState::declaration(),
            "lamp-cloud",
        )
        .ok()
    });
    let Some(mut publisher) = publisher else {
        return std::future::pending().await;
    };
    loop {
        let now = state(*since.borrow_and_update(), now_s());
        let _ = publisher.publish_json(lamp_ipc::monotonic_us(), &now);
        if let Ok(Err(_)) = tokio::time::timeout(SAY_EVERY, since.changed()).await {
            return std::future::pending().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn login(kept: Option<PathBuf>) -> Login {
        Login::open("the-key", kept, &mut |_| {})
    }

    fn observe(login: &mut Login, answer: Answer, times: u32) {
        for _ in 0..times {
            login.observe(answer, 1_000, &mut |_| {});
        }
    }

    #[test]
    fn only_the_rejected_answer_counts() {
        assert_eq!(Answer::of_status(401), Answer::Rejected);
        for status in [
            300, 301, 400, 402, 403, 404, 408, 429, 500, 502, 503, 504, 599,
        ] {
            assert_eq!(Answer::of_status(status), Answer::Neither, "{status}");
        }
    }

    #[test]
    fn a_success_that_is_not_the_backends_says_nothing() {
        assert_eq!(Answer::of_body(br#"{"status":"ok"}"#), Answer::Accepted);
        assert_eq!(Answer::of_body(br#"{"status":1}"#), Answer::Accepted);
        assert_eq!(Answer::of_body(b"<html>"), Answer::Neither);
        assert_eq!(Answer::of_body(b""), Answer::Neither);
    }

    #[test]
    fn the_twelfth_rejected_ping_in_a_row_is_signed_out() {
        let mut login = login(None);
        observe(&mut login, Answer::Rejected, REJECTED_IN_A_ROW - 1);
        assert!(!login.signed_out());
        observe(&mut login, Answer::Rejected, 1);
        assert!(login.signed_out());
    }

    #[test]
    fn what_says_nothing_neither_counts_nor_starts_the_count_again() {
        let mut login = login(None);
        observe(&mut login, Answer::Rejected, REJECTED_IN_A_ROW - 1);
        // Not a one: however many.
        observe(&mut login, Answer::Neither, 100);
        assert!(!login.signed_out());
        observe(&mut login, Answer::Rejected, 1);
        assert!(login.signed_out());
        // And it never ends a spell.
        observe(&mut login, Answer::Neither, 100);
        assert!(login.signed_out());
    }

    #[test]
    fn an_accepted_ping_starts_the_count_again_and_ends_a_spell() {
        let mut login = login(None);
        observe(&mut login, Answer::Rejected, REJECTED_IN_A_ROW - 1);
        observe(&mut login, Answer::Accepted, 1);
        observe(&mut login, Answer::Rejected, REJECTED_IN_A_ROW - 1);
        assert!(!login.signed_out());
        observe(&mut login, Answer::Rejected, 1);
        assert!(login.signed_out());
        observe(&mut login, Answer::Accepted, 1);
        assert!(!login.signed_out());
    }

    #[test]
    fn a_spell_keeps_its_start_while_more_pings_are_rejected() {
        let mut login = login(None);
        observe(&mut login, Answer::Rejected, REJECTED_IN_A_ROW);
        let since = *login.watch().borrow();
        login.observe(Answer::Rejected, 9_000, &mut |_| {});
        assert_eq!(*login.watch().borrow(), since);
        assert_eq!(since, Some(1_000));
    }

    #[test]
    fn a_restart_keeps_the_spell_for_the_same_login_and_a_new_login_ends_it() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("signed-out.json");
        let mut first = login(Some(path.clone()));
        observe(&mut first, Answer::Rejected, REJECTED_IN_A_ROW);
        assert!(path.exists());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("the-key"), "{text}");

        // The same login: still signed out at once, from the same start.
        let again = login(Some(path.clone()));
        assert!(again.signed_out());
        assert_eq!(*again.watch().borrow(), Some(1_000));

        // A new login: signed in, and the record is gone.
        let new = Login::open("another-key", Some(path.clone()), &mut |_| {});
        assert!(!new.signed_out());
        assert!(!path.exists());
    }

    #[test]
    fn an_accepted_ping_removes_the_kept_spell() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("signed-out.json");
        let mut first = login(Some(path.clone()));
        observe(&mut first, Answer::Rejected, REJECTED_IN_A_ROW);
        observe(&mut first, Answer::Accepted, 1);
        assert!(!path.exists());
        assert!(!login(Some(path)).signed_out());
    }

    #[test]
    fn the_bus_is_told_how_long_a_spell_has_lasted() {
        assert_eq!(state(None, 50), AccountState::default());
        assert_eq!(
            state(Some(40), 50),
            AccountState {
                signed_out: true,
                for_s: 10
            }
        );
        // A clock set back is a young spell, never a wrapped one.
        assert_eq!(state(Some(60), 50).for_s, 0);
    }
}
