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

/// Serves `stream` if `admit` says it may ask: its play, or `None` for a
/// connection that was refused or did not speak the protocol.
pub fn handle(
    mut stream: UnixStream,
    admit: &dyn Fn(&UnixStream) -> bool,
    net: &mut dyn Net,
    limits: Limits,
) -> Option<Ended> {
    if !admit(&stream) {
        return None;
    }
    let _ = stream.set_read_timeout(Some(REQUEST_WITHIN));
    serve_connection(&mut stream, net, limits)
}

#[cfg(test)]
mod tests;
