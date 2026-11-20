//! What `lamp-control` and `lamp-play` say to each other on the one unix
//! socket. Every message is a frame: 4 bytes big-endian for the length of
//! what follows, then one byte for its kind, then the payload.
//!
//! - **`J`**, JSON: lamp-control's `{"op":"play","id":N,"url":"…","hosts":["…"]}`;
//!   lamp-play's `{"ev":"started","id":N}` before the first sound and
//!   `{"ev":"ended","id":N,"outcome":"done","seconds":S,"bytes":B}` at the
//!   end.
//! - **`P`**, sound: 24 kHz mono S16LE, at most [`MOST_PCM`] bytes.
//!
//! One play for each connection. lamp-control ends it by closing the
//! connection. Nothing in a frame but the request holds a URL.
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

/// The most bytes of sound in one frame: the door's block, 9,600 bytes.
pub const MOST_PCM: usize = 9_600;
/// The most bytes in a request.
pub const MOST_REQUEST: usize = 8 * 1024;

/// A frame's kind.
pub const JSON: u8 = b'J';
pub const PCM: u8 = b'P';

/// The one request: play this URL, touching only these hosts.
#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields, tag = "op", rename_all = "snake_case")]
pub enum Request {
    Play {
        id: u64,
        url: String,
        hosts: Vec<String>,
    },
}

/// What lamp-play says as it goes.
#[derive(Debug, Serialize, Eq, PartialEq)]
#[serde(tag = "ev", rename_all = "snake_case")]
pub enum Event<'a> {
    Started {
        id: u64,
    },
    Ended {
        id: u64,
        outcome: &'a str,
        seconds: u64,
        bytes: u64,
    },
}

/// Writes one frame.
pub fn write_frame(out: &mut impl Write, kind: u8, payload: &[u8]) -> io::Result<()> {
    let length = u32::try_from(payload.len() + 1)
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    let mut frame = Vec::with_capacity(payload.len() + 5);
    frame.extend_from_slice(&length.to_be_bytes());
    frame.push(kind);
    frame.extend_from_slice(payload);
    out.write_all(&frame)
}

/// Writes an event.
pub fn write_event(out: &mut impl Write, event: &Event<'_>) -> io::Result<()> {
    let text =
        serde_json::to_vec(event).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    write_frame(out, JSON, &text)
}

/// Reads one frame of at most `most` payload bytes: its kind and payload.
pub fn read_frame(input: &mut impl Read, most: usize) -> io::Result<(u8, Vec<u8>)> {
    let mut length = [0u8; 4];
    input.read_exact(&mut length)?;
    let length = u32::from_be_bytes(length) as usize;
    if length == 0 || length > most + 1 {
        return Err(io::Error::from(io::ErrorKind::InvalidData));
    }
    let mut frame = vec![0u8; length];
    input.read_exact(&mut frame)?;
    let kind = frame.remove(0);
    Ok((kind, frame))
}

/// Reads the request: one JSON frame, nothing else.
pub fn read_request(input: &mut impl Read) -> io::Result<Request> {
    let (kind, payload) = read_frame(input, MOST_REQUEST)?;
    if kind != JSON {
        return Err(io::Error::from(io::ErrorKind::InvalidData));
    }
    serde_json::from_slice(&payload).map_err(|_| io::Error::from(io::ErrorKind::InvalidData))
}

#[cfg(test)]
mod tests;
