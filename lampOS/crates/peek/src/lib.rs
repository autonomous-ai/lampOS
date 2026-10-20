//! `lamp-peek`: a read-only page on an office lamp, for builders and
//! testers to see what the lamp sees and is doing (#2141, docs/peek.md).
//! It came out of `where_you_sit` failing twice on lamp-52e6 on 20 Oct
//! 2026: the person sat to the side, which only a Mac's camera pointed at
//! the lamp showed.
//!
//! Three views and nothing else:
//! - `/cam`: the eyes' faces as boxes on a plain frame, never a picture,
//!   and nothing while the camera is closed ([`view::cam_svg`]);
//! - `/bus`: the kept values and the events of a few topics, counts and
//!   states, never words ([`watch`]);
//! - `/status`: each unit, its running build against its release, and its
//!   last error line ([`units`]).
//!
//! It reads and never writes: no publisher, no setting, no button. Its one
//! mark on the lamp is the subscriber socket each reader of an event topic
//! has.
use std::net::SocketAddr;

pub mod serve;
pub mod units;
pub mod view;
pub mod watch;

/// The switch: the page runs only when this is `on`.
pub const SWITCH: &str = "LAMP_PEEK";
/// Where it answers, as `address:port`.
pub const LISTEN: &str = "LAMP_PEEK_LISTEN";
/// This machine only, unless told otherwise.
pub const DEFAULT_LISTEN: &str = "127.0.0.1:8077";
/// The credential (`LoadCredential=peek-token:...`) that lets the page
/// answer beyond this machine.
pub const TOKEN_CREDENTIAL: &str = "peek-token";
/// The shortest token taken: one a person could guess is no token.
pub const LEAST_TOKEN_CHARS: usize = 16;
/// Where the voice keeps its sessions, for the end-of-speech worker's state.
pub const SESSIONS: &str = "LAMP_PEEK_SESSIONS";
pub const DEFAULT_SESSIONS: &str = "/var/lib/lampOS/voice/sessions";

/// Whether the switch is on: `on` and nothing else.
pub fn switched_on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// Whether the page may answer on `listen`: this machine always, anywhere
/// else only with a token. A token given is asked for wherever it listens,
/// and goes in a cookie, so it is letters, digits, `-`, `_` and `.` only.
pub fn check_listen(listen: SocketAddr, token: Option<&str>) -> Result<(), String> {
    match token {
        Some(token) if token.chars().count() < LEAST_TOKEN_CHARS => Err(format!(
            "the token is under {LEAST_TOKEN_CHARS} characters: refused"
        )),
        Some(token)
            if !token
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte)) =>
        {
            Err("the token has other than letters, digits, '-', '_' and '.': refused".into())
        }
        Some(_) => Ok(()),
        None if listen.ip().is_loopback() => Ok(()),
        None => Err(format!(
            "{listen} is open to the network and no token was given \
             ({TOKEN_CREDENTIAL} by LoadCredential): refused"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> SocketAddr {
        text.parse().unwrap()
    }

    #[test]
    fn only_on_switches_it_on() {
        assert!(switched_on(Some("on")));
        for value in [None, Some(""), Some("1"), Some("ON"), Some("off")] {
            assert!(!switched_on(value), "{value:?}");
        }
    }

    #[test]
    fn beyond_this_machine_a_token_is_needed() {
        assert!(check_listen(at(DEFAULT_LISTEN), None).is_ok());
        assert!(check_listen(at("[::1]:8077"), None).is_ok());
        let refused = check_listen(at("0.0.0.0:8077"), None).unwrap_err();
        assert!(refused.contains("no token"), "{refused}");
        assert!(check_listen(at("192.168.1.20:8077"), None).is_err());
        assert!(check_listen(at("0.0.0.0:8077"), Some("short")).is_err());
        assert!(check_listen(at("0.0.0.0:8077"), Some("a-token-of-20-chars.")).is_ok());
        assert!(check_listen(at("0.0.0.0:8077"), Some("a token; of 20 chars")).is_err());
        // A short token is refused here too: it would be asked for.
        assert!(check_listen(at(DEFAULT_LISTEN), Some("short")).is_err());
    }
}
