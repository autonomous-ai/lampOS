//! `lamp-seen`: what the lamp sees for its proactive skills (posture, eat
//! and drink), from the cloud perception service, on the owner's opt-in
//! (#6543, docs/seen.md).
//!
//! Phase 1 is the lamp's side without the server: no frame leaves the lamp.
//!
//! - [`gate`]: the "no frames" gate, a pure function of what the bus says
//!   and the opt-in. No frame is even asked for unless it is open.
//! - [`opt_in`]: the owner's opt-in, off unless a kept yes says otherwise,
//!   and off again after a forget.
//! - [`frames`]: a client of the camera's frame link
//!   (`lamp_camera::frames`), following its rules: one request at a time,
//!   copy before anything else, the frame's grant kept and checked, and
//!   everything dropped on a privacy close. Each copy goes to a
//!   [`frames::Sink`], the one place a frame would go to the server; in
//!   phase 1 that is [`frames::Discard`].
//! - [`crypto`]: V1's encryption, byte for byte: a session AES-256-GCM key
//!   sent under the server's RSA key (OAEP, SHA-256). No plaintext path.
//! - [`backoff`]: the server's 503 and `Retry-After`, as V1 paused on it.
//! - [`wire`]: V1's WebSocket messages for activity and pose, as types.
//! - [`watch`]: the bus readings the gate is built from.
pub mod backoff;
pub mod crypto;
pub mod frames;
pub mod gate;
pub mod opt_in;
pub mod watch;
pub mod wire;

/// The switch: `on` exactly runs the gate and, given a frame socket, the
/// frame client; anything else and the process idles, touching nothing.
pub const ENVIRONMENT: &str = "LAMP_SEEN_ACTIVITY";

/// Whether the switch's value turns it on: `on` exactly.
pub fn switched_on(value: Option<&str>) -> bool {
    value == Some("on")
}

/// This part's name on the bus: as a subscriber, a publisher and a ring
/// mark's asker (one token of `[a-z][a-z0-9_]{0,31}`, docs/bus.md).
pub const BUS_NAME: &str = "lamp_seen";

/// This part's name on `diagnostics`, as lamp-health lists it.
pub const HEALTH_NAME: &str = "seen";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_is_on_exactly() {
        assert!(switched_on(Some("on")));
        for value in [
            None,
            Some(""),
            Some("ON"),
            Some("yes"),
            Some("1"),
            Some("on "),
        ] {
            assert!(!switched_on(value), "{value:?}");
        }
    }

    #[test]
    fn the_bus_name_is_one_token() {
        let mut bytes = BUS_NAME.bytes();
        assert!(bytes.next().is_some_and(|b| b.is_ascii_lowercase()));
        assert!(bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'));
        assert!(BUS_NAME.len() <= 32);
    }
}
