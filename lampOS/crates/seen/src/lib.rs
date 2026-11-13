//! `lamp-seen`: what the lamp sees for its proactive skills (posture, eat
//! and drink), from the cloud perception service, on the owner's opt-in
//! (#6543, docs/seen.md).
//!
//! Phase 1 is the lamp's side without the server. Phase 2 sends activity's
//! frames, in bursts, behind the gate and the ring's mark, and puts what
//! was seen on the bus (`humans/doing`, `humans/doing_seen`).
//!
//! - [`gate`]: the "no frames" gate, a pure function of what the bus says
//!   and the opt-in. No frame is even asked for unless it is open.
//! - [`opt_in`]: the owner's opt-in, off unless a kept yes says otherwise,
//!   read every 2 s and never written, and off again after a forget.
//! - [`link`]: the frame link's two boots, made once and shared by the
//!   eyes' and this part's units in the product.
//! - [`frames`]: a client of the camera's frame link
//!   (`lamp_camera::frames`), following its rules: one request at a time,
//!   copy before anything else, the frame's grant kept and checked, and
//!   everything dropped on a privacy close. Each copy goes to a
//!   [`frames::Sink`], the one place a frame would go to the server:
//!   [`activity::Activity`] when an account is given, else
//!   [`frames::Discard`].
//! - [`activity`]: one 8-frame burst a minute, each on its own socket
//!   ([`burst`]) to [`endpoint`], closed after; busy and failures retreat.
//! - [`scale`]: the camera's frame scaled down to 320×240 and encoded
//!   again before it is sent, whatever size the camera serves.
//! - [`sitting`]: a burst's labels read as drinking, eating or a break,
//!   and the sitting's timers for `humans/doing`.
//! - [`mark`]: the ring's mark before any frame: a [`frames::Sink`] that
//!   asks the ring for it and takes no frame until the ring said it drew
//!   it (fails closed).
//! - [`said_back`]: the opt-in said back once each way, through the voice's
//!   say service, when it is turned on and when it is turned off.
//! - [`crypto`]: V1's encryption, byte for byte: a session AES-256-GCM key
//!   sent under the server's RSA key (OAEP, SHA-256). No plaintext path.
//! - [`backoff`]: the server's 503 and `Retry-After`, as V1 paused on it,
//!   and activity's retreat after busy bursts and failures.
//! - [`wire`]: V1's WebSocket messages for activity and pose, as types.
//! - [`watch`]: the bus readings the gate is built from.
//! - [`pose`]: posture (phase 3): one frame every 30 s to the pose model,
//!   one socket each, its RULA score kept as this sitting's rolling window
//!   and said on `humans/posture`; off unless `LAMP_SEEN_POSTURE=on`, and
//!   no frame until an on line naming posture was said for the kept yes.
//! - [`outlet`]: activity and posture behind one ring mark per frame.
pub mod activity;
pub mod backoff;
pub mod burst;
pub mod crypto;
pub mod endpoint;
pub mod frames;
pub mod gate;
pub mod link;
pub mod mark;
pub mod opt_in;
pub mod outlet;
pub mod pose;
pub mod said_back;
pub mod scale;
pub mod sitting;
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
