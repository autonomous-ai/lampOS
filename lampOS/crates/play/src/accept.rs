//! The accept loop's one step: a connection is served only if it is admitted
//! (in the real unit: its peer is in `lamp-control.service`'s cgroup, see
//! [`crate::peer`]); anything else is closed before a byte is read and
//! nothing is fetched for it.
use crate::decode::Limits;
use crate::fetch::Net;
use crate::serve::{Ended, serve_connection};
use std::os::unix::net::UnixStream;
use std::time::Duration;

/// A caller that opens the socket and says nothing is let go after this.
pub const REQUEST_WITHIN: Duration = Duration::from_secs(5);
/// A caller that stops reading is let go after this: a write that moves
/// nothing for this long ends the play, so one stalled connection never holds
/// the one accept loop.
pub const WRITE_WITHIN: Duration = Duration::from_secs(10);

/// Serves `stream` if `admit` says it may ask: its play, or `None` for a
/// connection that was refused or did not speak the protocol.
pub fn handle(
    stream: UnixStream,
    admit: &dyn Fn(&UnixStream) -> bool,
    net: &mut dyn Net,
    limits: Limits,
) -> Option<Ended> {
    handle_within(stream, admit, net, limits, WRITE_WITHIN)
}

/// [`handle`] with the write bound given.
pub fn handle_within(
    mut stream: UnixStream,
    admit: &dyn Fn(&UnixStream) -> bool,
    net: &mut dyn Net,
    limits: Limits,
    write_within: Duration,
) -> Option<Ended> {
    if !admit(&stream) {
        return None;
    }
    let _ = stream.set_read_timeout(Some(REQUEST_WITHIN));
    let _ = stream.set_write_timeout(Some(write_within));
    serve_connection(&mut stream, net, limits)
}

#[cfg(test)]
mod tests;
