//! Room on a socket for the bus's longest datagram, where the system's
//! default is less. macOS sends a local datagram no longer than the socket's
//! send buffer, 2,048 bytes unless set (`net.local.dgram.maxdgram`), and
//! takes in no more than its receive buffer holds, 4,096 (`recvspace`): every
//! speaker, microphone and camera block was refused there. Both are the
//! socket's own and need no privilege (a probe on macOS 26: 60,000 bytes go
//! through with both raised, with neither or only one they do not). Linux's
//! defaults already take the longest, and are left as they are.
use std::{io, os::unix::net::UnixDatagram};

/// A sending socket's buffer: one longest datagram, with room to spare.
#[cfg(target_os = "macos")]
const SEND: usize = crate::wire::MOST_BYTES + 4_096;
/// A subscriber's buffer: four longest datagrams waiting, about what Linux's
/// default (208 KiB) holds.
#[cfg(target_os = "macos")]
const RECEIVE: usize = 4 * SEND;
#[cfg(target_os = "macos")]
const _: () = assert!(SEND > crate::wire::MOST_BYTES && RECEIVE >= 4 * crate::wire::MOST_BYTES);

/// Gives a publisher's socket room to send the longest datagram.
pub(crate) fn to_send(socket: &UnixDatagram) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    rustix::net::sockopt::set_socket_send_buffer_size(socket, SEND)?;
    let _ = socket;
    Ok(())
}

/// Gives a subscriber's socket room to take the longest datagram in.
pub(crate) fn to_receive(socket: &UnixDatagram) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    rustix::net::sockopt::set_socket_recv_buffer_size(socket, RECEIVE)?;
    let _ = socket;
    Ok(())
}
