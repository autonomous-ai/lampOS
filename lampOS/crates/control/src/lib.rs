//! `lamp-control`: the lamp's local programming door (#3504,
//! [docs/control.md](../../../docs/control.md)).
//!
//! A paired developer computer (and later a script skill on the lamp) speaks
//! one JSON protocol: it subscribes to events, reads state, takes a lease on
//! the parts it wants to drive, and asks for actions. Each action goes to the
//! part that already does it (the body's sockets, the ring's light requests,
//! the voice's say socket) and is held to that part's own limits; nothing
//! here can raise them.
//!
//! This first piece is the protocol's types and checks ([`protocol`]), who
//! holds which part and for how long ([`lease`]), and each client's rates
//! ([`rate`]). Nothing listens yet: the server, pairing and the routing to
//! the parts come in the pieces after it, off unless `LAMP_CONTROL=on`.

pub mod body_info;
pub mod body_lease;
pub mod cert;
pub mod feelings;
pub mod journal;
pub mod lease;
pub mod pairing;
pub mod parts;
pub mod protocol;
pub mod push;
pub mod rate;
pub mod server;
pub mod session;
pub mod watch;
