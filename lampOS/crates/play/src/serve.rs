//! One play on one connection from `lamp-control`: read the request, fetch,
//! decode, and send the sound back as it is made. The whole of what the unit
//! does for an app.
//!
//! The decode runs on the connection's own thread and each block is written
//! to the socket as it is made, so the socket's buffer, and lamp-control
//! reading it at the speaker's pace, is what paces a play: no more than the
//! buffer's worth of sound is ever ahead. A write that fails is lamp-control
//! going away, which stops the play.
use crate::decode::{BLOCK, Flow, Limits, Outcome, decode_report};
use crate::fetch::{self, FetchError, Net, SyncBody};
use crate::hosts::Allowed;
use crate::protocol::{Event, MOST_PCM, PCM, Request, read_request, write_event, write_frame};
use crate::resample::OUT_RATE;
use std::io::{Read, Write};

/// What the journal may say of a play: its id, the fixed word for how it
/// ended, how much sound and how many bytes. Never the URL or a host.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Ended {
    pub id: u64,
    pub outcome: &'static str,
    pub seconds: u64,
    pub bytes: u64,
}

/// Serves the one play of `stream`. `None` when the request was not one
/// (nothing is answered to a connection that does not speak the protocol).
pub fn serve_connection<S: Read + Write>(
    stream: &mut S,
    net: &mut dyn Net,
    limits: Limits,
) -> Option<Ended> {
    let Request::Play { id, url, hosts } = read_request(stream).ok()?;
    let ended = play(id, &url, &hosts, stream, net, limits);
    let _ = write_event(
        stream,
        &Event::Ended {
            id,
            outcome: ended.outcome,
            seconds: ended.seconds,
            bytes: ended.bytes,
        },
    );
    Some(ended)
}

fn play<S: Write>(
    id: u64,
    url: &str,
    hosts: &[String],
    stream: &mut S,
    net: &mut dyn Net,
    limits: Limits,
) -> Ended {
    let ended = |outcome, seconds, bytes| Ended {
        id,
        outcome,
        seconds,
        bytes,
    };
    let Ok(allowed) = Allowed::new(hosts) else {
        return ended("refused_host", 0, 0);
    };
    let body = match fetch::get(&allowed, url, limits.most_bytes, net) {
        Ok(body) => body,
        Err(error @ FetchError::TooBig) => return ended(error.word(), 0, 0),
        Err(error) => return ended(error.word(), 0, 0),
    };
    let mut started = false;
    let mut gone = false;
    let report = decode_report(SyncBody::new(body), limits, |block| {
        if !started {
            if write_event(stream, &Event::Started { id }).is_err() {
                gone = true;
                return Flow::Stop;
            }
            started = true;
        }
        let bytes: Vec<u8> = block
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        for part in bytes.chunks(MOST_PCM) {
            if write_frame(stream, PCM, part).is_err() {
                gone = true;
                return Flow::Stop;
            }
        }
        Flow::Continue
    });
    // A peer that went away is a stop, whatever the decoder had reached.
    let outcome = if gone {
        Outcome::Stopped
    } else {
        report.outcome
    };
    ended(
        outcome.word(),
        report.samples / u64::from(OUT_RATE),
        report.bytes,
    )
}

// A decoded block is exactly the door's block of sound, in whole samples.
const _: () = assert!(BLOCK * 2 == MOST_PCM);

#[cfg(test)]
mod tests;
